//! JMAP data types for mail, mailboxes, identities and submission.
//!
//! Field names follow RFC 8621 (`urn:ietf:params:jmap:mail` and
//! `urn:ietf:params:jmap:submission`) so that values round-trip through
//! `serde_json` without renaming.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Capability URN for the JMAP core specification (RFC 8620).
pub const CAPABILITY_CORE: &str = "urn:ietf:params:jmap:core";
/// Capability URN for the JMAP mail specification (RFC 8621).
pub const CAPABILITY_MAIL: &str = "urn:ietf:params:jmap:mail";
/// Capability URN for the JMAP submission specification (RFC 8621).
pub const CAPABILITY_SUBMISSION: &str = "urn:ietf:params:jmap:submission";

/// Role name for the inbox mailbox (RFC 8621 section 2).
pub const ROLE_INBOX: &str = "inbox";
/// Role name for the archive mailbox (RFC 8621 section 2).
pub const ROLE_ARCHIVE: &str = "archive";
/// Role name for the drafts mailbox (RFC 8621 section 2).
pub const ROLE_DRAFTS: &str = "drafts";
/// Role name for the sent mailbox (RFC 8621 section 2).
pub const ROLE_SENT: &str = "sent";
/// Role name for the trash mailbox (RFC 8621 section 2).
pub const ROLE_TRASH: &str = "trash";
/// Role name for the junk mailbox (RFC 8621 section 2).
pub const ROLE_JUNK: &str = "junk";

/// Keyword applied to a message the user has read (RFC 8621 section 4.1.1).
pub const KEYWORD_SEEN: &str = "$seen";
/// Keyword applied to a flagged or starred message.
pub const KEYWORD_FLAGGED: &str = "$flagged";
/// Keyword applied to a message the user has replied to.
pub const KEYWORD_ANSWERED: &str = "$answered";
/// Keyword applied to a message still being composed.
pub const KEYWORD_DRAFT: &str = "$draft";

/// The properties of an email that this client knows how to request and parse.
///
/// The list mixes message properties, generic header properties (every name
/// starting with `header:`) and the header names needed for threading and the
/// own-submission trust check described in the tinyclaw design.
pub const EMAIL_PROPERTIES: &[&str] = &[
    "id",
    "blobId",
    "threadId",
    "mailboxIds",
    "keywords",
    "size",
    "receivedAt",
    "sentAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "replyTo",
    "subject",
    "preview",
    "hasAttachment",
    "bodyStructure",
    "bodyValues",
    "textBody",
    "htmlBody",
    "header:Authentication-Results:asText",
    "header:Received:asText",
    "header:X-ME-Message-Id:asRaw",
    "header:X-ME-User:asRaw",
];

/// The header names carried alongside a message, keyed by lower-case name.
///
/// Only the headers named here are requested and preserved; other headers are
/// dropped when a message is converted into this map.
pub const EMAIL_HEADER_NAMES: &[&str] = &[
    "message-id",
    "in-reply-to",
    "references",
    "received",
    "authentication-results",
];

/// A single email message.
///
/// Every field past `id` is optional because JMAP returns only the properties a
/// client asked for. Deserialising a response that omitted a property leaves the
/// field `None` or empty, and re-serialising preserves the same shape.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Email {
    /// The ID of the email.
    pub id: String,

    /// The ID of the blob holding the raw RFC 5322 message, if requested.
    #[serde(rename = "blobId", default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<String>,

    /// The ID of the thread this message belongs to.
    #[serde(rename = "threadId", default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,

    /// The subject of the email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// The sender(s) of the email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Vec<EmailAddress>>,

    /// The "To" recipient(s) of the email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<EmailAddress>>,

    /// The "Cc" recipient(s) of the email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cc: Option<Vec<EmailAddress>>,

    /// The "Reply-To" address(es) of the email.
    #[serde(rename = "replyTo", default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,

    /// The mailbox IDs the message belongs to, mapped to `true`.
    #[serde(
        rename = "mailboxIds",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub mailbox_ids: HashMap<String, bool>,

    /// The keywords set on the message, mapped to `true`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub keywords: HashMap<String, bool>,

    /// The RFC 5322 `Message-ID` header value(s).
    #[serde(rename = "messageId", default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<Vec<String>>,

    /// The RFC 5322 `In-Reply-To` header value(s).
    #[serde(rename = "inReplyTo", default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<Vec<String>>,

    /// The RFC 5322 `References` header value(s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<String>>,

    /// The date and time the email was received.
    #[serde(
        rename = "receivedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub received_at: Option<String>,

    /// The date and time the email was sent.
    #[serde(rename = "sentAt", default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,

    /// A short plain-text preview of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,

    /// The size of the message in octets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Whether the message has at least one attachment.
    #[serde(
        rename = "hasAttachment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub has_attachment: Option<bool>,

    /// The body values of the email, keyed by part ID.
    #[serde(
        rename = "bodyValues",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub body_values: HashMap<String, BodyValue>,

    /// The text body parts of the email.
    #[serde(rename = "textBody", default, skip_serializing_if = "Vec::is_empty")]
    pub text_body: Vec<BodyPart>,

    /// The HTML body parts of the email.
    #[serde(rename = "htmlBody", default, skip_serializing_if = "Vec::is_empty")]
    pub html_body: Vec<BodyPart>,

    /// Parsed body structure of the message.
    #[serde(
        rename = "bodyStructure",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub body_structure: Option<serde_json::Value>,

    /// Additional headers requested through the `header:*` property form, keyed
    /// by the name given in the request. Only header properties present in the
    /// response appear here.
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, serde_json::Value>,
}

/// Represents an email address.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EmailAddress {
    /// The display name associated with the email address, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The email address itself.
    pub email: String,
}

/// Represents the value of an email body part.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BodyValue {
    /// The content of the body part.
    pub value: String,

    /// Whether the content is truncated.
    #[serde(
        rename = "isTruncated",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub is_truncated: Option<bool>,

    /// The content type of the body part, when the server reports it.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_type: Option<String>,
}

/// Represents an email body part.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BodyPart {
    /// The ID of the body part.
    #[serde(rename = "partId", default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,

    /// The content type of the body part.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    /// The size of the body part in octets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// The character set of the body part.
    #[serde(rename = "charset", default, skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
}

/// A mailbox as returned by `Mailbox/get`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Mailbox {
    /// The ID of the mailbox.
    pub id: String,

    /// The user-visible name of the mailbox.
    #[serde(default)]
    pub name: String,

    /// The role of the mailbox (`inbox`, `archive`, `junk`, ...), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// The ID of the parent mailbox, if this is a child mailbox.
    #[serde(rename = "parentId", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// The number of messages in the mailbox, excluding those in child mailboxes.
    #[serde(
        rename = "totalEmails",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub total_emails: Option<u64>,

    /// The number of unread messages in the mailbox.
    #[serde(
        rename = "unreadEmails",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unread_emails: Option<u64>,

    /// Whether the mailbox is subscribed to. The wire property is
    /// `isSubscribed` (RFC 8621 section 2).
    #[serde(
        rename = "isSubscribed",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub subscribed: Option<bool>,
}

impl Mailbox {
    /// Returns true when the mailbox carries the given role, compared
    /// case-insensitively.
    pub fn has_role(&self, role: &str) -> bool {
        self.role
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case(role))
    }

    /// Returns true when the mailbox name matches, compared
    /// case-insensitively.
    pub fn has_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// An identity as returned by `Identity/get`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The ID of the identity.
    pub id: String,

    /// The display name to use when sending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The email address to send from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// The `Reply-To` address to advertise, if the server supplies one.
    #[serde(rename = "replyTo", default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,

    /// The signature to append to text bodies.
    #[serde(
        rename = "textSignature",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub text_signature: Option<String>,

    /// Whether the server allows this identity to be deleted.
    #[serde(rename = "mayDelete", default, skip_serializing_if = "Option::is_none")]
    pub may_delete: Option<bool>,
}

impl Identity {
    /// Returns the address of this identity, or an empty string when the server
    /// supplied no email address.
    pub fn email_or_empty(&self) -> &str {
        self.email.as_deref().unwrap_or("")
    }
}

/// The state string returned by `Email/get`, `Email/query` and `Email/changes`.
///
/// The state is an opaque server token. Persist it verbatim and hand it back as
/// the `sinceState` of the next `Email/changes` call.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChangesState {
    /// The account the state belongs to.
    pub account_id: String,

    /// The opaque state string.
    pub state: String,
}

impl ChangesState {
    /// Creates a state record for the given account.
    pub fn new(account_id: impl Into<String>, state: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            state: state.into(),
        }
    }
}

/// The result of an `Email/changes` call.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EmailChanges {
    /// The account the changes belong to.
    pub account_id: String,

    /// The state to use as `sinceState` on the next call.
    pub new_state: String,

    /// The state the client asked from, echoed by the server.
    pub old_state: String,

    /// Whether the server could not compute the changes since `old_state`.
    ///
    /// When true the caller must discard its cursor and re-query the mailbox
    /// from scratch.
    pub cannot_calculate_changes: bool,

    /// The IDs of messages created since `old_state`.
    pub created: Vec<String>,

    /// The IDs of messages updated since `old_state`.
    pub updated: Vec<String>,

    /// The IDs of messages destroyed since `old_state`.
    pub destroyed: Vec<String>,

    /// The total number of messages in the mailbox at `new_state`, when the
    /// server reports it.
    pub total: Option<u64>,
}

impl EmailChanges {
    /// Returns the IDs a re-fetch should ask for: created and updated, without
    /// the destroyed ones.
    ///
    /// This is the ID list [`crate::JMAPClient::fetch_changed_emails`] builds.
    /// Destroyed IDs are excluded because [`crate::JMAPClient::get_emails`] on a
    /// deleted message merely reports it in `notFound`.
    pub fn fetchable_ids(&self) -> Vec<String> {
        let mut ids = Vec::with_capacity(self.created.len() + self.updated.len());
        ids.extend(self.created.iter().cloned());
        ids.extend(self.updated.iter().cloned());
        ids
    }

    /// Returns every ID that a re-fetch should ask for, in the order created,
    /// updated, destroyed.
    pub fn affected_ids(&self) -> Vec<String> {
        let mut ids =
            Vec::with_capacity(self.created.len() + self.updated.len() + self.destroyed.len());
        ids.extend(self.created.iter().cloned());
        ids.extend(self.updated.iter().cloned());
        ids.extend(self.destroyed.iter().cloned());
        ids
    }
}

/// The result of a `Mailbox/get` call.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MailboxList {
    /// The account the mailboxes belong to.
    pub account_id: String,

    /// The state string of the mailbox list.
    pub state: String,

    /// The mailboxes.
    pub list: Vec<Mailbox>,
}

impl MailboxList {
    /// Finds a mailbox by role, comparing case-insensitively.
    pub fn by_role(&self, role: &str) -> Option<&Mailbox> {
        self.list.iter().find(|m| m.has_role(role))
    }

    /// Finds a mailbox by name, comparing case-insensitively.
    pub fn by_name(&self, name: &str) -> Option<&Mailbox> {
        self.list.iter().find(|m| m.has_name(name))
    }
}
