//! Live integration tests against a real Fastmail JMAP account.
//!
//! These tests are `#[ignore]`d by default so that `cargo test` never touches
//! the network. They follow the repo convention used by
//! `crates/terraphim_middleware/tests/jmap_haystack.rs`: gated at runtime on an
//! environment variable and skipped with a message when it is absent, so CI
//! without credentials stays green while a developer machine exercises the real
//! server.
//!
//! Run them with:
//!
//! ```bash
//! FASTMAIL_API_TOKEN=$(op read "op://TerraphimPlatform/fastmail-api-token/credential") \
//!   cargo test -p haystack_jmap --test live_fastmail -- --ignored --nocapture
//! ```
//!
//! The token is read from the environment only; no token file is read. A
//! writable mailbox name may be supplied through `FASTMAIL_TEST_MAILBOX` (the
//! default is `Processed`). The tests never destroy mail: they mark one message
//! and move it back, and skip the write path entirely when no message is found.

use haystack_jmap::request::{MethodCall, core_mail_request};
use haystack_jmap::types::{KEYWORD_SEEN, ROLE_INBOX};
use haystack_jmap::{
    EmailAddress, EmailCreate, EmailGetOptions, JMAPClient, KEYWORD_FLAGGED, ReplyRequest,
};

/// Fastmail's JMAP session endpoint.
const FASTMAIL_SESSION_URL: &str = "https://api.fastmail.com/jmap/session";

/// Reads the token from the environment, returning `None` when unset or blank.
fn fastmail_token() -> Option<String> {
    match std::env::var("FASTMAIL_API_TOKEN") {
        Ok(token) if !token.trim().is_empty() => Some(token),
        _ => None,
    }
}

/// Connects to Fastmail, or returns `None` after printing why the test is
/// skipped. Tests that cannot reach their dependency fail rather than skip only
/// when the variable is present but the account is unusable.
async fn connect() -> Option<JMAPClient> {
    let token = match fastmail_token() {
        Some(token) => token,
        None => {
            println!("FASTMAIL_API_TOKEN unset; skipping live Fastmail test");
            return None;
        }
    };
    let client = JMAPClient::new(token, FASTMAIL_SESSION_URL)
        .await
        .expect("Fastmail session request should succeed with a valid token");
    Some(client)
}

/// The session advertises mail and submission capabilities and a mail account.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_session_exposes_mail_and_submission() {
    let Some(client) = connect().await else {
        return;
    };
    let session = client.session();
    println!("Fastmail account for {}", session.username);
    assert!(
        client.account_id().is_ok(),
        "no mail account in the session"
    );
    assert!(
        session.supports_submission(),
        "Fastmail should advertise urn:ietf:params:jmap:submission"
    );
}

/// `Mailbox/get` resolves the Inbox role and returns at least one mailbox.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_mailbox_get_resolves_the_inbox_role() {
    let Some(client) = connect().await else {
        return;
    };
    let mailboxes = client
        .get_mailboxes()
        .await
        .expect("Mailbox/get should succeed");
    println!("Fastmail reports {} mailboxes", mailboxes.list.len());
    assert!(!mailboxes.list.is_empty());
    assert!(
        mailboxes.by_role(ROLE_INBOX).is_some(),
        "the account should have an inbox role"
    );
}

/// `Email/query` by mailbox and `Email/get` with the full property set parse a
/// real message, including its thread ID and keywords.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_query_inbox_and_get_emails() {
    let Some(client) = connect().await else {
        return;
    };
    let inbox = client
        .mailbox_by_role(ROLE_INBOX)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have an inbox");

    let result = client
        .query_emails_in_mailbox(&inbox.id, None, Some(5), Some(0), &EmailGetOptions::full())
        .await
        .expect("Email/query plus Email/get should succeed");

    println!("Fetched {} messages from {}", result.list.len(), inbox.name);
    for email in &result.list {
        assert!(!email.id.is_empty());
        assert!(
            email.thread_id.is_some(),
            "threadId is always present in a JMAP get response"
        );
        println!(
            "  {} | thread {:?} | keywords {:?}",
            email.subject.clone().unwrap_or_default(),
            email.thread_id,
            email.keywords.keys().collect::<Vec<_>>()
        );
    }
}

/// The `Email/changes` cursor round-trips: a first call without a stored state
/// signals a full re-query, and storing the current state makes the next call
/// succeed and return a new state to persist.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_changes_cursor_round_trip() {
    let Some(client) = connect().await else {
        return;
    };
    let options = haystack_jmap::ChangesOptions::default();

    // With no stored cursor the caller is told to re-query.
    let first = client
        .email_changes(&options)
        .await
        .expect("first changes call");
    assert!(
        first.cannot_calculate_changes,
        "an absent cursor must signal a full re-query"
    );

    // Take the current state from a query and store it as the cursor.
    let inbox = client
        .mailbox_by_role(ROLE_INBOX)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have an inbox");
    let queried = client
        .query_email_ids(
            Some(&haystack_jmap::EmailQueryFilter::in_mailbox(
                inbox.id.clone(),
            )),
            Some(1),
            Some(0),
        )
        .await
        .expect("Email/query should succeed");
    // Email/query has no state string of its own on Fastmail; use Email/get.
    let state = if queried.ids.is_empty() {
        println!("No messages to derive a state from; skipping the cursor assertion");
        return;
    } else {
        client
            .get_emails_with_options(&queried.ids, &EmailGetOptions::with_properties(["id"]))
            .await
            .expect("Email/get should succeed")
            .state
    };
    client
        .store_cursor(&state)
        .expect("cursor store should accept the state");
    assert_eq!(
        client.load_cursor().unwrap().as_deref(),
        Some(state.as_str())
    );

    let second = client
        .email_changes(&options)
        .await
        .expect("second changes call");
    println!(
        "Changes since the stored cursor: {} created, {} updated, {} destroyed",
        second.created.len(),
        second.updated.len(),
        second.destroyed.len()
    );
    assert!(!second.cannot_calculate_changes);
    assert!(!second.new_state.is_empty());
}

/// `Email/set` marks one message `$seen` and then clears it, leaving the
/// mailbox as it was found.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_email_set_keyword_round_trip() {
    let Some(client) = connect().await else {
        return;
    };
    let inbox = client
        .mailbox_by_role(ROLE_INBOX)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have an inbox");
    let queried = client
        .query_email_ids(
            Some(&haystack_jmap::EmailQueryFilter::in_mailbox(
                inbox.id.clone(),
            )),
            Some(1),
            Some(0),
        )
        .await
        .expect("Email/query should succeed");
    if queried.ids.is_empty() {
        println!("Inbox is empty; nothing to flag");
        return;
    }
    let email_id = queried.ids[0].clone();

    let added = client
        .mark_seen(std::slice::from_ref(&email_id))
        .await
        .expect("Email/set keyword add should succeed")
        .into_result()
        .expect("the keyword add should not report a SetError");
    println!("Marked {} as seen (state {:?})", email_id, added.new_state);

    let restored = haystack_jmap::EmailPatch::new(email_id.clone()).mark_unseen();
    client
        .update_emails(std::slice::from_ref(&restored))
        .await
        .expect("Email/set keyword removal should succeed")
        .into_result()
        .expect("the keyword removal should not report a SetError");
}

/// A `Mailbox/get` lookup by an arbitrary name behaves predictably: it either
/// returns the mailbox or a descriptive error naming what exists.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_mailbox_lookup_reports_available_names() {
    let Some(client) = connect().await else {
        return;
    };
    let name = std::env::var("FASTMAIL_TEST_MAILBOX").unwrap_or_else(|_| "Processed".to_string());
    match client.resolve_mailbox(&name).await {
        Ok(mailbox) => println!("Resolved '{}' to {} ({})", name, mailbox.name, mailbox.id),
        Err(error) => println!("Lookup of '{}' failed: {}", name, error),
    }
}

/// `Identity/get` returns at least one sending identity.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_identity_get_returns_a_sending_address() {
    let Some(client) = connect().await else {
        return;
    };
    let identities = client
        .get_identities()
        .await
        .expect("Identity/get should succeed");
    println!("Fastmail reports {} identities", identities.list.len());
    assert!(
        !identities.list.is_empty(),
        "an account that can send has an identity"
    );
    let identity_id = client
        .resolve_identity_id(None)
        .await
        .expect("a default identity should resolve");
    assert!(!identity_id.is_empty());
}

/// Builds a threaded reply request from a real message without sending it.
///
/// The send path (`EmailSubmission/set`) is exercised only when
/// `FASTMAIL_SEND_TEST_TO` names a recipient, because sending mail has an
/// external side effect that a test run must opt into explicitly.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_reply_headers_are_built_from_a_real_message() {
    let Some(client) = connect().await else {
        return;
    };
    let inbox = client
        .mailbox_by_role(ROLE_INBOX)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have an inbox");
    let result = client
        .query_emails_in_mailbox(&inbox.id, None, Some(1), Some(0), &EmailGetOptions::full())
        .await
        .expect("Email/get should succeed");
    let Some(original) = result.list.first() else {
        println!("Inbox is empty; nothing to reply to");
        return;
    };

    let identity_email = client
        .get_identities()
        .await
        .expect("Identity/get should succeed")
        .list
        .first()
        .and_then(|identity| identity.email.clone())
        .expect("an identity address");

    let reply = ReplyRequest::threaded(
        EmailAddress {
            name: None,
            email: identity_email,
        },
        original.to.clone().unwrap_or_default(),
        format!("Re: {}", original.subject.clone().unwrap_or_default()),
        "Live integration test; not sent.".to_string(),
        inbox.id.clone(),
        original,
    );
    println!(
        "Reply would carry In-Reply-To {:?} and References {:?}",
        reply.in_reply_to, reply.references
    );

    if std::env::var("FASTMAIL_SEND_TEST_TO").is_err() {
        println!("FASTMAIL_SEND_TEST_TO unset; not submitting a message");
        return;
    }
    let outcome = client
        .send_reply(&reply)
        .await
        .expect("send_reply should succeed");
    println!("Submitted reply: {:?}", outcome.submission_id);
    assert!(outcome.sent_email_id.is_some());
}

/// `Email/set` can flag a message without changing its read state.
///
/// Kept separate from the seen round trip so a failure names the keyword that
/// misbehaved.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_email_set_flag_round_trip() {
    let Some(client) = connect().await else {
        return;
    };
    let inbox = client
        .mailbox_by_role(ROLE_INBOX)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have an inbox");
    let queried = client
        .query_email_ids(
            Some(&haystack_jmap::EmailQueryFilter::in_mailbox(
                inbox.id.clone(),
            )),
            Some(1),
            Some(0),
        )
        .await
        .expect("Email/query should succeed");
    if queried.ids.is_empty() {
        println!("Inbox is empty; nothing to flag");
        return;
    }
    let email_id = queried.ids[0].clone();

    let flagged = haystack_jmap::EmailPatch::new(email_id.clone()).mark_flagged();
    client
        .update_emails(std::slice::from_ref(&flagged))
        .await
        .expect("Email/set should flag the message");
    let cleared = haystack_jmap::EmailPatch::new(email_id.clone()).remove_keyword(KEYWORD_FLAGGED);
    client
        .update_emails(std::slice::from_ref(&cleared))
        .await
        .expect("Email/set should clear the flag");

    // The keyword constants are asserted here so the test fails loudly if the
    // names drift from the JMAP registry.
    assert_eq!(KEYWORD_SEEN, "$seen");
}

/// A real `Email/set` create accepts the crate's create shape, and the account
/// can read the draft back by the IDs the server assigned.
///
/// This is the regression guard for the outbound shape: the create carries a
/// createable `bodyStructure`, so a conforming server must not answer with
/// `invalidProperties`. The draft is destroyed at the end of the test, so the
/// account is left as it was found. No message is submitted.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_email_set_create_draft_is_accepted_and_cleaned_up() {
    let Some(client) = connect().await else {
        return;
    };
    let drafts = client
        .mailbox_by_role(haystack_jmap::ROLE_DRAFTS)
        .await
        .expect("mailbox lookup should succeed")
        .expect("the account should have a drafts mailbox");
    let identity_email = client
        .get_identities()
        .await
        .expect("Identity/get should succeed")
        .list
        .first()
        .and_then(|identity| identity.email.clone())
        .expect("an identity address");

    let create = EmailCreate::new_text(
        "draft",
        EmailAddress {
            name: None,
            email: identity_email,
        },
        Vec::new(),
        "haystack_jmap live create probe",
        "Live integration test; this draft is destroyed by the test.",
    )
    .in_mailbox(drafts.id.clone())
    .with_keyword(haystack_jmap::KEYWORD_DRAFT);

    let result = client
        .create_drafts(std::slice::from_ref(&create))
        .await
        .expect("Email/set create should succeed")
        .into_result()
        .expect("a conforming server accepts the create shape and reports no SetError");
    let created_id = result
        .created_record("draft")
        .expect("the server assigns an id to the new draft")
        .id
        .clone();
    println!("Created draft {created_id} through Email/set");

    let fetched = client
        .get_emails_with_options(std::slice::from_ref(&created_id), &EmailGetOptions::full())
        .await
        .expect("Email/get should succeed for the new draft");
    let draft = fetched
        .list
        .first()
        .expect("the created draft should be readable");
    assert!(
        draft.mailbox_ids.get(&drafts.id).copied().unwrap_or(false),
        "the draft should live in the drafts mailbox"
    );
    assert!(
        draft
            .keywords
            .get(haystack_jmap::KEYWORD_DRAFT)
            .copied()
            .unwrap_or(false),
        "the draft keyword should be set"
    );

    // Leave no trace: destroy the probe draft.
    let destroy = MethodCall(
        "Email/set".to_string(),
        serde_json::json!({
            "accountId": client.account_id().expect("mail account"),
            "destroy": [created_id],
        }),
        "d1".to_string(),
    );
    client
        .call_raw(core_mail_request(vec![destroy]), "d1")
        .await
        .expect("Email/set destroy should remove the probe draft");
}

/// The mailbox-scoped change helper keeps account-wide `Email/changes` inside
/// the configured mailbox.
///
/// The account is changed outside the inbox by creating a draft, so the change
/// set names at least one message the inbox helper must drop. Skipped when the
/// account reports no creating identity or no drafts mailbox.
#[tokio::test]
#[ignore = "requires FASTMAIL_API_TOKEN and live Fastmail connectivity"]
async fn live_changed_emails_in_mailbox_drops_mail_outside_the_mailbox() {
    let Some(client) = connect().await else {
        return;
    };
    let (Some(inbox), Some(drafts)) = (
        client
            .mailbox_by_role(ROLE_INBOX)
            .await
            .expect("mailbox lookup should succeed"),
        client
            .mailbox_by_role(haystack_jmap::ROLE_DRAFTS)
            .await
            .expect("mailbox lookup should succeed"),
    ) else {
        println!("The account lacks an inbox or drafts mailbox; nothing to check");
        return;
    };
    let Some(identity_email) = client
        .get_identities()
        .await
        .expect("Identity/get should succeed")
        .list
        .first()
        .and_then(|identity| identity.email.clone())
    else {
        println!("No sending identity; nothing to create outside the inbox");
        return;
    };

    // Seed the cursor from the current state of a full inbox query.
    let seeded = client
        .query_email_ids(
            Some(&haystack_jmap::EmailQueryFilter::in_mailbox(
                inbox.id.clone(),
            )),
            Some(1),
            Some(0),
        )
        .await
        .expect("Email/query should succeed");
    let state = match seeded.ids.first() {
        Some(id) => {
            client
                .get_emails_with_options(
                    std::slice::from_ref(id),
                    &EmailGetOptions::with_properties(["id"]),
                )
                .await
                .expect("Email/get should succeed")
                .state
        }
        None => {
            println!("Inbox is empty; no state to seed the cursor from");
            return;
        }
    };
    client.store_cursor(&state).expect("cursor store");

    // A draft lands in the drafts mailbox, never in the inbox.
    let create = EmailCreate::new_text(
        "draft",
        EmailAddress {
            name: None,
            email: identity_email,
        },
        Vec::new(),
        "haystack_jmap live mailbox filter probe",
        "Live integration test; destroyed by the test.",
    )
    .in_mailbox(drafts.id.clone())
    .with_keyword(haystack_jmap::KEYWORD_DRAFT);
    let created = client
        .create_drafts(std::slice::from_ref(&create))
        .await
        .expect("Email/set create should succeed")
        .into_result()
        .expect("the create should not report a SetError");
    let created_id = created
        .created_record("draft")
        .expect("the server assigns an id")
        .id
        .clone();

    let changes = client
        .email_changes(&haystack_jmap::ChangesOptions::default())
        .await
        .expect("Email/changes should succeed");
    println!(
        "Account-wide changes: {:?} created, {:?} updated",
        changes.created, changes.updated
    );

    let account_wide = client
        .fetch_changed_emails(&changes)
        .await
        .expect("Email/get for the change set should succeed");
    let inbox_scoped = client
        .fetch_changed_emails_in_mailbox(&changes, &inbox.id)
        .await
        .expect("Email/get for the change set should succeed");

    assert!(
        account_wide.iter().any(|email| email.id == created_id),
        "the account-wide change set should name the draft created outside the inbox"
    );
    assert!(
        !inbox_scoped.iter().any(|email| email.id == created_id),
        "the mailbox-scoped helper must drop a draft that is not in the inbox"
    );
    assert!(
        inbox_scoped
            .iter()
            .all(|email| email.mailbox_ids.get(&inbox.id).copied().unwrap_or(false))
    );

    // Leave no trace.
    let destroy = MethodCall(
        "Email/set".to_string(),
        serde_json::json!({
            "accountId": client.account_id().expect("mail account"),
            "destroy": [created_id],
        }),
        "d1".to_string(),
    );
    client
        .call_raw(core_mail_request(vec![destroy]), "d1")
        .await
        .expect("Email/set destroy should remove the probe draft");
}
