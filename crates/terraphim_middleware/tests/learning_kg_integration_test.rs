#![cfg(all(feature = "kg-integration", feature = "feedback-loop"))]

//! Integration tests for knowledge graph learning integration

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use ahash::AHashMap;
use terraphim_middleware::feedback_loop::{
    record_graph_query, record_learning_application, FeedbackConfig, GraphTouchStore,
};
use terraphim_middleware::learning_indexer::{index_learning, LearningIndexerConfig};
use terraphim_middleware::learning_query::query_with_learnings;
use terraphim_rolegraph::RoleGraph;
use terraphim_types::shared_learning::{LearningSource, SharedLearning, TrustLevel};
use terraphim_types::Document;
use terraphim_types::{
    Layer, NormalizedTerm, NormalizedTermValue, RoleName, SearchQuery, Thesaurus,
};

#[derive(Clone, Default)]
struct MockTouchStore {
    touched: Arc<Mutex<Vec<String>>>,
}

impl MockTouchStore {
    fn touched_ids(&self) -> Vec<String> {
        self.touched.lock().unwrap().clone()
    }
}

impl GraphTouchStore for MockTouchStore {
    fn record_graph_touch<'a>(
        &'a self,
        learning_id: &'a str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<(), terraphim_types::shared_learning::StoreError>>
                + Send
                + 'a,
        >,
    > {
        let touched = self.touched.clone();
        let id = learning_id.to_string();
        Box::pin(async move {
            touched.lock().unwrap().push(id);
            Ok(())
        })
    }
}

fn create_test_graph() -> RoleGraph {
    let mut thesaurus = Thesaurus::new("test".to_string());
    thesaurus.insert(
        NormalizedTermValue::from("rust"),
        NormalizedTerm::with_auto_id(NormalizedTermValue::from("rust programming language")),
    );
    thesaurus.insert(
        NormalizedTermValue::from("error"),
        NormalizedTerm::with_auto_id(NormalizedTermValue::from("error handling")),
    );

    RoleGraph::new_sync(RoleName::new("test"), thesaurus).unwrap()
}

fn create_test_learning(
    title: &str,
    keywords: Vec<&str>,
    trust_level: TrustLevel,
) -> SharedLearning {
    let mut learning = SharedLearning::new(
        title.to_string(),
        format!("Content for {}", title),
        LearningSource::Manual,
        "test-agent".to_string(),
    );
    learning.keywords = keywords.iter().map(|k| k.to_string()).collect();
    learning.trust_level = trust_level;
    learning
}

fn create_query(term: &str) -> SearchQuery {
    SearchQuery {
        search_term: NormalizedTermValue::from(term),
        search_terms: None,
        operator: None,
        skip: None,
        limit: Some(10),
        role: Some(RoleName::new("test")),
        layer: Layer::default(),
        include_pinned: false,
        min_quality: None,
    }
}

fn create_query_with_pinned(term: &str, include_pinned: bool) -> SearchQuery {
    SearchQuery {
        search_term: NormalizedTermValue::from(term),
        search_terms: None,
        operator: None,
        skip: None,
        limit: Some(10),
        role: Some(RoleName::new("test")),
        layer: Layer::default(),
        include_pinned,
        min_quality: None,
    }
}

/// Create a graph where a known node ID can be pinned.
///
/// Uses a fixed node ID (1001) for the "rust" thesaurus entry so the test can
/// pin that node via `load_trigger_index` and verify the trigger fallback path.
fn create_graph_with_known_node_id() -> (RoleGraph, u64) {
    let pinned_node_id: u64 = 1001;
    let mut thesaurus = Thesaurus::new("test-pinned".to_string());
    thesaurus.insert(
        NormalizedTermValue::from("rust"),
        NormalizedTerm::new(
            pinned_node_id,
            NormalizedTermValue::from("rust programming language"),
        ),
    );
    let graph = RoleGraph::new_sync(RoleName::new("test-pinned"), thesaurus).unwrap();
    (graph, pinned_node_id)
}

#[test]
fn test_learning_indexing() {
    let mut graph = create_test_graph();
    let learning =
        create_test_learning("Rust Error Handling", vec!["rust", "error"], TrustLevel::L2);

    let config = LearningIndexerConfig::default();
    let result = index_learning(&mut graph, &learning, &config);

    assert!(result.is_ok());
    let indexed = result.unwrap();
    assert_eq!(indexed.id, learning.id);
    assert!(indexed.rank > 0);
}

#[test]
fn test_l1_learning_filtered_by_default() {
    let mut graph = create_test_graph();
    let learning = create_test_learning("Rust Basics", vec!["rust"], TrustLevel::L1);

    let config = LearningIndexerConfig::default();
    let result = index_learning(&mut graph, &learning, &config);

    assert!(result.is_err());
}

#[test]
fn test_learning_appears_in_query() {
    let mut graph = create_test_graph();
    let learning =
        create_test_learning("Rust Error Patterns", vec!["rust", "error"], TrustLevel::L2);

    let config = LearningIndexerConfig::default();
    index_learning(&mut graph, &learning, &config).unwrap();

    let query = create_query("rust");

    let results = query_with_learnings(&graph, &query, true);

    assert!(!results.is_empty());
    assert!(results.iter().any(|doc| doc.id == learning.id));
}

#[test]
fn test_learning_ranked_by_quality() {
    let mut graph = create_test_graph();

    let mut low_quality =
        create_test_learning("Low Quality Learning", vec!["rust"], TrustLevel::L2);
    low_quality.quality.success_rate = Some(0.3);

    let mut high_quality =
        create_test_learning("High Quality Learning", vec!["rust"], TrustLevel::L2);
    high_quality.quality.success_rate = Some(0.9);

    let config = LearningIndexerConfig::default();
    index_learning(&mut graph, &low_quality, &config).unwrap();
    index_learning(&mut graph, &high_quality, &config).unwrap();

    let query = create_query("rust");

    let results = query_with_learnings(&graph, &query, true);

    assert_eq!(results.len(), 2);
    // High quality should be first (higher rank)
    assert!(results[0].rank >= results[1].rank);
}

#[test]
fn test_query_with_learnings_preserves_regular_documents() {
    let mut graph = create_test_graph();
    let learning = create_test_learning("Rust Learning", vec!["rust"], TrustLevel::L2);
    index_learning(&mut graph, &learning, &LearningIndexerConfig::default()).unwrap();

    let regular = Document {
        id: "regular-rust-doc".to_string(),
        url: "file:///tmp/rust.md".to_string(),
        title: "Rust Guide".to_string(),
        body: "rust error handling guide".to_string(),
        description: None,
        summarization: None,
        stub: None,
        tags: Some(vec!["rust".to_string(), "error".to_string()]),
        rank: Some(50),
        source_haystack: None,
        doc_type: terraphim_types::DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    };
    let regular_id = regular.id.clone();
    graph.insert_document(&regular_id, regular);

    let query = create_query("rust");
    let results = query_with_learnings(&graph, &query, true);
    assert!(results.iter().any(|doc| doc.id == learning.id));
    assert!(results.iter().any(|doc| doc.id == "regular-rust-doc"));

    let results_without_learnings = query_with_learnings(&graph, &query, false);
    assert!(results_without_learnings
        .iter()
        .all(|doc| doc.id != learning.id));
    assert!(results_without_learnings
        .iter()
        .any(|doc| doc.id == "regular-rust-doc"));
}

#[test]
fn test_feedback_loop_only_touches_learning_documents_and_updates_rank() {
    let mut graph = create_test_graph();
    let learning = create_test_learning("Rust Learning", vec!["rust"], TrustLevel::L2);
    let indexed = index_learning(&mut graph, &learning, &LearningIndexerConfig::default()).unwrap();

    let regular = Document {
        id: "regular-rust-doc".to_string(),
        url: "file:///tmp/rust.md".to_string(),
        title: "Rust Guide".to_string(),
        body: "rust error book".to_string(),
        description: None,
        summarization: None,
        stub: None,
        tags: Some(vec!["rust".to_string(), "error".to_string()]),
        rank: Some(20),
        source_haystack: None,
        doc_type: terraphim_types::DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    };
    let regular_id = regular.id.clone();
    graph.insert_document(&regular_id, regular);

    let store = MockTouchStore::default();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(record_graph_query(
            &graph,
            &store,
            "rust",
            &FeedbackConfig::default(),
        ))
        .unwrap();

    assert_eq!(store.touched_ids(), vec![learning.id.clone()]);

    let original_rank = graph.get_document(&indexed.id).unwrap().rank;
    runtime
        .block_on(record_learning_application(
            &mut graph,
            &store,
            &indexed.id,
            true,
            &FeedbackConfig::default(),
        ))
        .unwrap();
    let boosted_rank = graph.get_document(&indexed.id).unwrap().rank;
    assert!(boosted_rank > original_rank);
}

/// Verify that when `include_pinned` is true, `query_with_learnings` routes through
/// `query_graph_with_trigger_fallback` so that pinned nodes surface documents even
/// when the query text has no Aho-Corasick match.
///
/// Setup:
///   1. A thesaurus has "rust" mapped to a known node ID.
///   2. A document containing the word "rust" is indexed — it becomes linked to that node.
///   3. `load_trigger_index` marks that node ID as pinned.
///   4. A query using a nonsense term ("xyzzy_no_match") produces no Aho-Corasick hits.
///
/// Expected behaviour:
///   - `include_pinned = false`: 0 results (no AC match, pinned not consulted).
///   - `include_pinned = true`: the "rust" document is returned via the pinned node.
#[test]
fn test_include_pinned_routes_through_trigger_fallback() {
    let (mut graph, pinned_node_id) = create_graph_with_known_node_id();

    // Index a document that the Aho-Corasick automata will link to node `pinned_node_id`
    let doc = Document {
        id: "pinned-rust-guide".to_string(),
        url: "file:///tmp/pinned-rust-guide.md".to_string(),
        title: "Pinned Rust Guide".to_string(),
        body: "rust programming language essentials".to_string(),
        description: None,
        summarization: None,
        stub: None,
        tags: Some(vec!["rust".to_string()]),
        rank: Some(50),
        source_haystack: None,
        doc_type: terraphim_types::DocumentType::KgEntry,
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    };
    graph.insert_document(&doc.id.clone(), doc);

    // Mark the node as pinned so it is always included when include_pinned=true
    graph.load_trigger_index(AHashMap::new(), vec![pinned_node_id], 0.3);

    // A query that has no Aho-Corasick match — only pinned nodes can surface results
    let query_without_pinned = create_query_with_pinned("xyzzy_no_match", false);
    let query_with_pinned = create_query_with_pinned("xyzzy_no_match", true);

    let results_without_pinned = query_with_learnings(&graph, &query_without_pinned, false);
    assert!(
        results_without_pinned.is_empty(),
        "Expected no results when include_pinned=false and query has no AC match, \
         got {} result(s)",
        results_without_pinned.len()
    );

    let results_with_pinned = query_with_learnings(&graph, &query_with_pinned, false);
    assert!(
        !results_with_pinned.is_empty(),
        "Expected the pinned document to appear when include_pinned=true, \
         but got no results. The include_pinned flag is not wired through to \
         query_graph_with_trigger_fallback."
    );
    assert!(
        results_with_pinned
            .iter()
            .any(|d| d.id == "pinned-rust-guide"),
        "Expected 'pinned-rust-guide' in results, got: {:?}",
        results_with_pinned
            .iter()
            .map(|d| &d.id)
            .collect::<Vec<_>>()
    );
}
