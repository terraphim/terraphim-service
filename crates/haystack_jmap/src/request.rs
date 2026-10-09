//! JMAP request building and JSON-RPC envelope types.
//!
//! The envelope follows RFC 8620 section 3: a request carries `using`,
//! `methodCalls` and an optional `createdIds`; a response carries
//! `methodResponses`, `sessionState` and, for `*/set`, `createdIds`.
use crate::types::{
    CAPABILITY_CORE, CAPABILITY_MAIL, CAPABILITY_SUBMISSION, EMAIL_HEADER_NAMES, EMAIL_PROPERTIES,
    EmailAddress, KEYWORD_ANSWERED, KEYWORD_DRAFT, KEYWORD_FLAGGED, KEYWORD_SEEN,
};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

/// A single JMAP method call: name, arguments and client-supplied call ID.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct MethodCall(
    /// The name of the method being called.
    pub String,
    /// The arguments for the method call.
    pub Value,
    /// The client-specified method call ID.
    pub String,
);

/// A JMAP request envelope.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct JMAPRequest {
    /// The set of capabilities being used in the request.
    pub using: Vec<String>,

    /// The method calls included in the request.
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<MethodCall>,

    /// Client-created record IDs that later calls may reference. Omitted when
    /// empty.
    #[serde(
        rename = "createdIds",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub created_ids: Option<HashMap<String, String>>,
}

/// A JMAP method response: name, arguments and the matching call ID.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct MethodResponse(
    /// The name of the method that was called.
    pub String,
    /// The result of the method call, or a `notFound` / `error` argument set.
    pub Value,
    /// The client-specified method call ID.
    pub String,
);

/// A JMAP response envelope.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct JMAPResponse {
    /// The method responses included in the response.
    #[serde(rename = "methodResponses", default)]
    pub method_responses: Vec<MethodResponse>,

    /// The updated state string for the session.
    #[serde(rename = "sessionState", default)]
    pub session_state: String,

    /// IDs the server assigned to records the client asked it to create.
    #[serde(
        rename = "createdIds",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub created_ids: Option<HashMap<String, String>>,
}

/// Details of a JMAP method-level error (`type` plus optional `description`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct JMAPError {
    /// The RFC 8620 error type, for example `cannotCalculateChanges`.
    #[serde(rename = "type")]
    pub type_: String,

    /// A human-readable description, when the server supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl JMAPError {
    /// Returns true when the error is the `cannotCalculateChanges` type.
    pub fn is_cannot_calculate_changes(&self) -> bool {
        self.type_ == "cannotCalculateChanges"
    }
}

impl std::fmt::Display for JMAPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.description {
            Some(d) => write!(f, "{}: {}", self.type_, d),
            None => write!(f, "{}", self.type_),
        }
    }
}

impl JMAPResponse {
    /// Returns the method response for the given call ID, if the server
    /// answered that call.
    pub fn response_for(&self, call_id: &str) -> Option<&MethodResponse> {
        self.method_responses.iter().find(|r| r.2 == call_id)
    }

    /// Returns the leading `error` method response, if the server reported one.
    pub fn error(&self) -> Option<JMAPError> {
        self.method_responses
            .iter()
            .find(|r| r.0 == "error")
            .and_then(|r| serde_json::from_value(r.1.clone()).ok())
    }

    /// Returns the result object of the given call, or an error describing why
    /// the result is missing.
    pub fn result(&self, call_id: &str) -> Result<&Value> {
        if let Some(err) = self.error() {
            return Err(anyhow!("JMAP method error: {}", err));
        }
        let response = self
            .response_for(call_id)
            .ok_or_else(|| anyhow!("No JMAP response for method call '{call_id}'"))?;
        if response.0 == "error" {
            let err: JMAPError =
                serde_json::from_value(response.1.clone()).unwrap_or_else(|_| JMAPError {
                    type_: "unknownMethodError".to_string(),
                    description: None,
                });
            return Err(anyhow!("JMAP method error for '{call_id}': {}", err));
        }
        Ok(&response.1)
    }
}

/// Builds a request envelope using the core and mail capabilities.
pub fn core_mail_request(calls: Vec<MethodCall>) -> JMAPRequest {
    JMAPRequest {
        using: vec![CAPABILITY_CORE.to_string(), CAPABILITY_MAIL.to_string()],
        method_calls: calls,
        created_ids: None,
    }
}

/// Builds a request envelope using the core, mail and submission capabilities.
pub fn submission_request(calls: Vec<MethodCall>) -> JMAPRequest {
    JMAPRequest {
        using: vec![
            CAPABILITY_CORE.to_string(),
            CAPABILITY_MAIL.to_string(),
            CAPABILITY_SUBMISSION.to_string(),
        ],
        method_calls: calls,
        created_ids: None,
    }
}

/// Builds a `Mailbox/get` call fetching the full mailbox list.
pub fn mailbox_get_call(account_id: &str, call_id: &str) -> MethodCall {
    MethodCall(
        "Mailbox/get".to_string(),
        json!({
            "accountId": account_id,
            "properties": ["id", "name", "role", "parentId", "totalEmails", "unreadEmails", "subscribed"],
        }),
        call_id.to_string(),
    )
}

/// Builds an `Identity/get` call fetching every identity on the account.
pub fn identity_get_call(account_id: &str, call_id: &str) -> MethodCall {
    MethodCall(
        "Identity/get".to_string(),
        json!({ "accountId": account_id }),
        call_id.to_string(),
    )
}

/// A filter for `Email/query`.
///
/// Use [`EmailQueryFilter::in_mailbox`] to scope a query to one mailbox and
/// [`EmailQueryFilter::and`] to combine filters.
#[derive(Debug, Clone, PartialEq)]
pub enum EmailQueryFilter {
    /// Match every message.
    All,
    /// Match messages in the given mailbox.
    InMailbox(String),
    /// Match unread messages.
    NotKeyword(String),
    /// Match messages carrying the given keyword.
    HasKeyword(String),
    /// Match messages whose subject or body contains the text.
    Text(String),
    /// Match messages received at or after the given UTCDate.
    After(String),
    /// Match messages received before the given UTCDate.
    Before(String),
    /// Match every one of the child filters.
    And(Vec<EmailQueryFilter>),
    /// Match at least one of the child filters.
    Or(Vec<EmailQueryFilter>),
    /// Match none of the child filters.
    Not(Box<EmailQueryFilter>),
}

impl EmailQueryFilter {
    /// Scopes a query to a single mailbox.
    pub fn in_mailbox(mailbox_id: impl Into<String>) -> Self {
        Self::InMailbox(mailbox_id.into())
    }

    /// Combines filters with AND semantics.
    pub fn and(filters: Vec<EmailQueryFilter>) -> Self {
        Self::And(filters)
    }

    /// Combines filters with OR semantics.
    pub fn or(filters: Vec<EmailQueryFilter>) -> Self {
        Self::Or(filters)
    }

    /// Negates a filter.
    pub fn negate(filter: EmailQueryFilter) -> Self {
        Self::Not(Box::new(filter))
    }

    /// Renders the filter as the RFC 8621 `FilterCondition` / `FilterOperator`
    /// JSON form.
    pub fn to_json(&self) -> Value {
        match self {
            Self::All => json!({}),
            Self::InMailbox(id) => json!({ "inMailbox": id }),
            Self::NotKeyword(keyword) => json!({ "notKeyword": keyword }),
            Self::HasKeyword(keyword) => json!({ "hasKeyword": keyword }),
            Self::Text(text) => json!({ "text": text }),
            Self::After(utc_date) => json!({ "after": utc_date }),
            Self::Before(utc_date) => json!({ "before": utc_date }),
            Self::And(filters) => json!({
                "operator": "AND",
                "conditions": filters.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            Self::Or(filters) => json!({
                "operator": "OR",
                "conditions": filters.iter().map(Self::to_json).collect::<Vec<_>>(),
            }),
            Self::Not(filter) => json!({ "operator": "NOT", "conditions": [filter.to_json()] }),
        }
    }
}

/// Builds an `Email/query` call, optionally filtered and sorted.
pub fn email_query_call(
    account_id: &str,
    filter: Option<&EmailQueryFilter>,
    limit: Option<u32>,
    position: Option<u32>,
    call_id: &str,
) -> MethodCall {
    let mut args = serde_json::Map::new();
    args.insert("accountId".to_string(), json!(account_id));
    if let Some(filter) = filter {
        args.insert("filter".to_string(), filter.to_json());
    }
    if let Some(limit) = limit {
        args.insert("limit".to_string(), json!(limit));
    }
    if let Some(position) = position {
        args.insert("position".to_string(), json!(position));
    }
    MethodCall(
        "Email/query".to_string(),
        Value::Object(args),
        call_id.to_string(),
    )
}

/// The properties carried by an `Email/changes` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesOptions {
    /// The maximum number of changed IDs to return in one call.
    pub max_changes: Option<u32>,

    /// Whether the server should also report messages moved into or out of the
    /// mailbox.
    pub include_updated: bool,
}

impl Default for ChangesOptions {
    fn default() -> Self {
        Self {
            max_changes: None,
            include_updated: true,
        }
    }
}

/// Builds an `Email/changes` call using the persisted cursor as `sinceState`.
pub fn email_changes_call(
    account_id: &str,
    since_state: &str,
    options: &ChangesOptions,
    call_id: &str,
) -> MethodCall {
    let mut args = serde_json::Map::new();
    args.insert("accountId".to_string(), json!(account_id));
    args.insert("sinceState".to_string(), json!(since_state));
    args.insert(
        "properties".to_string(),
        json!(["id", "threadId", "mailboxIds", "keywords"]),
    );
    if let Some(max_changes) = options.max_changes {
        args.insert("maxChanges".to_string(), json!(max_changes));
    }
    MethodCall(
        "Email/changes".to_string(),
        Value::Object(args),
        call_id.to_string(),
    )
}

/// Which `Email/get` properties to request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailGetOptions {
    /// The properties to request. Empty means "the default set".
    pub properties: Vec<String>,

    /// Whether the server should expand `bodyValues` for the text body.
    pub fetch_text_body_values: bool,

    /// Whether the server should expand `bodyValues` for the HTML body.
    pub fetch_html_body_values: bool,

    /// The maximum body value size in octets, when limiting.
    pub max_body_value_bytes: Option<u64>,
}

impl Default for EmailGetOptions {
    fn default() -> Self {
        Self {
            properties: Vec::new(),
            fetch_text_body_values: true,
            fetch_html_body_values: false,
            max_body_value_bytes: None,
        }
    }
}

impl EmailGetOptions {
    /// Requests the default property set: message metadata, threading fields,
    /// keywords and the headers named in [`EMAIL_HEADER_NAMES`].
    pub fn full() -> Self {
        let mut properties: Vec<String> =
            EMAIL_PROPERTIES.iter().map(|p| (*p).to_string()).collect();
        for name in EMAIL_HEADER_NAMES {
            properties.push(format!("header:{name}:asText"));
        }
        Self {
            properties,
            ..Self::default()
        }
    }

    /// Requests only the given properties.
    pub fn with_properties<I, S>(properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            properties: properties.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

/// Builds an `Email/get` call for the given IDs.
pub fn email_get_call(
    account_id: &str,
    ids: &[String],
    options: &EmailGetOptions,
    call_id: &str,
) -> MethodCall {
    let mut args = serde_json::Map::new();
    args.insert("accountId".to_string(), json!(account_id));
    args.insert("ids".to_string(), json!(ids));
    if !options.properties.is_empty() {
        args.insert("properties".to_string(), json!(options.properties));
    }
    if options.fetch_text_body_values || options.fetch_html_body_values {
        args.insert(
            "fetchTextBodyValues".to_string(),
            json!(options.fetch_text_body_values),
        );
        args.insert(
            "fetchHTMLBodyValues".to_string(),
            json!(options.fetch_html_body_values),
        );
    }
    if let Some(bytes) = options.max_body_value_bytes {
        args.insert("maxBodyValueBytes".to_string(), json!(bytes));
    }
    MethodCall(
        "Email/get".to_string(),
        Value::Object(args),
        call_id.to_string(),
    )
}

/// Builds an `Email/get` call resolving the IDs produced by `Email/query`.
///
/// The `#ids` reference form asks the server to reuse the query result, so a
/// single request can query and fetch without a second round trip.
pub fn email_get_referencing_query_call(
    account_id: &str,
    query_call_id: &str,
    options: &EmailGetOptions,
    call_id: &str,
) -> MethodCall {
    let mut call = email_get_call(account_id, &[], options, call_id);
    if let Value::Object(ref mut args) = call.1 {
        args.remove("ids");
        args.insert(
            "#ids".to_string(),
            json!({
                "resultOf": query_call_id,
                "name": "Email/query",
                "path": "/ids"
            }),
        );
    }
    call
}

/// A patch applied to a single email by `Email/set`.
///
/// `patch` serialises to the RFC 8621 PatchObject form, so `mailboxIds/x` and
/// `keywords/$seen` become the keys of the patch. A removal is rendered as
/// `null`, which is how RFC 8620 section 5.3 expresses "delete this key".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailPatch {
    /// The email to update.
    pub id: String,

    /// Keyword additions (true) and removals (null), keyed by keyword name.
    pub keywords: HashMap<String, Option<bool>>,

    /// Mailbox additions (true) and removals (null), keyed by mailbox ID.
    pub mailbox_ids: HashMap<String, Option<bool>>,
}

impl EmailPatch {
    /// Creates an empty patch for one email ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
        }
    }

    /// Adds a keyword such as `$seen`.
    pub fn add_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.insert(keyword.into(), Some(true));
        self
    }

    /// Removes a keyword.
    pub fn remove_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.insert(keyword.into(), None);
        self
    }

    /// Marks the message as read.
    pub fn mark_seen(self) -> Self {
        self.add_keyword(KEYWORD_SEEN)
    }

    /// Marks the message as unread.
    pub fn mark_unseen(self) -> Self {
        self.remove_keyword(KEYWORD_SEEN)
    }

    /// Flags the message.
    pub fn mark_flagged(self) -> Self {
        self.add_keyword(KEYWORD_FLAGGED)
    }

    /// Clears the flag on the message.
    pub fn mark_unflagged(self) -> Self {
        self.remove_keyword(KEYWORD_FLAGGED)
    }

    /// Marks the message as answered.
    pub fn mark_answered(self) -> Self {
        self.add_keyword(KEYWORD_ANSWERED)
    }

    /// Marks the message as a draft.
    pub fn mark_draft(self) -> Self {
        self.add_keyword(KEYWORD_DRAFT)
    }

    /// Adds the message to a mailbox.
    pub fn add_to_mailbox(mut self, mailbox_id: impl Into<String>) -> Self {
        self.mailbox_ids.insert(mailbox_id.into(), Some(true));
        self
    }

    /// Removes the message from a mailbox.
    pub fn remove_from_mailbox(mut self, mailbox_id: impl Into<String>) -> Self {
        self.mailbox_ids.insert(mailbox_id.into(), None);
        self
    }

    /// Moves the message from one mailbox to another in a single patch.
    pub fn move_to(
        self,
        from_mailbox_id: impl Into<String>,
        to_mailbox_id: impl Into<String>,
    ) -> Self {
        self.remove_from_mailbox(from_mailbox_id)
            .add_to_mailbox(to_mailbox_id)
    }

    /// Renders the update as a single RFC 8621 `PatchObject`.
    pub fn to_patch_object(&self) -> Value {
        let mut patch = serde_json::Map::new();
        let mut keywords: Vec<String> = self.keywords.keys().cloned().collect();
        keywords.sort();
        for keyword in keywords {
            patch.insert(
                format!("keywords/{keyword}"),
                json!(self.keywords[&keyword]),
            );
        }
        let mut mailboxes: Vec<String> = self.mailbox_ids.keys().cloned().collect();
        mailboxes.sort();
        for mailbox_id in mailboxes {
            patch.insert(
                format!("mailboxIds/{mailbox_id}"),
                json!(self.mailbox_ids[&mailbox_id]),
            );
        }
        Value::Object(patch)
    }
}

/// Builds an `Email/set` call that applies patches to existing messages.
pub fn email_set_update_call(
    account_id: &str,
    patches: &[EmailPatch],
    call_id: &str,
) -> MethodCall {
    let mut update = serde_json::Map::new();
    for patch in patches {
        update.insert(patch.id.clone(), patch.to_patch_object());
    }
    MethodCall(
        "Email/set".to_string(),
        json!({
            "accountId": account_id,
            "update": Value::Object(update),
        }),
        call_id.to_string(),
    )
}

/// A message creation for `Email/set`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailCreate {
    /// The client-chosen creation ID, referenced later in the same request.
    pub creation_id: String,

    /// The mailboxes the new message belongs to.
    pub mailbox_ids: HashMap<String, bool>,

    /// The keywords to set on the new message.
    pub keywords: HashMap<String, bool>,

    /// The subject line.
    pub subject: String,

    /// The "From" address. Required by the JMAP mail specification.
    pub from: Vec<EmailAddress>,

    /// The "To" recipients.
    pub to: Vec<EmailAddress>,

    /// The "Cc" recipients.
    pub cc: Vec<EmailAddress>,

    /// The plain-text body.
    pub text_body: String,

    /// The RFC 5322 `Message-ID` to advertise, when the server allows it.
    pub message_id: Option<String>,

    /// The RFC 5322 `In-Reply-To` value, used when threading a reply.
    pub in_reply_to: Option<Vec<String>>,

    /// The RFC 5322 `References` values, used when threading a reply.
    pub references: Option<Vec<String>>,

    /// The date the message was sent, in RFC 3339 form.
    pub sent_at: Option<String>,
}

impl EmailCreate {
    /// Creates a plain-text message with a client-chosen creation ID.
    pub fn new_text(
        creation_id: impl Into<String>,
        from: EmailAddress,
        to: Vec<EmailAddress>,
        subject: impl Into<String>,
        text_body: impl Into<String>,
    ) -> Self {
        Self {
            creation_id: creation_id.into(),
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            subject: subject.into(),
            from: vec![from],
            to,
            cc: Vec::new(),
            text_body: text_body.into(),
            message_id: None,
            in_reply_to: None,
            references: None,
            sent_at: None,
        }
    }

    /// Sets the mailbox the draft lives in.
    pub fn in_mailbox(mut self, mailbox_id: impl Into<String>) -> Self {
        self.mailbox_ids.insert(mailbox_id.into(), true);
        self
    }

    /// Adds a keyword.
    pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.insert(keyword.into(), true);
        self
    }

    /// Adds a Cc recipient.
    pub fn with_cc(mut self, address: EmailAddress) -> Self {
        self.cc.push(address);
        self
    }

    /// Sets the `In-Reply-To` header values.
    pub fn in_reply_to(mut self, ids: Vec<String>) -> Self {
        self.in_reply_to = Some(ids);
        self
    }

    /// Sets the `References` header values.
    pub fn references(mut self, ids: Vec<String>) -> Self {
        self.references = Some(ids);
        self
    }

    /// Sets the RFC 5322 `Message-ID`.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets the sent-at timestamp.
    pub fn sent_at(mut self, sent_at: impl Into<String>) -> Self {
        self.sent_at = Some(sent_at.into());
        self
    }

    /// Renders the create as an RFC 8621 `Email` object.
    pub fn to_json(&self) -> Value {
        let body_part_id = "text";
        let mut create = serde_json::Map::new();
        create.insert("mailboxIds".to_string(), json!(self.mailbox_ids));
        create.insert("keywords".to_string(), json!(self.keywords));
        create.insert("subject".to_string(), json!(self.subject));
        create.insert("from".to_string(), json!(self.from));
        if !self.to.is_empty() {
            create.insert("to".to_string(), json!(self.to));
        }
        if !self.cc.is_empty() {
            create.insert("cc".to_string(), json!(self.cc));
        }
        if let Some(message_id) = &self.message_id {
            create.insert("messageId".to_string(), json!([message_id]));
        }
        if let Some(in_reply_to) = &self.in_reply_to {
            create.insert("inReplyTo".to_string(), json!(in_reply_to));
        }
        if let Some(references) = &self.references {
            create.insert("references".to_string(), json!(references));
        }
        if let Some(sent_at) = &self.sent_at {
            create.insert("sentAt".to_string(), json!(sent_at));
        }
        create.insert(
            "bodyValues".to_string(),
            json!({ body_part_id: { "value": self.text_body } }),
        );
        create.insert(
            "textBody".to_string(),
            json!([{ "partId": body_part_id, "type": "text/plain" }]),
        );
        Value::Object(create)
    }
}

/// Builds an `Email/set` call that creates drafts.
pub fn email_set_create_call(
    account_id: &str,
    creates: &[EmailCreate],
    call_id: &str,
) -> MethodCall {
    let mut create = serde_json::Map::new();
    for item in creates {
        create.insert(item.creation_id.clone(), item.to_json());
    }
    MethodCall(
        "Email/set".to_string(),
        json!({
            "accountId": account_id,
            "create": Value::Object(create),
        }),
        call_id.to_string(),
    )
}

/// A submission envelope for `EmailSubmission/set`, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailSubmission {
    /// The client-chosen creation ID.
    pub creation_id: String,

    /// The ID of the draft email to send.
    pub email_id: String,

    /// The identity to send as. Resolved from `Identity/get` when omitted.
    pub identity_id: String,

    /// The envelope, when overriding the addresses derived from the headers.
    pub envelope: Option<Envelope>,
}

/// The SMTP envelope for a submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The envelope sender address.
    pub mail_from: EmailAddress,

    /// The envelope recipient addresses.
    pub rcpt_to: Vec<EmailAddress>,
}

impl EmailSubmission {
    /// Creates a submission for a draft email and an identity.
    pub fn new(
        creation_id: impl Into<String>,
        email_id: impl Into<String>,
        identity_id: impl Into<String>,
    ) -> Self {
        Self {
            creation_id: creation_id.into(),
            email_id: email_id.into(),
            identity_id: identity_id.into(),
            envelope: None,
        }
    }

    /// Overrides the SMTP envelope.
    pub fn with_envelope(mut self, envelope: Envelope) -> Self {
        self.envelope = Some(envelope);
        self
    }

    /// Renders the submission as an RFC 8621 `EmailSubmission` object.
    pub fn to_json(&self) -> Value {
        let mut json_object = serde_json::Map::new();
        json_object.insert("identityId".to_string(), json!(self.identity_id));
        json_object.insert("emailId".to_string(), json!(self.email_id));
        if let Some(envelope) = &self.envelope {
            json_object.insert(
                "envelope".to_string(),
                json!({
                    "mailFrom": envelope.mail_from,
                    "rcptTo": envelope.rcpt_to,
                }),
            );
        }
        Value::Object(json_object)
    }
}

/// Builds an `EmailSubmission/set` call that sends the given submissions.
pub fn email_submission_set_call(
    account_id: &str,
    submissions: &[EmailSubmission],
    call_id: &str,
) -> MethodCall {
    let mut create = serde_json::Map::new();
    for submission in submissions {
        create.insert(submission.creation_id.clone(), submission.to_json());
    }
    MethodCall(
        "EmailSubmission/set".to_string(),
        json!({
            "accountId": account_id,
            "create": Value::Object(create),
        }),
        call_id.to_string(),
    )
}

impl EmailSubmission {
    /// Builds an `EmailSubmission/set` call whose `emailId` may refer to an
    /// email created earlier in the same request through a `#creationId`.
    ///
    /// The reference form lets a create and a submit travel in one round trip,
    /// which is how [`crate::JMAPClient::send_reply`] sends a threaded reply.
    pub fn set_call_with_created_ids(
        account_id: &str,
        submissions: &[EmailSubmission],
        call_id: &str,
    ) -> MethodCall {
        email_submission_set_call(account_id, submissions, call_id)
    }
}

/// Parses a `standard/*` result object into a typed value.
pub fn parse_result<T: for<'de> Deserialize<'de>>(
    response: &JMAPResponse,
    call_id: &str,
) -> Result<T> {
    let value = response.result(call_id)?;
    serde_json::from_value(value.clone())
        .with_context(|| format!("Failed to parse result of method call '{call_id}'"))
}

/// Convenience re-export of the constructing helper for `MethodCall`.
pub fn method_call(name: &str, args: Value, call_id: &str) -> MethodCall {
    MethodCall(name.to_string(), args, call_id.to_string())
}
