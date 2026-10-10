//! Wire-format tests for the JMAP request and response types.
//!
//! The JSON fixtures below are the examples published in RFC 8620 (the core
//! JMAP protocol) and RFC 8621 (JMAP for Mail). They exercise the exact shapes
//! the crate sends and parses; no HTTP server is involved, so the tests are
//! pure (de)serialisation checks against the specification.
//!
//! Section references name the RFC and the section the example is taken from.

use haystack_jmap::request::{
    ChangesOptions, EmailCreate, EmailGetOptions, EmailPatch, EmailQueryFilter, EmailSubmission,
    Envelope, JMAPRequest, JMAPResponse, MethodCall, core_mail_request, email_changes_call,
    email_get_call, email_get_referencing_query_call, email_query_call, email_set_create_call,
    email_set_update_call, email_submission_set_call, identity_get_call, mailbox_get_call,
    submission_request,
};
use haystack_jmap::response::{
    EmailChangesResult, EmailGetResult, EmailQueryResult, IdentityGetResult, MailboxGetResult,
    SetResult, cannot_calculate_changes_error,
};
use haystack_jmap::types::{
    Email, EmailAddress, KEYWORD_ANSWERED, KEYWORD_DRAFT, KEYWORD_SEEN, ROLE_INBOX,
};
use serde_json::{Value, json};

/// Parses a `methodCalls` array into the crate's request envelope.
fn request_from_method_calls(using: Vec<String>, method_calls: Value) -> JMAPRequest {
    let calls: Vec<MethodCall> = serde_json::from_value(method_calls).unwrap();
    JMAPRequest {
        using,
        method_calls: calls,
        created_ids: None,
    }
}

/// Serialises a value and returns its compact JSON text.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

// RFC 8621 section 4.10: the inbox query filters by mailbox, sorts newest
// first, collapses threads, pages from position 0 and asks for the total.
#[test]
fn rfc8621_4_10_inbox_query_matches_the_published_filter_shape() {
    let published: Value = json!([["Email/query", {
        "accountId": "ue150411c",
        "filter": {
            "inMailbox": "fb666a55"
        },
        "sort": [{
            "isAscending": false,
            "property": "receivedAt"
        }],
        "collapseThreads": true,
        "position": 0,
        "limit": 30,
        "calculateTotal": true
    }, "0"]]);

    let expected_filter = published[0][1]["filter"].clone();
    let call = email_query_call(
        "ue150411c",
        Some(&EmailQueryFilter::in_mailbox("fb666a55")),
        Some(30),
        Some(0),
        "0",
    );
    assert_eq!(call.1["filter"], expected_filter);
    assert_eq!(call.1["accountId"], "ue150411c");
    assert_eq!(call.1["limit"], 30);
    assert_eq!(call.1["position"], 0);
    assert_eq!(call.0, "Email/query");
    assert_eq!(call.2, "0");
}

// RFC 8620 section 5.7 (adapted to Email): OR of hasKeyword conditions must
// serialise as a FilterOperator with the two conditions in order.
#[test]
fn rfc8620_5_7_or_filter_operator_serialises_as_published() {
    let published: Value = json!({
        "operator": "OR",
        "conditions": [
            { "hasKeyword": "music" },
            { "hasKeyword": "video" }
        ]
    });

    let filter = EmailQueryFilter::or(vec![
        EmailQueryFilter::HasKeyword("music".to_string()),
        EmailQueryFilter::HasKeyword("video".to_string()),
    ]);
    assert_eq!(filter.to_json(), published);
}

// RFC 8621 section 4.4.1: a mailbox filter combines with other conditions
// through the and filter, which nests as a FilterOperator.
#[test]
fn rfc8621_4_4_1_mailbox_and_keyword_filter_nests_correctly() {
    let filter = EmailQueryFilter::and(vec![
        EmailQueryFilter::in_mailbox("fb666a55"),
        EmailQueryFilter::negate(EmailQueryFilter::HasKeyword(KEYWORD_SEEN.to_string())),
    ]);

    assert_eq!(
        filter.to_json(),
        json!({
            "operator": "AND",
            "conditions": [
                { "inMailbox": "fb666a55" },
                { "operator": "NOT", "conditions": [ { "hasKeyword": "$seen" } ] }
            ]
        })
    );
}

// RFC 8621 section 4.10: the query response carries accountId, queryState,
// canCalculateChanges, position, total and ids.
#[test]
fn rfc8621_4_10_query_response_parses_ids_total_and_state() {
    let published = r#"[[ "Email/query", {
      "accountId": "ue150411c",
      "queryState": "09aa9a075588-780599:0",
      "canCalculateChanges": true,
      "position": 0,
      "total": 115,
      "ids": [ "Ma783e5cdf5f2deffbc97930a",
        "M9bd17497e2a99cb345fc1d0a" ]
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = EmailQueryResult::from_value(envelope.result("0").unwrap()).unwrap();

    assert_eq!(result.account_id, "ue150411c");
    assert_eq!(result.query_state, "09aa9a075588-780599:0");
    assert_eq!(result.total, Some(115));
    assert_eq!(result.position, 0);
    assert_eq!(
        result.ids,
        vec![
            "Ma783e5cdf5f2deffbc97930a".to_string(),
            "M9bd17497e2a99cb345fc1d0a".to_string()
        ]
    );
    assert_eq!(result.can_calculate_changes, Some(true));
}

// RFC 8621 section 4.10: the query and get calls share one request through an
// #ids back-reference, so a poll is a single round trip.
#[test]
fn rfc8621_4_10_query_then_get_uses_an_ids_back_reference() {
    let published: Value = json!([ "Email/get", {
        "accountId": "ue150411c",
        "#ids": {
            "resultOf": "0",
            "name": "Email/query",
            "path": "/ids"
        },
        "properties": [
            "threadId"
        ]
    }, "1" ]);

    let call = email_get_referencing_query_call(
        "ue150411c",
        "0",
        &EmailGetOptions::with_properties(["threadId"]),
        "1",
    );

    assert_eq!(call.1["accountId"], published[1]["accountId"]);
    assert_eq!(call.1["#ids"], published[1]["#ids"]);
    assert_eq!(call.1["properties"], published[1]["properties"]);
    // The literal ids array must be absent: the reference replaces it.
    assert!(call.1.get("ids").is_none());
}

// RFC 8621 section 4.2.1: the Email/get request carries ids, properties,
// bodyProperties, fetchHTMLBodyValues and maxBodyValueBytes.
#[test]
fn rfc8621_4_2_1_get_request_carries_properties_and_body_options() {
    let published: Value = json!([ "Email/get", {
        "ids": [ "f123u456", "f123u457" ],
        "properties": [ "threadId", "mailboxIds", "from", "subject",
          "receivedAt", "header:List-POST:asURLs",
          "htmlBody", "bodyValues" ],
        "bodyProperties": [ "partId", "blobId", "size", "type" ],
        "fetchHTMLBodyValues": true,
        "maxBodyValueBytes": 256
    }, "#1" ]);

    let ids = vec!["f123u456".to_string(), "f123u457".to_string()];
    let options = EmailGetOptions {
        properties: vec![
            "threadId".to_string(),
            "mailboxIds".to_string(),
            "from".to_string(),
            "subject".to_string(),
            "receivedAt".to_string(),
            "header:List-POST:asURLs".to_string(),
            "htmlBody".to_string(),
            "bodyValues".to_string(),
        ],
        fetch_text_body_values: true,
        fetch_html_body_values: true,
        max_body_value_bytes: Some(256),
    };
    let call = email_get_call("abc", &ids, &options, "#1");

    assert_eq!(call.1["ids"], published[1]["ids"]);
    assert_eq!(call.1["properties"], published[1]["properties"]);
    assert_eq!(
        call.1["fetchHTMLBodyValues"],
        published[1]["fetchHTMLBodyValues"]
    );
    assert_eq!(
        call.1["maxBodyValueBytes"],
        published[1]["maxBodyValueBytes"]
    );
}

// RFC 8621 section 4.2.1: a get response keeps threadId, mailboxIds, headers,
// htmlBody parts and bodyValues, and reports notFound ids.
#[test]
fn rfc8621_4_2_1_get_response_parses_threading_and_headers() {
    let published = r##"[[ "Email/get", {
     "accountId": "abc",
     "state": "41234123231",
     "list": [
       {
         "id": "f123u457",
         "threadId": "ef1314a",
         "mailboxIds": { "f123": true },
         "from": [{ "name": "Joe Bloggs", "email": "joe@example.com" }],
         "subject": "Dinner on Thursday?",
         "receivedAt": "2013-10-13T14:12:00Z",
         "header:List-POST:asURLs": [
           "mailto:partytime@lists.example.com"
         ],
         "htmlBody": [{
           "partId": "1",
           "blobId": "B841623871",
           "size": 283331,
           "type": "text/html"
         }, {
           "partId": "2",
           "blobId": "B319437193",
           "size": 10343,
           "type": "text/plain"
         }],
         "bodyValues": {
           "1": {
             "isEncodingProblem": false,
             "isTruncated": true,
             "value": "<html><body><p>Hello ..."
           },
           "2": {
             "isEncodingProblem": false,
             "isTruncated": false,
             "value": "-- Sent by your friendly mailing list ..."
           }
         }
       }
     ],
     "notFound": [ "f123u456" ]
   }, "#1" ]]"##;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = EmailGetResult::from_value(envelope.result("#1").unwrap()).unwrap();

    assert_eq!(result.account_id, "abc");
    assert_eq!(result.state, "41234123231");
    assert_eq!(result.not_found, vec!["f123u456".to_string()]);
    assert_eq!(result.list.len(), 1);

    let email = &result.list[0];
    assert_eq!(email.id, "f123u457");
    assert_eq!(email.thread_id.as_deref(), Some("ef1314a"));
    assert_eq!(email.mailbox_ids.get("f123"), Some(&true));
    assert_eq!(email.subject.as_deref(), Some("Dinner on Thursday?"));
    assert_eq!(email.html_body.len(), 2);
    assert_eq!(email.html_body[0].type_.as_deref(), Some("text/html"));
    assert_eq!(
        email.body_values["2"].value,
        "-- Sent by your friendly mailing list ..."
    );

    // The generic header property survives as a flattened field.
    assert_eq!(
        email.headers["header:List-POST:asURLs"],
        json!(["mailto:partytime@lists.example.com"])
    );
}

// RFC 8621 section 4.1.3: the parsed header properties for threading, the
// Authentication-Results trust marker and the received chain.
#[test]
fn rfc8621_4_1_3_email_get_parses_threading_keywords_and_auth_headers() {
    let published = r#"{
      "id": "M123",
      "threadId": "T123",
      "keywords": { "$seen": true, "$answered": true },
      "mailboxIds": { "MB23cfa8094c0f41e6": true },
      "messageId": ["<abc@example.com>"],
      "inReplyTo": ["<parent@example.com>"],
      "references": ["<root@example.com>", "<parent@example.com>"],
      "cc": [{ "name": null, "email": "carol@example.com" }],
      "header:Authentication-Results:asText": "mx.example.com; spf=pass; dkim=pass",
      "header:Received:asText": "from mail.example.com by mx.example.com; Tue, 10 Jul 2018 11:03:11 +1000",
      "header:X-ME-Message-Id:asRaw": "<marker-1>"
    }"#;

    let email: Email = serde_json::from_str(published).unwrap();

    assert_eq!(email.thread_id.as_deref(), Some("T123"));
    assert_eq!(email.keywords.get(KEYWORD_SEEN), Some(&true));
    assert_eq!(email.keywords.get(KEYWORD_ANSWERED), Some(&true));
    assert_eq!(email.message_id.as_ref().unwrap()[0], "<abc@example.com>");
    assert_eq!(
        email.in_reply_to.as_ref().unwrap()[0],
        "<parent@example.com>"
    );
    assert_eq!(email.references.as_ref().unwrap().len(), 2);
    assert_eq!(email.cc.as_ref().unwrap()[0].email, "carol@example.com");
    assert_eq!(
        email.headers["header:Authentication-Results:asText"],
        json!("mx.example.com; spf=pass; dkim=pass")
    );
    assert!(
        email.headers["header:Received:asText"]
            .as_str()
            .unwrap()
            .starts_with("from mail.example.com")
    );
    assert_eq!(
        email.headers["header:X-ME-Message-Id:asRaw"],
        json!("<marker-1>")
    );
}

// Round-trip: an Email parsed from a get response re-serialises to the same
// JSON, so nothing is silently dropped or renamed.
#[test]
fn email_round_trips_through_serde() {
    let published: Value = json!({
      "id": "M123",
      "threadId": "T123",
      "keywords": { "$seen": true },
      "mailboxIds": { "MB1": true },
      "cc": [{ "email": "carol@example.com" }],
      "messageId": ["<abc@example.com>"],
      "header:Authentication-Results:asText": "spf=pass"
    });

    let email: Email = serde_json::from_value(published.clone()).unwrap();
    let rendered: Value = serde_json::to_value(&email).unwrap();
    assert_eq!(rendered, published);
}

// RFC 8621 section 4.10: the changes request carries sinceState and
// maxChanges, and the response carries oldState, newState, hasMoreChanges and
// the created/updated/destroyed id lists.
#[test]
fn rfc8621_4_10_changes_request_and_response_round_trip() {
    let published_request: Value = json!([[ "Email/changes", {
        "accountId": "ue150411c",
        "sinceState": "780605",
        "maxChanges": 50
    }, "3" ]]);

    let options = ChangesOptions {
        max_changes: Some(50),
        include_updated: true,
    };
    let call = email_changes_call("ue150411c", "780605", &options, "3");
    assert_eq!(call.1["accountId"], published_request[0][1]["accountId"]);
    assert_eq!(call.1["sinceState"], published_request[0][1]["sinceState"]);
    assert_eq!(call.1["maxChanges"], published_request[0][1]["maxChanges"]);

    let published_response = r#"[[ "Email/changes", {
      "accountId": "ue150411c",
      "oldState": "780605",
      "newState": "780800",
      "hasMoreChanges": false,
      "created": [ "Me8de6c9f6de198239b982ea2" ],
      "updated": [ "Ma783e5cdf5f2deffbc97930a" ],
      "destroyed": [ "M9bd17497e2a99cb345fc1d0a" ]
    }, "3" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published_response).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let parsed = EmailChangesResult::from_value(envelope.result("3").unwrap()).unwrap();

    assert_eq!(parsed.old_state, "780605");
    assert_eq!(parsed.new_state, "780800");
    assert!(!parsed.has_more_changes);
    assert_eq!(
        parsed.created,
        vec!["Me8de6c9f6de198239b982ea2".to_string()]
    );
    assert_eq!(
        parsed.updated,
        vec!["Ma783e5cdf5f2deffbc97930a".to_string()]
    );
    assert_eq!(
        parsed.destroyed,
        vec!["M9bd17497e2a99cb345fc1d0a".to_string()]
    );

    let (changes, cursor) = parsed.into_changes("780605");
    assert!(!changes.cannot_calculate_changes);
    assert_eq!(cursor.as_deref(), Some("780800"));
    assert_eq!(changes.new_state, "780800");
    assert_eq!(changes.affected_ids().len(), 3);
}

// RFC 8620 section 5.2: when the server cannot calculate changes it returns a
// cannotCalculateChanges error, which the client turns into a re-query signal.
#[test]
fn rfc8620_5_2_cannot_calculate_changes_signals_a_full_requery() {
    let published_error = cannot_calculate_changes_error();
    assert_eq!(published_error["type"], "cannotCalculateChanges");

    let parsed = EmailChangesResult::from_value(&published_error).unwrap();
    let (changes, cursor) = parsed.into_changes("780605");

    assert!(changes.cannot_calculate_changes);
    assert!(cursor.is_none());
    assert!(changes.created.is_empty());

    // The same error delivered inside an envelope must surface as an error
    // result from the envelope accessor.
    let envelope = JMAPResponse {
        method_responses: serde_json::from_value(json!([
            ["error", {
                "type": "cannotCalculateChanges",
                "description": "state too old"
            }, "ch1"]
        ]))
        .unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let error = envelope.error().unwrap();
    assert!(error.is_cannot_calculate_changes());
    assert_eq!(error.description.as_deref(), Some("state too old"));
    assert_eq!(error.to_string(), "cannotCalculateChanges: state too old");
    assert!(envelope.result("ch1").is_err());
}

// RFC 8620 section 5.2: hasMoreChanges with an intermediate state lets a
// client page through a large change set.
#[test]
fn rfc8620_5_2_intermediate_state_reports_more_changes() {
    let published = r#"{
      "accountId": "x",
      "oldState": "B",
      "newState": "B-D-E",
      "hasMoreChanges": true,
      "created": [],
      "updated": [ "M1" ],
      "destroyed": []
    }"#;

    let parsed = EmailChangesResult::from_value(&serde_json::from_str(published).unwrap()).unwrap();
    assert!(parsed.has_more_changes);
    let (changes, cursor) = parsed.into_changes("B");
    assert_eq!(cursor.as_deref(), Some("B-D-E"));
    assert!(!changes.cannot_calculate_changes);
}

// RFC 8621 section 2.6: Mailbox/get returns name, role, parentId and counts,
// and the client resolves a mailbox by role.
#[test]
fn rfc8621_2_6_mailbox_get_parses_roles_and_resolves_by_name_or_role() {
    let published = r#"[[ "Mailbox/get", {
      "accountId": "u33084183",
      "state": "78540",
      "list": [{
        "id": "MB23cfa8094c0f41e6",
        "name": "Inbox",
        "parentId": null,
        "role": "inbox",
        "sortOrder": 10,
        "totalEmails": 16307,
        "unreadEmails": 13905,
        "totalThreads": 5833,
        "unreadThreads": 5128,
        "myRights": {
          "mayAddItems": true,
          "mayRename": false,
          "maySubmit": true,
          "mayDelete": false,
          "maySetKeywords": true,
          "mayRemoveItems": true,
          "mayCreateChild": true,
          "maySetSeen": true,
          "mayReadItems": true
        },
        "isSubscribed": true
      }, {
        "id": "MB674cc24095db49ce",
        "name": "Processed",
        "parentId": null,
        "role": null,
        "totalEmails": 42,
        "unreadEmails": 0
      }, {
        "id": "MB99",
        "name": "Quarantine",
        "role": null
      }],
      "notFound": []
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = MailboxGetResult::from_value(envelope.result("0").unwrap()).unwrap();
    assert_eq!(result.account_id, "u33084183");
    assert_eq!(result.state, "78540");

    let list = result.into_list();
    let inbox = list.by_role(ROLE_INBOX).unwrap();
    assert_eq!(inbox.id, "MB23cfa8094c0f41e6");
    assert_eq!(inbox.total_emails, Some(16307));
    assert_eq!(inbox.unread_emails, Some(13905));
    assert_eq!(inbox.subscribed, Some(true));

    // Name lookup is case-insensitive and finds mailboxes without a role.
    assert_eq!(list.by_name("processed").unwrap().id, "MB674cc24095db49ce");
    assert_eq!(list.by_name("QUARANTINE").unwrap().id, "MB99");
    assert!(list.by_role("archive").is_none());
}

// RFC 8621 section 2.6: the Mailbox/get request asks for the mailbox list.
#[test]
fn rfc8621_2_6_mailbox_get_request_names_the_account() {
    let call = mailbox_get_call("u33084183", "0");
    assert_eq!(call.0, "Mailbox/get");
    assert_eq!(call.1["accountId"], "u33084183");
    assert!(
        call.1["properties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "role")
    );
}

// RFC 8621 section 6.4: Identity/get returns the sending identities, and the
// client picks one by address or falls back to the first.
#[test]
fn rfc8621_6_4_identity_get_parses_and_resolves_by_address() {
    let published = r#"[ "Identity/get", {
      "accountId": "acme",
      "state": "99401312ae-11-333",
      "list": [
        {
          "id": "XD-3301-222-11_22AAz",
          "name": "Joe Bloggs",
          "email": "joe@example.com",
          "replyTo": null,
          "bcc": [{
            "name": null,
            "email": "joe+archive@example.com"
          }],
          "textSignature": "-- \nJoe Bloggs\nMaster of Email",
          "htmlSignature": "<div><b>Joe Bloggs</b></div>",
          "mayDelete": false
        },
        {
          "id": "XD-9911312-11_22AAz",
          "name": "Joe B",
          "email": "*@example.com",
          "replyTo": null,
          "bcc": null,
          "textSignature": "",
          "htmlSignature": "",
          "mayDelete": true
        }
      ],
      "notFound": []
    }, "0" ]"#;

    let response: haystack_jmap::request::MethodResponse = serde_json::from_str(published).unwrap();
    let result = IdentityGetResult::from_value(&response.1).unwrap();

    assert_eq!(result.account_id, "acme");
    assert_eq!(result.list.len(), 2);
    assert_eq!(
        result.by_email("JOE@example.com").unwrap().id,
        "XD-3301-222-11_22AAz"
    );
    assert_eq!(
        result.default_identity().unwrap().email_or_empty(),
        "joe@example.com"
    );
    assert_eq!(result.list[0].may_delete, Some(false));
    assert!(result.by_email("nobody@example.com").is_none());
}

// RFC 8621 section 6.4: the Identity/get request names the account.
#[test]
fn rfc8621_6_4_identity_get_request_names_the_account() {
    let call = identity_get_call("acme", "0");
    assert_eq!(call.0, "Identity/get");
    assert_eq!(call.1["accountId"], "acme");
}

// RFC 8621 section 4.10: marking a message unread is a patch that sets
// keywords/$seen to null.
#[test]
fn rfc8621_4_10_email_set_keyword_patch_matches_the_published_form() {
    let published: Value = json!([ "Email/set", {
      "accountId": "ue150411c",
      "update": {
        "Ma783e5cdf5f2deffbc97930a": {
          "keywords/$seen": null
        }
      }
    }, "0" ]);

    let patch = EmailPatch::new("Ma783e5cdf5f2deffbc97930a").mark_unseen();
    let call = email_set_update_call("ue150411c", std::slice::from_ref(&patch), "0");

    assert_eq!(call.0, "Email/set");
    assert_eq!(call.1["accountId"], published[1]["accountId"]);
    assert_eq!(call.1["update"], published[1]["update"]);
}

// RFC 8620 section 5.7: the minimal patch syntax sets one keyword and clears
// another in the same object.
#[test]
fn rfc8620_5_7_email_set_adds_and_removes_keywords_in_one_patch() {
    let published: Value = json!({
        "keywords/$answered": true,
        "keywords/$seen": true,
        "keywords/$flagged": null
    });

    let patch = EmailPatch::new("M1")
        .add_keyword(KEYWORD_ANSWERED)
        .mark_seen()
        .remove_keyword("$flagged");

    assert_eq!(patch.to_patch_object(), published);
}

// RFC 8621 section 4.6: moving a message is a patch that removes one mailbox
// and adds another, so membership is never empty.
#[test]
fn rfc8621_4_6_mailbox_move_patch_removes_and_adds_in_one_call() {
    let published: Value = json!({
        "mailboxIds/MB-inbox": null,
        "mailboxIds/MB-processed": true
    });

    let patch = EmailPatch::new("M1").move_to("MB-inbox", "MB-processed");
    assert_eq!(patch.to_patch_object(), published);

    let call = email_set_update_call("u1", std::slice::from_ref(&patch), "0");
    assert_eq!(call.1["update"]["M1"], published);
}

// RFC 8621 section 4.10 and section 4.1.4: a create supplies the text as a
// createable `bodyStructure` plus `bodyValues`. `textBody` and `htmlBody` are
// properties the server derives for `Email/get` responses, so sending them in a
// create is rejected with `invalidProperties`.
#[test]
fn rfc8621_4_10_email_set_create_renders_a_createable_body_structure() {
    let published: Value = json!({
      "mailboxIds": { "2ea1ca41b38e": true },
      "keywords": { "$seen": true, "$draft": true },
      "from": [{ "name": "Joe Bloggs", "email": "joe@example.com" }],
      "subject": "World domination",
      "sentAt": "2018-07-10T11:03:11+10:00",
      "bodyValues": {
        "text": {
          "value": "I have the most brilliant plan."
        }
      },
      "bodyStructure": {
        "partId": "text",
        "type": "text/plain"
      }
    });

    let create = EmailCreate::new_text(
        "k192",
        EmailAddress {
            name: Some("Joe Bloggs".to_string()),
            email: "joe@example.com".to_string(),
        },
        Vec::new(),
        "World domination",
        "I have the most brilliant plan.",
    )
    .in_mailbox("2ea1ca41b38e")
    .with_keyword(KEYWORD_SEEN)
    .with_keyword(KEYWORD_DRAFT)
    .sent_at("2018-07-10T11:03:11+10:00");

    assert_eq!(create.to_json(), published);

    // The derived convenience properties are never part of a create.
    let rendered = create.to_json();
    assert!(rendered.get("textBody").is_none());
    assert!(rendered.get("htmlBody").is_none());
    assert_eq!(
        rendered["bodyStructure"]["partId"],
        rendered["bodyValues"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone()
    );

    let call = email_set_create_call("ue150411c", std::slice::from_ref(&create), "0");
    assert_eq!(call.1["create"]["k192"], published);
}

// RFC 8621 section 4.10: a create response reports id, blobId, threadId and
// size for the new object.
#[test]
fn rfc8621_4_10_email_set_create_response_parses_the_new_ids() {
    let published = r#"[[ "Email/set", {
      "accountId": "ue150411c",
      "oldState": "780823",
      "newState": "780839",
      "created": {
        "k192": {
          "id": "Mf40b5f831efa7233b9eb1c7f",
          "blobId": "Gf40b5f831efa7233b9eb1c7f8f97d84eeeee64f7",
          "threadId": "Td957e72e89f516dc",
          "size": 359
        }
      }
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = SetResult::from_value(envelope.result("0").unwrap()).unwrap();

    assert_eq!(result.old_state, "780823");
    assert_eq!(result.new_state, "780839");
    let created = result.created_record("k192").unwrap();
    assert_eq!(created.id, "Mf40b5f831efa7233b9eb1c7f");
    assert_eq!(created.thread_id.as_deref(), Some("Td957e72e89f516dc"));
    assert_eq!(created.size, Some(359));
    assert!(result.into_result().is_ok());
}

// RFC 8621 section 4.10: an update response lists the changed ids under
// "updated", and the destroy list carries the removed ids.
#[test]
fn rfc8621_4_10_email_set_update_and_destroy_response_parses() {
    let published = r#"[[ "Email/set", {
      "accountId": "ue150411c",
      "oldState": "780839",
      "newState": "780842",
      "destroyed": [ "Mf40b5f831efa7233b9eb1c7f" ],
      "updated": { "Ma783e5cdf5f2deffbc97930a": null }
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = SetResult::from_value(envelope.result("0").unwrap()).unwrap();

    assert_eq!(
        result.destroyed,
        vec!["Mf40b5f831efa7233b9eb1c7f".to_string()]
    );
    assert_eq!(
        result.updated_ids(),
        vec!["Ma783e5cdf5f2deffbc97930a".to_string()]
    );
    assert_eq!(result.new_state, "780842");
}

// RFC 8620 section 5.3: a notUpdated entry carries a SetError with a type and
// description; into_result turns it into an error.
#[test]
fn rfc8620_5_3_set_error_surfaces_through_into_result() {
    let published = r#"[[ "Email/set", {
      "accountId": "u1",
      "oldState": "1",
      "newState": "1",
      "notUpdated": {
        "M1": {
          "type": "invalidProperties",
          "description": "Mailbox does not exist",
          "properties": [ "mailboxIds" ]
        }
      }
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = SetResult::from_value(envelope.result("0").unwrap()).unwrap();

    let error = result.update_error("M1").unwrap();
    assert_eq!(error.type_, "invalidProperties");
    assert_eq!(error.properties.as_ref().unwrap()[0], "mailboxIds");

    let message = result.into_result().unwrap_err().to_string();
    assert!(message.contains("M1"), "error was: {message}");
    assert!(
        message.contains("invalidProperties"),
        "error was: {message}"
    );
}

// RFC 8621 section 7.5.1: the submission request names identityId, emailId and
// an envelope with mailFrom and rcptTo.
#[test]
fn rfc8621_7_5_1_submission_set_renders_identity_email_and_envelope() {
    let published: Value = json!({
        "identityId": "I64588216",
        "emailId": "M7f6ed5bcfd7e2604d1753f6c",
        "envelope": {
            "mailFrom": {
                "email": "john@example.com"
            },
            "rcptTo": [{
                "email": "jane@example.com"
            }]
        }
    });

    let submission = EmailSubmission::new("k1490", "M7f6ed5bcfd7e2604d1753f6c", "I64588216")
        .with_envelope(Envelope {
            mail_from: EmailAddress {
                name: None,
                email: "john@example.com".to_string(),
            },
            rcpt_to: vec![EmailAddress {
                name: None,
                email: "jane@example.com".to_string(),
            }],
        });

    assert_eq!(submission.to_json(), published);

    let call = email_submission_set_call("ue411d190", std::slice::from_ref(&submission), "0");
    assert_eq!(call.0, "EmailSubmission/set");
    assert_eq!(call.1["accountId"], "ue411d190");
    assert_eq!(call.1["create"]["k1490"], published);

    // The submission capability is opted into by the request envelope.
    let request = submission_request(vec![call]);
    assert!(
        request
            .using
            .contains(&"urn:ietf:params:jmap:submission".to_string())
    );
}

// RFC 8621 section 7.5.1: a successful submission response reports the created
// submission id and the state transition.
#[test]
fn rfc8621_7_5_1_submission_response_parses_created_id() {
    let published = r#"[[ "EmailSubmission/set", {
      "accountId": "ue411d190",
      "oldState": "012421s6-8nrq-4ps4-n0p4-9330r951ns21",
      "newState": "355421f6-8aed-4cf4-a0c4-7377e951af36",
      "created": {
        "k1490": {
          "id": "ES-3bab7f9a-623e-4acf-99a5-2e67facb02a0"
        }
      }
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = SetResult::from_value(envelope.result("0").unwrap()).unwrap();

    assert_eq!(
        result.created_record("k1490").unwrap().id,
        "ES-3bab7f9a-623e-4acf-99a5-2e67facb02a0"
    );
    // The implicit Email/set response shares the same call id, so the lookup
    // needs to find the first matching response only.
    assert!(envelope.response_for("0").is_some());
}

// RFC 8621 section 7.5.1: a rejected submission returns notCreated with the
// forbiddenToSend type and a description for display.
#[test]
fn rfc8621_7_5_1_submission_rejection_reports_forbidden_to_send() {
    let published = r#"[[ "EmailSubmission/set", {
      "accountId": "ue411d190",
      "oldState": "012421s6-8nrq-4ps4-n0p4-9330r951ns21",
      "newState": "012421s6-8nrq-4ps4-n0p4-9330r951ns21",
      "notCreated": {
        "k1490": {
          "type": "forbiddenToSend",
          "description": "Sending is blocked for this account."
        }
      }
    }, "0" ]]"#;

    let envelope = JMAPResponse {
        method_responses: serde_json::from_str(published).unwrap(),
        session_state: "s1".to_string(),
        created_ids: None,
    };
    let result = SetResult::from_value(envelope.result("0").unwrap()).unwrap();

    let error = result.creation_error("k1490").unwrap();
    assert_eq!(error.type_, "forbiddenToSend");
    assert_eq!(
        error.description.as_deref(),
        Some("Sending is blocked for this account.")
    );
    assert!(result.into_result().is_err());
}

// A threaded reply names the parent through inReplyTo and references, and the
// create travels with the submission through a #creationId reference.
#[test]
fn threaded_reply_request_carries_in_reply_to_and_references() {
    let create = EmailCreate::new_text(
        "reply",
        EmailAddress {
            name: None,
            email: "me@example.com".to_string(),
        },
        vec![EmailAddress {
            name: None,
            email: "them@example.com".to_string(),
        }],
        "Re: Dinner",
        "Sounds good.",
    )
    .in_mailbox("MB-drafts")
    .with_keyword(KEYWORD_DRAFT)
    .in_reply_to(vec!["<parent@example.com>".to_string()])
    .references(vec![
        "<root@example.com>".to_string(),
        "<parent@example.com>".to_string(),
    ]);

    let rendered = create.to_json();
    assert_eq!(rendered["inReplyTo"], json!(["<parent@example.com>"]));
    assert_eq!(
        rendered["references"],
        json!(["<root@example.com>", "<parent@example.com>"])
    );
    assert_eq!(rendered["keywords"][KEYWORD_DRAFT], json!(true));

    // The submission references the created email, so one request both creates
    // and sends.
    let submission = EmailSubmission::new("submission", "#reply", "I1");
    assert_eq!(submission.to_json()["emailId"], json!("#reply"));
}

// RFC 8620 section 3.3: the request envelope carries "using" and
// "methodCalls", and the response envelope carries "methodResponses".
#[test]
fn rfc8620_3_3_request_and_response_envelopes_match_the_published_shape() {
    let published_request: Value = json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail"
        ],
        "methodCalls": [
            ["Email/query", { "accountId": "x", "filter": { "inMailbox": "MB1" } }, "0"]
        ]
    });

    let request = request_from_method_calls(
        vec![
            "urn:ietf:params:jmap:core".to_string(),
            "urn:ietf:params:jmap:mail".to_string(),
        ],
        published_request["methodCalls"].clone(),
    );
    assert_eq!(serde_json::to_value(&request).unwrap(), published_request);

    // The crate's own builder produces the same envelope shape.
    let built = core_mail_request(vec![email_query_call(
        "x",
        Some(&EmailQueryFilter::in_mailbox("MB1")),
        None,
        None,
        "0",
    )]);
    assert_eq!(serde_json::to_value(&built).unwrap(), published_request);
}

// RFC 8620 section 3.4.1: the response envelope carries methodResponses and
// sessionState.
#[test]
fn rfc8620_3_4_1_response_envelope_parses_method_responses() {
    let published = r#"{
      "methodResponses": [
        ["Email/query", { "accountId": "x", "ids": ["M1"] }, "0"]
      ],
      "sessionState": "75128aab4b1b"
    }"#;

    let response: JMAPResponse = serde_json::from_str(published).unwrap();
    assert_eq!(response.session_state, "75128aab4b1b");
    assert_eq!(response.method_responses.len(), 1);
    assert_eq!(response.method_responses[0].0, "Email/query");
    assert_eq!(response.method_responses[0].2, "0");
    assert_eq!(response.result("0").unwrap()["ids"], json!(["M1"]));
    assert!(response.response_for("missing").is_none());
    assert!(response.result("missing").is_err());
}

// RFC 8620 section 3.6.2: a method-level error is reported as an "error" method
// response carrying a type, and the envelope accessor surfaces it.
#[test]
fn rfc8620_3_6_2_method_level_error_is_surfaced() {
    let published = r#"{
      "methodResponses": [
        ["error", { "type": "unknownMethod" }, "0"]
      ],
      "sessionState": "75128aab4b1b"
    }"#;

    let response: JMAPResponse = serde_json::from_str(published).unwrap();
    let error = response.error().unwrap();
    assert_eq!(error.type_, "unknownMethod");
    assert!(response.result("0").is_err());
}

// RFC 8621 section 4.2: a changes result splits into created, updated and
// destroyed. Only created and updated IDs can be fetched; a destroyed message
// has no data left to get.
#[test]
fn rfc8621_4_2_changes_fetchable_ids_exclude_destroyed() {
    let changes = haystack_jmap::types::EmailChanges {
        account_id: "u1".to_string(),
        new_state: "s2".to_string(),
        old_state: "s1".to_string(),
        cannot_calculate_changes: false,
        created: vec!["M1".to_string()],
        updated: vec!["M2".to_string(), "M3".to_string()],
        destroyed: vec!["M4".to_string()],
        total: None,
    };

    assert_eq!(
        changes.fetchable_ids(),
        vec!["M1".to_string(), "M2".to_string(), "M3".to_string()]
    );
    assert_eq!(
        changes.affected_ids(),
        vec![
            "M1".to_string(),
            "M2".to_string(),
            "M3".to_string(),
            "M4".to_string()
        ]
    );
}

// The compact form helper keeps the assertions readable; this guards against
// accidental pretty-printing changes in the serialisers.
#[test]
fn method_call_serialises_compactly_without_padding() {
    let call = email_query_call("x", None, None, None, "0");
    let text = compact(&serde_json::to_value(&call).unwrap());
    assert!(!text.contains('\n'));
    assert!(text.starts_with("[\"Email/query\","));
}

/// The RFC 8621 section 4.10 create result the send path accepts: the server
/// assigned an id to the `reply` creation.
fn accepted_create_result() -> Value {
    json!({
        "accountId": "u1",
        "oldState": "1",
        "newState": "2",
        "created": {
            "reply": {
                "id": "M-created",
                "blobId": "G-1",
                "threadId": "T-1",
                "size": 512
            }
        }
    })
}

/// The RFC 8621 section 7.5.1 submission result the send path accepts.
fn accepted_submit_result() -> Value {
    json!({
        "accountId": "u1",
        "oldState": "2",
        "newState": "3",
        "created": { "submission": { "id": "ES-1" } }
    })
}

/// The RFC 8620 section 5.3 update result for the original-message flag patch.
fn accepted_flag_result() -> Value {
    json!({
        "accountId": "u1",
        "oldState": "2",
        "newState": "3",
        "updated": { "M-original": null }
    })
}

/// Parses a whole response envelope from a JSON document.
fn response_from_json(document: &str) -> JMAPResponse {
    serde_json::from_str(document).unwrap()
}

// RFC 8620 section 5.3: a `*/set` call answers 200 while rejecting the record
// in notCreated. send_reply must surface that as an error rather than a success
// with no ids.
#[test]
fn rfc8620_5_3_send_reply_rejects_a_message_the_server_refused_to_create() {
    let outcome = haystack_jmap::client::reply_outcome_from_response(&response_from_json(
        r#"{
          "methodResponses": [
            ["Email/set", {
              "accountId": "u1",
              "oldState": "1",
              "newState": "1",
              "notCreated": {
                "reply": {
                  "type": "invalidProperties",
                  "description": "The bodyStructure is invalid",
                  "properties": ["bodyStructure"]
                }
              }
            }, "create"],
            ["EmailSubmission/set", {
              "accountId": "u1",
              "oldState": "1",
              "newState": "1",
              "created": { "submission": { "id": "ES-1" } }
            }, "submit"]
          ],
          "sessionState": "s-1"
        }"#,
    ));

    let error = outcome.expect_err("a rejected create must not report success");
    let message = error.to_string();
    assert!(message.contains("Email/set"), "error was: {message}");
    assert!(message.contains("reply"), "error was: {message}");
    assert!(
        message.contains("invalidProperties"),
        "error was: {message}"
    );
}

// RFC 8621 section 7.5.1: a submission the server refuses (forbiddenToSend)
// must fail the send even though the draft was created.
#[test]
fn rfc8621_7_5_1_send_reply_rejects_a_message_the_server_refused_to_submit() {
    let outcome = haystack_jmap::client::reply_outcome_from_response(&response_from_json(
        r#"{
          "methodResponses": [
            ["Email/set", {
              "accountId": "u1",
              "oldState": "1",
              "newState": "2",
              "created": {
                "reply": { "id": "M-created", "blobId": "G-1", "threadId": "T-1", "size": 512 }
              }
            }, "create"],
            ["EmailSubmission/set", {
              "accountId": "u1",
              "oldState": "2",
              "newState": "2",
              "notCreated": {
                "submission": {
                  "type": "forbiddenToSend",
                  "description": "Sending is blocked for this account."
                }
              }
            }, "submit"]
          ],
          "sessionState": "s-1"
        }"#,
    ));

    let error = outcome.expect_err("a rejected submission must not report success");
    let message = error.to_string();
    assert!(
        message.contains("EmailSubmission/set"),
        "error was: {message}"
    );
    assert!(message.contains("submission"), "error was: {message}");
    assert!(message.contains("forbiddenToSend"), "error was: {message}");
}

// RFC 8620 section 5.3: the original message is flagged with a separate
// Email/set call; a rejection there fails the send even when the draft and the
// submission succeeded.
#[test]
fn rfc8620_5_3_send_reply_rejects_a_message_the_server_refused_to_update() {
    let outcome = haystack_jmap::client::reply_outcome_from_response(&response_from_json(
        r#"{
          "methodResponses": [
            ["Email/set", {
              "accountId": "u1",
              "oldState": "1",
              "newState": "2",
              "created": {
                "reply": { "id": "M-created", "blobId": "G-1", "threadId": "T-1", "size": 512 }
              }
            }, "create"],
            ["EmailSubmission/set", {
              "accountId": "u1",
              "oldState": "2",
              "newState": "3",
              "created": { "submission": { "id": "ES-1" } }
            }, "submit"],
            ["Email/set", {
              "accountId": "u1",
              "oldState": "3",
              "newState": "3",
              "notUpdated": {
                "M-original": {
                  "type": "notFound",
                  "description": "The message no longer exists"
                }
              }
            }, "flag"]
          ],
          "sessionState": "s-1"
        }"#,
    ));

    let error = outcome.expect_err("a rejected flag update must not report success");
    let message = error.to_string();
    assert!(message.contains("M-original"), "error was: {message}");
    assert!(message.contains("notFound"), "error was: {message}");
}

// The happy path: every method reports the record it applied, so the two ids
// are present and the original flag update is accepted.
#[test]
fn send_reply_returns_the_created_and_submitted_ids_on_success() {
    let outcome =
        haystack_jmap::client::reply_outcome_from_response(&response_from_json(&compact(&json!({
            "methodResponses": [
                ["Email/set", accepted_create_result(), "create"],
                ["EmailSubmission/set", accepted_submit_result(), "submit"],
                ["Email/set", accepted_flag_result(), "flag"]
            ],
            "sessionState": "s-1"
        }))))
        .expect("a fully applied reply should succeed");

    assert_eq!(outcome.sent_email_id.as_deref(), Some("M-created"));
    assert_eq!(outcome.submission_id.as_deref(), Some("ES-1"));
    assert!(outcome.is_complete());
    assert_eq!(
        outcome.created.new_state, "2",
        "the create result is kept for callers"
    );
    assert_eq!(
        outcome.submitted.created_record("submission").unwrap().id,
        "ES-1"
    );
}

// A reply without an original-message id skips the flag call, so only the two
// mandatory methods are read.
#[test]
fn send_reply_without_an_original_skips_the_flag_call() {
    let create = accepted_create_result();
    let submit = accepted_submit_result();
    let outcome =
        haystack_jmap::client::reply_outcome_from_response(&response_from_json(&compact(&json!({
            "methodResponses": [
                ["Email/set", create, "create"],
                ["EmailSubmission/set", submit, "submit"]
            ],
            "sessionState": "s-1"
        }))))
        .expect("a create and a submit without a flag call should succeed");

    assert!(outcome.is_complete());
    assert_eq!(outcome.sent_email_id.as_deref(), Some("M-created"));
}

// A response missing one of the mandatory methods cannot be trusted.
#[test]
fn send_reply_fails_when_the_submit_response_is_missing() {
    let outcome =
        haystack_jmap::client::reply_outcome_from_response(&response_from_json(&compact(&json!({
            "methodResponses": [
                ["Email/set", accepted_create_result(), "create"]
            ],
            "sessionState": "s-1"
        }))));

    let error = outcome.expect_err("a missing submit response must fail");
    assert!(error.to_string().contains("submit"), "error was: {error}");
}

// A response that reports the request as a method-level error cannot be read as
// a success either.
#[test]
fn send_reply_fails_when_the_server_answers_with_a_method_error() {
    let outcome = haystack_jmap::client::reply_outcome_from_response(&response_from_json(
        r#"{
          "methodResponses": [
            ["error", { "type": "serverUnavailable", "description": "Try later" }, "create"]
          ],
          "sessionState": "s-1"
        }"#,
    ));

    let error = outcome.expect_err("a method-level error must fail");
    assert!(
        error.to_string().contains("serverUnavailable"),
        "error was: {error}"
    );
}

// Round 2 P2: send_reply must address Email/set at the mail account and
// EmailSubmission/set at the submission account, which RFC 8620 section 2 lets
// a session report separately.
//
// The client's own builders are private to `send_reply`, so this pins the wire
// form a real session produces: `Session::mail_account_id` and
// `Session::submission_account_id` are read from the session, and the calls
// built from them must address those two accounts.
#[test]
fn send_reply_addresses_each_method_at_its_own_account() {
    let mail_account = "u-mail";
    let submission_account = "u-submission";

    let session: haystack_jmap::Session = serde_json::from_value(json!({
        "primaryAccounts": {
            "urn:ietf:params:jmap:mail": mail_account,
            "urn:ietf:params:jmap:submission": submission_account
        },
        "apiUrl": "https://api.example.com/jmap",
        "capabilities": {
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:submission": {}
        },
        "state": "s-1",
        "username": "me@example.com"
    }))
    .unwrap();

    assert_eq!(session.mail_account_id().unwrap(), mail_account);
    assert_eq!(session.submission_account_id().unwrap(), submission_account);

    let create_call = email_set_create_call(
        session.mail_account_id().unwrap(),
        &[EmailCreate::new_text(
            "reply",
            EmailAddress {
                name: None,
                email: "me@example.com".to_string(),
            },
            vec![EmailAddress {
                name: None,
                email: "them@example.com".to_string(),
            }],
            "Re: Dinner",
            "Sounds good.",
        )],
        "create",
    );
    let submit_call = EmailSubmission::set_call_with_created_ids(
        session.submission_account_id().unwrap(),
        &[EmailSubmission::new("submission", "#reply", "I1")],
        "submit",
    );
    let flag_call = email_set_update_call(
        session.mail_account_id().unwrap(),
        &[EmailPatch::new("M-original").mark_seen().mark_answered()],
        "flag",
    );
    let request = submission_request(vec![create_call, submit_call, flag_call]);

    // A regression here sends Email/set to the submission account, which a
    // conforming server rejects.
    assert_eq!(request.method_calls[0].1["accountId"], json!(mail_account));
    assert_eq!(
        request.method_calls[1].1["accountId"],
        json!(submission_account)
    );
    assert_eq!(request.method_calls[2].1["accountId"], json!(mail_account));

    // The submission's emailId travels as a #creationId reference so the two
    // calls can share one round trip.
    assert_eq!(
        request.method_calls[1].1["create"]["submission"]["emailId"],
        json!("#reply")
    );
}
