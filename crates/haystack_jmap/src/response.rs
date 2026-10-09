//! Parsing of JMAP method results into typed values.
//!
//! Each parser takes the `arguments` object of one method response and returns
//! either the typed result or the server-reported `notFound` / error entries
//! alongside it.
use crate::request::JMAPError;
use crate::types::{Email, EmailChanges, Identity, Mailbox, MailboxList};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The `Email/query` result.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EmailQueryResult {
    /// The account the query ran against.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The IDs of the matching messages, in query order.
    #[serde(default)]
    pub ids: Vec<String>,

    /// The total number of messages matching the filter, when the server
    /// reports it.
    #[serde(default)]
    pub total: Option<i64>,

    /// The `queryState` token for the query.
    #[serde(rename = "queryState", default)]
    pub query_state: String,

    /// The position of the first returned ID within the full result.
    #[serde(default)]
    pub position: u32,

    /// Whether the server capped the query at `limit`.
    #[serde(rename = "canCalculateChanges", default)]
    pub can_calculate_changes: Option<bool>,
}

impl EmailQueryResult {
    /// Parses an `Email/query` result object.
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|e| anyhow!("Invalid Email/query result: {e}"))
    }
}

/// The `Email/get` result.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EmailGetResult {
    /// The account the messages belong to.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The state string for the messages returned.
    #[serde(default)]
    pub state: String,

    /// The messages.
    #[serde(default)]
    pub list: Vec<Email>,

    /// IDs the server could not find, mapped to the `notFound` marker.
    #[serde(rename = "notFound", default, skip_serializing_if = "Vec::is_empty")]
    pub not_found: Vec<String>,
}

impl EmailGetResult {
    /// Parses an `Email/get` result object.
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| anyhow!("Invalid Email/get result: {e}"))
    }
}

/// The `Mailbox/get` result.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MailboxGetResult {
    /// The account the mailboxes belong to.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The state string of the mailbox list.
    #[serde(default)]
    pub state: String,

    /// The mailboxes.
    #[serde(default)]
    pub list: Vec<Mailbox>,

    /// IDs the server could not find.
    #[serde(rename = "notFound", default, skip_serializing_if = "Vec::is_empty")]
    pub not_found: Vec<String>,
}

impl MailboxGetResult {
    /// Parses a `Mailbox/get` result object.
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|e| anyhow!("Invalid Mailbox/get result: {e}"))
    }

    /// Narrows the result into a lookup structure without cloning the list.
    pub fn into_list(self) -> MailboxList {
        MailboxList {
            account_id: self.account_id,
            state: self.state,
            list: self.list,
        }
    }
}

/// The `Identity/get` result.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct IdentityGetResult {
    /// The account the identities belong to.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The state string of the identity list.
    #[serde(default)]
    pub state: String,

    /// The identities.
    #[serde(default)]
    pub list: Vec<Identity>,

    /// IDs the server could not find.
    #[serde(rename = "notFound", default, skip_serializing_if = "Vec::is_empty")]
    pub not_found: Vec<String>,
}

impl IdentityGetResult {
    /// Parses an `Identity/get` result object.
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .map_err(|e| anyhow!("Invalid Identity/get result: {e}"))
    }

    /// Finds the identity whose address matches, ignoring case.
    pub fn by_email(&self, email: &str) -> Option<&Identity> {
        self.list
            .iter()
            .find(|i| i.email_or_empty().eq_ignore_ascii_case(email))
    }

    /// Returns the first identity, matching the server's default ordering.
    pub fn default_identity(&self) -> Option<&Identity> {
        self.list.first()
    }
}

/// The `Email/changes` result.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EmailChangesResult {
    /// The account the changes belong to.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The state the changes were computed from.
    #[serde(rename = "oldState", default)]
    pub old_state: String,

    /// The state to use as `sinceState` next time.
    #[serde(rename = "newState", default)]
    pub new_state: String,

    /// The IDs of messages created since `oldState`.
    #[serde(default)]
    pub created: Vec<String>,

    /// The IDs of messages updated since `oldState`.
    #[serde(default)]
    pub updated: Vec<String>,

    /// The IDs of messages destroyed since `oldState`.
    #[serde(default)]
    pub destroyed: Vec<String>,

    /// Whether the server truncated the change list at `maxChanges`.
    #[serde(rename = "hasMoreChanges", default)]
    pub has_more_changes: bool,

    /// The total number of messages, when the server reports it.
    #[serde(default)]
    pub total: Option<u64>,
}

impl EmailChangesResult {
    /// Parses an `Email/changes` result object. A `cannotCalculateChanges`
    /// error becomes a result marked for a full re-query rather than a failure.
    pub fn from_value(value: &Value) -> Result<Self> {
        if let Ok(error) = serde_json::from_value::<JMAPError>(value.clone())
            && error.is_cannot_calculate_changes()
        {
            return Ok(Self {
                account_id: value
                    .get("accountId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                old_state: String::new(),
                new_state: String::new(),
                created: Vec::new(),
                updated: Vec::new(),
                destroyed: Vec::new(),
                has_more_changes: false,
                total: None,
            });
        }
        serde_json::from_value(value.clone())
            .map_err(|e| anyhow!("Invalid Email/changes result: {e}"))
    }

    /// Converts the result into the caller-facing [`EmailChanges`] plus the
    /// cursor to persist on success.
    pub fn into_changes(self, previous_state: &str) -> (EmailChanges, Option<String>) {
        let changes = EmailChanges {
            account_id: self.account_id.clone(),
            new_state: self.new_state.clone(),
            old_state: self.old_state.clone(),
            cannot_calculate_changes: self.new_state.is_empty(),
            created: self.created,
            updated: self.updated,
            destroyed: self.destroyed,
            total: self.total,
        };
        if changes.cannot_calculate_changes {
            (changes, None)
        } else {
            let _ = previous_state;
            (changes, Some(self.new_state))
        }
    }
}

/// One entry in a `*/set` `created` map.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SetCreated {
    /// The server-assigned ID.
    pub id: String,

    /// The blob ID of a created email, when the server reports one.
    #[serde(rename = "blobId", default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<String>,

    /// The thread ID of a created email, when the server reports one.
    #[serde(rename = "threadId", default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,

    /// The size of a created email, when the server reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// A `SetError` entry, as described by RFC 8620 section 5.3.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SetError {
    /// The RFC 8620 error type, for example `notFound` or `invalidProperties`.
    #[serde(rename = "type")]
    pub type_: String,

    /// A human-readable description, when the server supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The properties that caused the error, when the server names them.
    #[serde(
        rename = "properties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub properties: Option<Vec<String>>,
}

/// The result of an `Email/set` or `EmailSubmission/set` call.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SetResult {
    /// The account the changes were applied to.
    #[serde(rename = "accountId", default)]
    pub account_id: String,

    /// The state before the changes were applied.
    #[serde(rename = "oldState", default)]
    pub old_state: String,

    /// The state after the changes were applied.
    #[serde(rename = "newState", default)]
    pub new_state: String,

    /// The records created, keyed by creation ID.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub created: HashMap<String, SetCreated>,

    /// The IDs of the records updated, mapped to `null` as RFC 8620
    /// section 5.3 specifies.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub updated: HashMap<String, Value>,

    /// The IDs of the records destroyed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destroyed: Vec<String>,

    /// Records that could not be created, keyed by creation ID.
    #[serde(
        rename = "notCreated",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub not_created: Option<HashMap<String, SetError>>,

    /// Records that could not be updated, keyed by record ID.
    #[serde(
        rename = "notUpdated",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub not_updated: Option<HashMap<String, SetError>>,

    /// Records that could not be destroyed, keyed by record ID.
    #[serde(
        rename = "notDestroyed",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub not_destroyed: Option<HashMap<String, SetError>>,
}

impl SetResult {
    /// Parses an `Email/set` or `EmailSubmission/set` result object.
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| anyhow!("Invalid */set result: {e}"))
    }

    /// Returns the created record for a creation ID.
    pub fn created_record(&self, creation_id: &str) -> Option<&SetCreated> {
        self.created.get(creation_id)
    }

    /// Returns the IDs of the records this call updated, in sorted order.
    pub fn updated_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.updated.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Returns the error for a creation ID, if the server rejected it.
    pub fn creation_error(&self, creation_id: &str) -> Option<&SetError> {
        self.not_created.as_ref()?.get(creation_id)
    }

    /// Returns the error for an update to a record, if the server rejected it.
    pub fn update_error(&self, id: &str) -> Option<&SetError> {
        self.not_updated.as_ref()?.get(id)
    }

    /// Returns an error when the server rejected anything in this call.
    ///
    /// This lets a caller treat a partially applied change as a failure without
    /// inspecting the individual maps first.
    pub fn into_result(self) -> Result<Self> {
        if let Some(not_created) = &self.not_created
            && let Some((creation_id, error)) = not_created.iter().next()
        {
            return Err(anyhow!(
                "JMAP create '{creation_id}' failed: {}",
                error.type_
            ));
        }
        if let Some(not_updated) = &self.not_updated
            && let Some((id, error)) = not_updated.iter().next()
        {
            return Err(anyhow!("JMAP update '{id}' failed: {}", error.type_));
        }
        if let Some(not_destroyed) = &self.not_destroyed
            && let Some((id, error)) = not_destroyed.iter().next()
        {
            return Err(anyhow!("JMAP destroy '{id}' failed: {}", error.type_));
        }
        Ok(self)
    }
}

/// Parses the `createdIds` map that a `*/set` response may carry.
pub fn parse_created_ids(value: &Value) -> HashMap<String, String> {
    value
        .get("createdIds")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Renders a request-shaped `Email/changes` error so tests and callers can
/// recognise the full re-query signal.
pub fn cannot_calculate_changes_error() -> Value {
    json!({
        "type": "cannotCalculateChanges",
        "description": "The server cannot calculate changes from the supplied state",
    })
}
