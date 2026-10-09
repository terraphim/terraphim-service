//! The JMAP client: session discovery plus the mail, mailbox and submission
//! calls used by the tinyclaw email channel.
use crate::cursor::StateCursorStore;
use crate::request::{
    ChangesOptions, EmailCreate, EmailGetOptions, EmailPatch, EmailQueryFilter, EmailSubmission,
    JMAPRequest, JMAPResponse, MethodCall, core_mail_request, email_changes_call, email_get_call,
    email_get_referencing_query_call, email_query_call, email_set_create_call,
    email_set_update_call, email_submission_set_call, identity_get_call, mailbox_get_call,
    submission_request,
};
use crate::response::{
    EmailChangesResult, EmailGetResult, EmailQueryResult, IdentityGetResult, MailboxGetResult,
    SetResult,
};
use crate::types::{CAPABILITY_MAIL, CAPABILITY_SUBMISSION, Email, EmailChanges, MailboxList};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Represents a JMAP session as returned by the session resource.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    /// The primary accounts associated with the session.
    #[serde(rename = "primaryAccounts", default)]
    pub primary_accounts: HashMap<String, String>,

    /// The URL of the JMAP API.
    #[serde(rename = "apiUrl")]
    pub api_url: String,

    /// The capabilities of the JMAP server.
    #[serde(default)]
    pub capabilities: HashMap<String, serde_json::Value>,

    /// The URL for downloading attachments.
    #[serde(rename = "downloadUrl", default)]
    pub download_url: String,

    /// The URL for uploading attachments.
    #[serde(rename = "uploadUrl", default)]
    pub upload_url: String,

    /// The current state of the session.
    #[serde(default)]
    pub state: String,

    /// The username associated with the session.
    #[serde(default)]
    pub username: String,
}

impl Session {
    /// Returns the mail account ID for the `urn:ietf:params:jmap:mail`
    /// capability, or an error naming the missing capability.
    pub fn mail_account_id(&self) -> Result<&str> {
        self.primary_accounts
            .get(CAPABILITY_MAIL)
            .map(String::as_str)
            .context("No mail account found in primaryAccounts")
    }

    /// Returns the submission account ID, falling back to the mail account.
    ///
    /// Fastmail maps both capabilities to the same account; servers that report
    /// a distinct submission account take precedence.
    pub fn submission_account_id(&self) -> Result<&str> {
        self.primary_accounts
            .get(CAPABILITY_SUBMISSION)
            .or_else(|| self.primary_accounts.get(CAPABILITY_MAIL))
            .map(String::as_str)
            .context("No submission or mail account found in primaryAccounts")
    }

    /// Returns true when the server advertises the submission capability.
    pub fn supports_submission(&self) -> bool {
        self.capabilities.contains_key(CAPABILITY_SUBMISSION)
            || self.primary_accounts.contains_key(CAPABILITY_SUBMISSION)
    }
}

/// A client for interacting with a JMAP server.
///
/// The existing [`JMAPClient::new`] and [`JMAPClient::search_emails`] entry
/// points keep working; the additional methods cover cursors, mailbox moves,
/// keyword changes and submission.
#[derive(Debug, Clone)]
pub struct JMAPClient {
    /// The JMAP session associated with the client.
    session: Session,

    /// The HTTP client used for making requests.
    client: reqwest::Client,

    /// The access token used for authentication.
    access_token: String,

    /// The durable store for the `Email/changes` state cursor.
    cursor_store: Arc<dyn StateCursorStore>,
}

impl JMAPClient {
    /// Creates a new `JMAPClient` with the given access token and session URL.
    ///
    /// The client starts with an in-memory cursor store; call
    /// [`JMAPClient::with_cursor_store`] to persist the state string.
    pub async fn new(access_token: String, session_url: &str) -> Result<Self> {
        let client = reqwest::Client::new();

        log::info!("Connecting to JMAP session: {}", session_url);

        let session_response = client
            .get(session_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await
            .context("Failed to connect to JMAP server")?;

        let status = session_response.status();
        log::debug!("JMAP session status: {}", status);

        let response_text = session_response.text().await?;

        if !status.is_success() {
            return Err(anyhow!(
                "Failed to authenticate: {} - {}",
                status,
                response_text
            ));
        }

        log::debug!("JMAP session body length: {} bytes", response_text.len());

        let session: Session =
            serde_json::from_str(&response_text).context("Failed to parse session response")?;

        log::info!("JMAP API URL: {}", session.api_url);

        Ok(Self {
            session,
            client,
            access_token,
            cursor_store: Arc::new(crate::cursor::MemoryCursorStore::new()),
        })
    }

    /// Replaces the cursor store, consuming and returning the client.
    pub fn with_cursor_store(mut self, cursor_store: Arc<dyn StateCursorStore>) -> Self {
        self.cursor_store = cursor_store;
        self
    }

    /// Returns the session backing this client.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Returns the cursor store backing this client.
    pub fn cursor_store(&self) -> &Arc<dyn StateCursorStore> {
        &self.cursor_store
    }

    /// Returns the mail account ID.
    pub fn account_id(&self) -> Result<&str> {
        self.session.mail_account_id()
    }

    /// Sends one request envelope and parses the response.
    async fn send(&self, request: &JMAPRequest) -> Result<JMAPResponse> {
        log::debug!("JMAP request: {}", serde_json::to_string(request)?);

        let response = self
            .client
            .post(&self.session.api_url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .context("Failed to send JMAP request")?;

        let status = response.status();
        log::debug!("JMAP response status: {}", status);

        let response_text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!(
                "JMAP request failed: {} - {}",
                status,
                response_text
            ));
        }

        serde_json::from_str(&response_text).context("Failed to parse JMAP response")
    }

    /// Sends a single method call and returns its arguments object.
    async fn call(&self, request: JMAPRequest, call_id: &str) -> Result<serde_json::Value> {
        let response = self.send(&request).await?;
        response.result(call_id).cloned()
    }

    /// Searches for emails matching the given query with configurable result limit.
    ///
    /// This is the original entry point; it keeps its signature and behaviour.
    pub async fn search_emails(&self, query: &str, limit: u32) -> Result<Vec<Email>> {
        let account_id = self.account_id()?.to_string();
        let filter = EmailQueryFilter::Text(query.to_string());
        let query_call = email_query_call(&account_id, Some(&filter), Some(limit), None, "s1");
        let query_args = self.call(core_mail_request(vec![query_call]), "s1").await?;
        let query_result = EmailQueryResult::from_value(&query_args)?;
        self.get_emails(&query_result.ids).await
    }

    /// Runs an `Email/query` scoped to a mailbox and returns the matching IDs.
    ///
    /// The `filter` is optional; `limit` and `position` support paging.
    pub async fn query_email_ids(
        &self,
        filter: Option<&EmailQueryFilter>,
        limit: Option<u32>,
        position: Option<u32>,
    ) -> Result<EmailQueryResult> {
        let account_id = self.account_id()?.to_string();
        let call = email_query_call(&account_id, filter, limit, position, "q1");
        let args = self.call(core_mail_request(vec![call]), "q1").await?;
        EmailQueryResult::from_value(&args)
    }

    /// Runs `Email/query` scoped to one mailbox and fetches the matching
    /// messages in a single request using an `#ids` back-reference.
    ///
    /// This is the read path the design names for inbound mail: one round trip
    /// per poll, filtered by mailbox.
    pub async fn query_emails_in_mailbox(
        &self,
        mailbox_id: &str,
        filter: Option<&EmailQueryFilter>,
        limit: Option<u32>,
        position: Option<u32>,
        options: &EmailGetOptions,
    ) -> Result<EmailGetResult> {
        let account_id = self.account_id()?.to_string();
        let combined = match filter {
            Some(extra) => EmailQueryFilter::and(vec![
                EmailQueryFilter::in_mailbox(mailbox_id.to_string()),
                extra.clone(),
            ]),
            None => EmailQueryFilter::in_mailbox(mailbox_id.to_string()),
        };
        let query_call = email_query_call(&account_id, Some(&combined), limit, position, "q1");
        let get_call = email_get_referencing_query_call(&account_id, "q1", options, "g1");
        let request = core_mail_request(vec![query_call, get_call]);
        let response = self.send(&request).await?;
        let args = response.result("g1")?;
        EmailGetResult::from_value(args)
    }

    /// Fetches the full email data for the given email IDs.
    pub async fn get_emails(&self, email_ids: &[String]) -> Result<Vec<Email>> {
        Ok(self
            .get_emails_with_options(email_ids, &EmailGetOptions::full())
            .await?
            .list)
    }

    /// Fetches emails with explicit property selection.
    pub async fn get_emails_with_options(
        &self,
        email_ids: &[String],
        options: &EmailGetOptions,
    ) -> Result<EmailGetResult> {
        if email_ids.is_empty() {
            return Ok(EmailGetResult {
                account_id: self.account_id()?.to_string(),
                state: String::new(),
                list: Vec::new(),
                not_found: Vec::new(),
            });
        }

        let account_id = self.account_id()?.to_string();
        let call = email_get_call(&account_id, email_ids, options, "g1");
        let args = self.call(core_mail_request(vec![call]), "g1").await?;
        EmailGetResult::from_value(&args)
    }

    /// Fetches every mailbox on the account.
    pub async fn get_mailboxes(&self) -> Result<MailboxList> {
        let account_id = self.account_id()?.to_string();
        let call = mailbox_get_call(&account_id, "m1");
        let args = self.call(core_mail_request(vec![call]), "m1").await?;
        Ok(MailboxGetResult::from_value(&args)?.into_list())
    }

    /// Resolves a mailbox by role (`inbox`, `archive`, `junk`, ...).
    pub async fn mailbox_by_role(&self, role: &str) -> Result<Option<crate::types::Mailbox>> {
        Ok(self.get_mailboxes().await?.by_role(role).cloned())
    }

    /// Resolves a mailbox by user-visible name, such as `Processed` or
    /// `Quarantine`. The name comparison ignores case.
    pub async fn mailbox_by_name(&self, name: &str) -> Result<Option<crate::types::Mailbox>> {
        Ok(self.get_mailboxes().await?.by_name(name).cloned())
    }

    /// Resolves a mailbox by role first and by name second, returning an error
    /// that names both lookups when neither matches.
    pub async fn resolve_mailbox(&self, name_or_role: &str) -> Result<crate::types::Mailbox> {
        let mailboxes = self.get_mailboxes().await?;
        if let Some(mailbox) = mailboxes
            .by_role(name_or_role)
            .or_else(|| mailboxes.by_name(name_or_role))
        {
            return Ok(mailbox.clone());
        }
        let available: Vec<String> = mailboxes
            .list
            .iter()
            .map(|m| match &m.role {
                Some(role) => format!("{} (role {role})", m.name),
                None => m.name.clone(),
            })
            .collect();
        Err(anyhow!(
            "No mailbox found with name or role '{name_or_role}'. Available: {}",
            available.join(", ")
        ))
    }

    /// Fetches the identities configured on the account.
    pub async fn get_identities(&self) -> Result<IdentityGetResult> {
        let account_id = self.account_id()?.to_string();
        let call = identity_get_call(&account_id, "i1");
        let args = self.call(core_mail_request(vec![call]), "i1").await?;
        IdentityGetResult::from_value(&args)
    }

    /// Picks an identity ID for sending.
    ///
    /// When `from_address` is given, the identity with that address is used.
    /// Otherwise the first identity the server returns (the default) is used.
    pub async fn resolve_identity_id(&self, from_address: Option<&str>) -> Result<String> {
        let identities = self.get_identities().await?;
        let identity = match from_address {
            Some(address) => identities.by_email(address).ok_or_else(|| {
                let available: Vec<&str> =
                    identities.list.iter().map(|i| i.email_or_empty()).collect();
                anyhow!(
                    "No identity for '{address}'. Available: {}",
                    available.join(", ")
                )
            })?,
            None => identities
                .default_identity()
                .context("The account reports no sending identities")?,
        };
        Ok(identity.id.clone())
    }

    /// Applies `Email/set` patches, for example marking `$seen` or moving a
    /// message between mailboxes.
    pub async fn update_emails(&self, patches: &[EmailPatch]) -> Result<SetResult> {
        let account_id = self.account_id()?.to_string();
        let call = email_set_update_call(&account_id, patches, "u1");
        let args = self.call(core_mail_request(vec![call]), "u1").await?;
        SetResult::from_value(&args)
    }

    /// Marks one or more messages as read.
    pub async fn mark_seen(&self, email_ids: &[String]) -> Result<SetResult> {
        let patches: Vec<EmailPatch> = email_ids
            .iter()
            .map(|id| EmailPatch::new(id.clone()).mark_seen())
            .collect();
        self.update_emails(&patches).await
    }

    /// Marks one or more messages as unread.
    pub async fn mark_unseen(&self, email_ids: &[String]) -> Result<SetResult> {
        let patches: Vec<EmailPatch> = email_ids
            .iter()
            .map(|id| EmailPatch::new(id.clone()).mark_unseen())
            .collect();
        self.update_emails(&patches).await
    }

    /// Moves messages from one mailbox to another.
    ///
    /// The patch adds and removes membership in one call, so no message is left
    /// outside every mailbox.
    pub async fn move_emails(
        &self,
        email_ids: &[String],
        from_mailbox_id: &str,
        to_mailbox_id: &str,
    ) -> Result<SetResult> {
        let patches: Vec<EmailPatch> = email_ids
            .iter()
            .map(|id| {
                EmailPatch::new(id.clone())
                    .move_to(from_mailbox_id.to_string(), to_mailbox_id.to_string())
            })
            .collect();
        self.update_emails(&patches).await
    }

    /// Creates drafts through `Email/set` and returns the result.
    pub async fn create_drafts(&self, creates: &[EmailCreate]) -> Result<SetResult> {
        let account_id = self.account_id()?.to_string();
        let call = email_set_create_call(&account_id, creates, "c1");
        let args = self.call(core_mail_request(vec![call]), "c1").await?;
        SetResult::from_value(&args)
    }

    /// Sends submissions through `EmailSubmission/set`.
    pub async fn submit_emails(&self, submissions: &[EmailSubmission]) -> Result<SetResult> {
        let account_id = self.session.submission_account_id()?.to_string();
        let call = email_submission_set_call(&account_id, submissions, "s1");
        let args = self.call(submission_request(vec![call]), "s1").await?;
        SetResult::from_value(&args)
    }

    /// Sends a threaded reply in one request: create the draft, submit it and
    /// mark the original as seen and answered.
    ///
    /// `reply` carries the headers that thread the message. The identity is
    /// resolved with [`JMAPClient::resolve_identity_id`].
    pub async fn send_reply(&self, reply: &ReplyRequest) -> Result<ReplyOutcome> {
        let account_id = self.session.submission_account_id()?.to_string();
        let identity_id = self.resolve_identity_id(Some(&reply.from.email)).await?;

        let mut create = EmailCreate::new_text(
            "reply",
            reply.from.clone(),
            reply.to.clone(),
            reply.subject.clone(),
            reply.text_body.clone(),
        )
        .in_mailbox(reply.drafts_mailbox_id.clone())
        .with_keyword(crate::types::KEYWORD_DRAFT);
        if !reply.cc.is_empty() {
            for address in &reply.cc {
                create = create.with_cc(address.clone());
            }
        }
        if let Some(message_id) = &reply.message_id {
            create = create.with_message_id(message_id.clone());
        }
        if let Some(in_reply_to) = &reply.in_reply_to {
            create = create.in_reply_to(in_reply_to.clone());
        }
        if let Some(references) = &reply.references {
            create = create.references(references.clone());
        }
        if let Some(sent_at) = &reply.sent_at {
            create = create.sent_at(sent_at.clone());
        }

        let submission = EmailSubmission::new("submission", "#reply", identity_id);

        let mut calls = vec![
            email_set_create_call(&account_id, std::slice::from_ref(&create), "create"),
            EmailSubmission::set_call_with_created_ids(&account_id, &[submission], "submit"),
        ];
        if let Some(original_id) = &reply.in_reply_to_email_id {
            calls.push(email_set_update_call(
                &account_id,
                &[EmailPatch::new(original_id.clone())
                    .mark_seen()
                    .mark_answered()],
                "flag",
            ));
        }
        let request = submission_request(calls);
        let response = self.send(&request).await?;

        let create_result = SetResult::from_value(response.result("create")?)?;
        let submit_result = SetResult::from_value(response.result("submit")?)?;
        let sent_email_id = create_result
            .created_record("reply")
            .map(|record| record.id.clone());
        let submission_id = submit_result
            .created_record("submission")
            .map(|record| record.id.clone());

        Ok(ReplyOutcome {
            sent_email_id,
            submission_id,
            created: create_result,
            submitted: submit_result,
        })
    }

    /// Runs `Email/changes` from the persisted cursor and returns the changes
    /// plus the cursor to store.
    ///
    /// When the account holds no cursor yet the caller gets
    /// `cannot_calculate_changes` and must run a full query. When the server
    /// reports `cannotCalculateChanges` the same signal is returned and the
    /// stale cursor is cleared.
    pub async fn email_changes(&self, options: &ChangesOptions) -> Result<EmailChanges> {
        let account_id = self.account_id()?.to_string();
        let since_state = match self.cursor_store.load(&account_id)? {
            Some(state) => state,
            None => {
                return Ok(EmailChanges {
                    account_id,
                    new_state: String::new(),
                    old_state: String::new(),
                    cannot_calculate_changes: true,
                    created: Vec::new(),
                    updated: Vec::new(),
                    destroyed: Vec::new(),
                    total: None,
                });
            }
        };

        let call = email_changes_call(&account_id, &since_state, options, "ch1");
        let response = self.send(&core_mail_request(vec![call])).await?;

        // A cannotCalculateChanges error means the cursor is too old to use; the
        // caller must re-query the mailbox from scratch.
        if response
            .error()
            .is_some_and(|error| error.is_cannot_calculate_changes())
        {
            self.cursor_store.clear(&account_id)?;
            return Ok(EmailChanges {
                account_id,
                new_state: String::new(),
                old_state: since_state,
                cannot_calculate_changes: true,
                created: Vec::new(),
                updated: Vec::new(),
                destroyed: Vec::new(),
                total: None,
            });
        }

        let args = match response.result("ch1") {
            Ok(args) => args.clone(),
            Err(error) => {
                // Some servers report the error against the call id rather than
                // as a leading error response; check the text as a fallback.
                if error.to_string().contains("cannotCalculateChanges") {
                    self.cursor_store.clear(&account_id)?;
                    return Ok(EmailChanges {
                        account_id,
                        new_state: String::new(),
                        old_state: since_state,
                        cannot_calculate_changes: true,
                        created: Vec::new(),
                        updated: Vec::new(),
                        destroyed: Vec::new(),
                        total: None,
                    });
                }
                return Err(error);
            }
        };

        let parsed = EmailChangesResult::from_value(&args)?;
        let (changes, next_state) = parsed.into_changes(&since_state);
        if let Some(state) = next_state {
            self.cursor_store.store(&account_id, &state)?;
        } else {
            self.cursor_store.clear(&account_id)?;
        }
        Ok(changes)
    }

    /// Stores a cursor state directly, for example after a full re-query that
    /// returned a fresh state string from `Email/get`.
    pub fn store_cursor(&self, state: &str) -> Result<()> {
        self.cursor_store.store(self.account_id()?, state)
    }

    /// Loads the persisted cursor, if any.
    pub fn load_cursor(&self) -> Result<Option<String>> {
        self.cursor_store.load(self.account_id()?)
    }

    /// Fetches the full data for the emails named by a change set, excluding
    /// destroyed messages.
    pub async fn fetch_changed_emails(&self, changes: &EmailChanges) -> Result<Vec<Email>> {
        let mut ids: Vec<String> = changes.created.clone();
        ids.extend(changes.updated.iter().cloned());
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.get_emails(&ids).await
    }

    /// Builds the request envelope an `Email/query` by mailbox produces.
    ///
    /// Exposed so callers can inspect or log the exact wire form without a live
    /// server.
    pub fn build_mailbox_query_request(
        &self,
        mailbox_id: &str,
        filter: Option<&EmailQueryFilter>,
        limit: Option<u32>,
    ) -> Result<JMAPRequest> {
        let account_id = self.account_id()?;
        let combined = match filter {
            Some(extra) => EmailQueryFilter::and(vec![
                EmailQueryFilter::in_mailbox(mailbox_id.to_string()),
                extra.clone(),
            ]),
            None => EmailQueryFilter::in_mailbox(mailbox_id.to_string()),
        };
        Ok(core_mail_request(vec![email_query_call(
            account_id,
            Some(&combined),
            limit,
            None,
            "q1",
        )]))
    }
}

/// A threaded reply to send through [`JMAPClient::send_reply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyRequest {
    /// The address to send from. Used to select the identity.
    pub from: crate::types::EmailAddress,

    /// The primary recipients of the reply.
    pub to: Vec<crate::types::EmailAddress>,

    /// Additional recipients, typically the original Cc list.
    pub cc: Vec<crate::types::EmailAddress>,

    /// The subject line, conventionally `Re: <original subject>`.
    pub subject: String,

    /// The plain-text body of the reply.
    pub text_body: String,

    /// The mailbox the draft is created in before submission.
    pub drafts_mailbox_id: String,

    /// The original message ID, flagged `$seen` and `$answered` after sending.
    pub in_reply_to_email_id: Option<String>,

    /// The `Message-ID` of the original message, used for `In-Reply-To`.
    pub message_id: Option<String>,

    /// The `In-Reply-To` header values.
    pub in_reply_to: Option<Vec<String>>,

    /// The `References` header values.
    pub references: Option<Vec<String>>,

    /// The sent-at timestamp, in RFC 3339 form.
    pub sent_at: Option<String>,
}

impl ReplyRequest {
    /// Builds a reply carrying the threading headers taken from an original
    /// message.
    ///
    /// `In-Reply-To` becomes the original `Message-ID` and `References` becomes
    /// the original references plus that `Message-ID`, which is the threading
    /// rule in RFC 5322 section 3.6.4.
    pub fn threaded(
        from: crate::types::EmailAddress,
        to: Vec<crate::types::EmailAddress>,
        subject: String,
        text_body: String,
        drafts_mailbox_id: String,
        original: &Email,
    ) -> Self {
        let message_id = original
            .message_id
            .as_ref()
            .and_then(|ids| ids.first())
            .cloned();
        let mut references = original.references.clone().unwrap_or_default();
        if let Some(id) = &message_id
            && !references.contains(id)
        {
            references.push(id.clone());
        }
        Self {
            from,
            to,
            cc: original.cc.clone().unwrap_or_default(),
            subject,
            text_body,
            drafts_mailbox_id,
            in_reply_to_email_id: Some(original.id.clone()),
            message_id: None,
            in_reply_to: message_id.map(|id| vec![id]),
            references: if references.is_empty() {
                None
            } else {
                Some(references)
            },
            sent_at: None,
        }
    }

    /// Sets the `Message-ID` this reply advertises.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets the sent-at timestamp.
    pub fn sent_at(mut self, sent_at: impl Into<String>) -> Self {
        self.sent_at = Some(sent_at.into());
        self
    }
}

/// The outcome of [`JMAPClient::send_reply`].
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyOutcome {
    /// The server-assigned ID of the sent message, when the server reported it.
    pub sent_email_id: Option<String>,

    /// The server-assigned ID of the submission.
    pub submission_id: Option<String>,

    /// The full `Email/set` result for the created draft.
    pub created: SetResult,

    /// The full `EmailSubmission/set` result.
    pub submitted: SetResult,
}

impl ReplyOutcome {
    /// Returns true when the server reported both the sent message and the
    /// submission.
    pub fn is_complete(&self) -> bool {
        self.sent_email_id.is_some() && self.submission_id.is_some()
    }
}

/// Re-export of the raw call type for callers that build their own requests.
pub type RawMethodCall = MethodCall;
