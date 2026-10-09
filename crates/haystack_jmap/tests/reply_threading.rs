//! Tests for the header logic that threads a reply.
//!
//! Threading is the one part of sending that is pure logic: the `In-Reply-To`
//! and `References` values are derived from the original message. RFC 5322
//! section 3.6.4 says a reply's `References` is the original's `References`
//! with the original's `Message-ID` appended, and `In-Reply-To` is the
//! original's `Message-ID`.

use haystack_jmap::types::{Email, EmailAddress, KEYWORD_ANSWERED};
use haystack_jmap::{EmailCreate, EmailPatch, ReplyRequest};
use std::collections::HashMap;

/// Builds an email carrying the threading headers.
fn original(message_id: Option<&str>, references: Option<Vec<&str>>) -> Email {
    Email {
        id: "M-original".to_string(),
        blob_id: None,
        thread_id: Some("T1".to_string()),
        subject: Some("Dinner on Thursday?".to_string()),
        from: Some(vec![EmailAddress {
            name: Some("Alice".to_string()),
            email: "alice@example.com".to_string(),
        }]),
        to: Some(vec![EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        }]),
        cc: Some(vec![EmailAddress {
            name: None,
            email: "carol@example.com".to_string(),
        }]),
        reply_to: None,
        mailbox_ids: HashMap::new(),
        keywords: HashMap::new(),
        message_id: message_id.map(|id| vec![id.to_string()]),
        in_reply_to: None,
        references: references.map(|refs| refs.into_iter().map(str::to_string).collect()),
        received_at: Some("2026-10-09T10:00:00Z".to_string()),
        sent_at: None,
        preview: None,
        size: None,
        has_attachment: None,
        body_values: HashMap::new(),
        text_body: Vec::new(),
        html_body: Vec::new(),
        body_structure: None,
        headers: HashMap::new(),
    }
}

/// A reply to a message without a `Message-ID` cannot thread; the fields stay
/// empty rather than fabricating an ID.
#[test]
fn reply_without_a_message_id_leaves_threading_empty() {
    let reply = ReplyRequest::threaded(
        EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        },
        vec![EmailAddress {
            name: None,
            email: "alice@example.com".to_string(),
        }],
        "Re: Dinner on Thursday?".to_string(),
        "Sounds good.".to_string(),
        "MB-drafts".to_string(),
        &original(None, None),
    );

    assert!(reply.in_reply_to.is_none());
    assert!(reply.references.is_none());
    assert_eq!(reply.in_reply_to_email_id.as_deref(), Some("M-original"));
    // The original Cc list is carried onto the reply.
    assert_eq!(reply.cc[0].email, "carol@example.com");
}

/// A reply inherits the original references and appends the parent message ID.
#[test]
fn reply_appends_the_parent_message_id_to_references() {
    let reply = ReplyRequest::threaded(
        EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        },
        vec![EmailAddress {
            name: None,
            email: "alice@example.com".to_string(),
        }],
        "Re: Dinner on Thursday?".to_string(),
        "Sounds good.".to_string(),
        "MB-drafts".to_string(),
        &original(
            Some("<parent@example.com>"),
            Some(vec!["<root@example.com>"]),
        ),
    );

    assert_eq!(
        reply.in_reply_to,
        Some(vec!["<parent@example.com>".to_string()])
    );
    assert_eq!(
        reply.references,
        Some(vec![
            "<root@example.com>".to_string(),
            "<parent@example.com>".to_string(),
        ])
    );
}

/// A message that already references its own message ID is not duplicated.
#[test]
fn reply_does_not_duplicate_an_existing_reference() {
    let reply = ReplyRequest::threaded(
        EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        },
        Vec::new(),
        "Re: Looping".to_string(),
        "Body".to_string(),
        "MB-drafts".to_string(),
        &original(
            Some("<parent@example.com>"),
            Some(vec!["<root@example.com>", "<parent@example.com>"]),
        ),
    );

    assert_eq!(
        reply.references,
        Some(vec![
            "<root@example.com>".to_string(),
            "<parent@example.com>".to_string(),
        ])
    );
}

/// The reply renders as a JMAP create with the threading headers set, and a
/// builder override can set the advertised `Message-ID`.
#[test]
fn reply_renders_threading_headers_into_the_create() {
    let reply = ReplyRequest::threaded(
        EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        },
        vec![EmailAddress {
            name: None,
            email: "alice@example.com".to_string(),
        }],
        "Re: Dinner on Thursday?".to_string(),
        "Sounds good.".to_string(),
        "MB-drafts".to_string(),
        &original(Some("<parent@example.com>"), None),
    )
    .with_message_id("<reply@example.com>");

    let mut create = EmailCreate::new_text(
        "reply",
        reply.from.clone(),
        reply.to.clone(),
        reply.subject.clone(),
        reply.text_body.clone(),
    )
    .in_mailbox(reply.drafts_mailbox_id.clone());
    if let Some(message_id) = &reply.message_id {
        create = create.with_message_id(message_id.clone());
    }
    if let Some(in_reply_to) = &reply.in_reply_to {
        create = create.in_reply_to(in_reply_to.clone());
    }
    if let Some(references) = &reply.references {
        create = create.references(references.clone());
    }

    let rendered = create.to_json();
    assert_eq!(
        rendered["messageId"],
        serde_json::json!(["<reply@example.com>"])
    );
    assert_eq!(
        rendered["inReplyTo"],
        serde_json::json!(["<parent@example.com>"])
    );
    assert_eq!(
        rendered["references"],
        serde_json::json!(["<parent@example.com>"])
    );
    // The subject and recipient survive the round trip.
    assert_eq!(
        rendered["subject"],
        serde_json::json!("Re: Dinner on Thursday?")
    );
    assert_eq!(
        rendered["to"][0]["email"],
        serde_json::json!("alice@example.com")
    );
}

/// The answer flag is applied through the same patch the send path uses.
#[test]
fn answered_flag_patch_matches_the_send_path() {
    let patch = EmailPatch::new("M-original").mark_seen().mark_answered();
    assert_eq!(
        patch.to_patch_object(),
        serde_json::json!({
            "keywords/$seen": true,
            "keywords/$answered": true
        })
    );
    assert_eq!(KEYWORD_ANSWERED, "$answered");
}
