//! Haystack integration for email via JMAP.
//!
//! Implements [`HaystackProvider`] over a JMAP mail server, allowing email
//! messages and threads to be searched as Terraphim haystack documents.
//!
//! Beyond search, the crate exposes the JMAP calls tinyclaw's email channel
//! needs:
//!
//! - [`JMAPClient::query_email_ids`] and
//!   [`JMAPClient::query_emails_in_mailbox`] for `Email/query` scoped to a
//!   mailbox.
//! - [`JMAPClient::email_changes`] for a durable `Email/changes` cursor backed
//!   by a [`StateCursorStore`].
//! - [`JMAPClient::fetch_changed_emails_in_mailbox`] for the mailbox-scoped
//!   subset of those changes, because `Email/changes` itself is account-wide.
//! - [`JMAPClient::get_emails_with_options`] for `Email/get` with threading,
//!   recipients, keywords and selected headers.
//! - [`JMAPClient::update_emails`], [`JMAPClient::mark_seen`] and
//!   [`JMAPClient::move_emails`] for `Email/set` keyword and mailbox changes.
//! - [`JMAPClient::create_drafts`], [`JMAPClient::submit_emails`] and
//!   [`JMAPClient::send_reply`] for `Email/set` plus `EmailSubmission/set`.
//!
//! Wire types live in [`types`], request building in [`request`], result
//! parsing in [`response`] and cursor persistence in [`cursor`].

pub mod client;
pub mod cursor;
pub mod request;
pub mod response;
pub mod types;

pub use client::{JMAPClient, ReplyOutcome, ReplyRequest, Session};
pub use cursor::{FileCursorStore, MemoryCursorStore, StateCursorStore};
pub use request::{
    ChangesOptions, EmailCreate, EmailGetOptions, EmailPatch, EmailQueryFilter, EmailSubmission,
    Envelope, JMAPError, JMAPRequest, JMAPResponse, MethodCall, MethodResponse,
};
pub use response::{
    EmailChangesResult, EmailGetResult, EmailQueryResult, IdentityGetResult, MailboxGetResult,
    SetCreated, SetError, SetResult,
};
pub use types::{
    BodyPart, BodyValue, CAPABILITY_CORE, CAPABILITY_MAIL, CAPABILITY_SUBMISSION, ChangesState,
    EMAIL_HEADER_NAMES, EMAIL_PROPERTIES, Email, EmailAddress, EmailChanges, Identity,
    KEYWORD_ANSWERED, KEYWORD_DRAFT, KEYWORD_FLAGGED, KEYWORD_SEEN, Mailbox, MailboxList,
    ROLE_ARCHIVE, ROLE_DRAFTS, ROLE_INBOX, ROLE_JUNK, ROLE_SENT, ROLE_TRASH,
};

use anyhow::Result;
use haystack_core::HaystackProvider;
use terraphim_types::{Document, SearchQuery};

/// Converts an Email into a Document with enriched metadata.
pub fn email_to_document(email: &Email) -> Document {
    let sender = email
        .from
        .as_ref()
        .and_then(|addrs| addrs.first())
        .map(|a| a.email.clone())
        .unwrap_or_default();

    let recipient = email
        .to
        .as_ref()
        .and_then(|addrs| addrs.first())
        .map(|a| a.email.clone())
        .unwrap_or_default();

    let description = Some(format!("From: {} To: {}", sender, recipient));

    let body_text = email
        .body_values
        .values()
        .next()
        .map(|bv| bv.value.clone())
        .unwrap_or_default();

    let stub = if body_text.is_empty() {
        None
    } else {
        Some(body_text.chars().take(200).collect::<String>())
    };

    let mut tags = vec!["email".to_string()];
    if !sender.is_empty() {
        tags.push(format!("sender:{}", sender));
    }
    if let Some(ref date) = email.received_at
        && let Some(date_part) = date.split('T').next()
    {
        tags.push(date_part.to_string());
    }

    let url = format!("jmap:///email/{}", email.id);

    Document {
        id: email.id.clone(),
        title: email.subject.clone().unwrap_or_default(),
        body: body_text,
        url,
        description,
        stub,
        tags: Some(tags),
        summarization: None,
        rank: None,
        source_haystack: None,
        doc_type: terraphim_types::DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    }
}

impl HaystackProvider for JMAPClient {
    type Error = anyhow::Error;

    async fn search(&self, query: &SearchQuery) -> Result<Vec<Document>, Self::Error> {
        let emails = self
            .search_emails(&query.search_term.to_string(), 50)
            .await?;
        Ok(emails.iter().map(email_to_document).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_session_json() -> &'static str {
        r#"{
            "primaryAccounts": {
                "urn:ietf:params:jmap:mail": "acc-001",
                "urn:ietf:params:jmap:contacts": "acc-001"
            },
            "apiUrl": "https://jmap.example.com/api/",
            "capabilities": {},
            "downloadUrl": "https://jmap.example.com/download/",
            "uploadUrl": "https://jmap.example.com/upload/",
            "state": "abc123",
            "username": "user@example.com"
        }"#
    }

    fn sample_email() -> Email {
        let mut body_values = HashMap::new();
        body_values.insert(
            "1".to_string(),
            BodyValue {
                value: "Hello, this is the email body content.".to_string(),
                is_truncated: Some(false),
                content_type: None,
            },
        );
        Email {
            id: "email-001".to_string(),
            blob_id: None,
            thread_id: None,
            subject: Some("Test Subject".to_string()),
            from: Some(vec![EmailAddress {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }]),
            to: Some(vec![EmailAddress {
                name: Some("Bob".to_string()),
                email: "bob@example.com".to_string(),
            }]),
            cc: None,
            reply_to: None,
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            message_id: None,
            in_reply_to: None,
            references: None,
            received_at: Some("2025-01-15T10:30:00Z".to_string()),
            sent_at: None,
            preview: None,
            size: None,
            has_attachment: None,
            body_values,
            text_body: vec![BodyPart {
                part_id: Some("1".to_string()),
                type_: Some("text/plain".to_string()),
                size: None,
                charset: None,
            }],
            html_body: vec![],
            body_structure: None,
            headers: HashMap::new(),
        }
    }

    #[test]
    fn test_session_deserialization() {
        let session: Session = serde_json::from_str(sample_session_json()).unwrap();
        assert_eq!(session.api_url, "https://jmap.example.com/api/");
        assert_eq!(session.username, "user@example.com");
        assert_eq!(
            session.primary_accounts.get("urn:ietf:params:jmap:mail"),
            Some(&"acc-001".to_string())
        );
    }

    #[test]
    fn test_email_deserialization() {
        let json = r#"{
            "id": "e-123",
            "subject": "Meeting Tomorrow",
            "from": [{"name": "Alice", "email": "alice@test.com"}],
            "to": [{"name": "Bob", "email": "bob@test.com"}],
            "bodyValues": {
                "1": {"value": "See you at 3pm", "isTruncated": false}
            },
            "textBody": [{"partId": "1"}],
            "receivedAt": "2025-03-01T14:00:00Z"
        }"#;
        let email: Email = serde_json::from_str(json).unwrap();
        assert_eq!(email.id, "e-123");
        assert_eq!(email.subject, Some("Meeting Tomorrow".to_string()));
        assert_eq!(email.from.as_ref().unwrap()[0].email, "alice@test.com");
        assert_eq!(email.body_values.get("1").unwrap().value, "See you at 3pm");
        assert_eq!(email.received_at, Some("2025-03-01T14:00:00Z".to_string()));
    }

    #[test]
    fn test_email_to_document_mapping() {
        let email = sample_email();
        let doc = email_to_document(&email);

        assert_eq!(doc.id, "email-001");
        assert_eq!(doc.title, "Test Subject");
        assert_eq!(doc.url, "jmap:///email/email-001");
        assert_eq!(
            doc.description,
            Some("From: alice@example.com To: bob@example.com".to_string())
        );
        assert_eq!(doc.body, "Hello, this is the email body content.");
        assert_eq!(
            doc.stub,
            Some("Hello, this is the email body content.".to_string())
        );

        let tags = doc.tags.unwrap();
        assert!(tags.contains(&"email".to_string()));
        assert!(tags.contains(&"sender:alice@example.com".to_string()));
        assert!(tags.contains(&"2025-01-15".to_string()));
    }

    #[test]
    fn test_email_to_document_empty_fields() {
        let email = Email {
            id: "empty-001".to_string(),
            subject: None,
            from: None,
            to: None,
            ..sample_email()
        };
        let doc = email_to_document(&email);

        assert_eq!(doc.id, "empty-001");
        assert_eq!(doc.title, "");
        assert_eq!(doc.description, Some("From:  To: ".to_string()));

        let tags = doc.tags.unwrap();
        assert!(tags.contains(&"email".to_string()));
        assert!(!tags.iter().any(|t| t.starts_with("sender:")));
    }

    #[test]
    fn test_email_to_document_stub_truncation() {
        let long_body = "A".repeat(500);
        let mut body_values = HashMap::new();
        body_values.insert(
            "1".to_string(),
            BodyValue {
                value: long_body,
                is_truncated: None,
                content_type: None,
            },
        );
        let email = Email {
            id: "long-001".to_string(),
            subject: Some("Long Email".to_string()),
            from: None,
            to: None,
            body_values,
            text_body: vec![],
            ..sample_email()
        };
        let doc = email_to_document(&email);

        let stub = doc.stub.unwrap();
        assert_eq!(stub.len(), 200);
        assert_eq!(stub, "A".repeat(200));
    }
}
