//! Query merging for knowledge graph integration
//!
//! Merges learning documents with regular graph query results.

use terraphim_rolegraph::RoleGraph;
use terraphim_types::{IndexedDocument, SearchQuery};

/// Query the graph including learning documents.
///
/// Returns all documents (regular + learning) whose nodes match the query,
/// sorted by rank.
///
/// When `query.include_pinned` is `true`, the search is routed through
/// `query_graph_with_trigger_fallback` which activates the TF-IDF trigger
/// index and always includes pinned nodes in the result set. This allows
/// documents linked to pinned nodes to surface even when the query term
/// has no Aho-Corasick match in the thesaurus.
///
/// When `query.include_pinned` is `false`, the search uses the standard
/// `query_graph` path which relies solely on Aho-Corasick matching.
pub fn query_with_learnings(
    graph: &RoleGraph,
    query: &SearchQuery,
    include_learnings: bool,
) -> Vec<IndexedDocument> {
    let query_text = query.search_term.as_str();
    let mut merged = std::collections::HashMap::new();

    let graph_results = if query.include_pinned {
        graph.query_graph_with_trigger_fallback(query_text, query.skip, query.limit, true)
    } else {
        graph.query_graph(query_text, query.skip, query.limit)
    };

    if let Ok(results) = graph_results {
        for (id, doc) in results {
            merged.insert(id, doc);
        }
    }

    if include_learnings {
        for doc in graph.get_learning_documents(query_text) {
            merged
                .entry(doc.id.clone())
                .and_modify(|existing: &mut IndexedDocument| {
                    if doc.rank > existing.rank {
                        *existing = doc.clone();
                    }
                })
                .or_insert_with(|| doc.clone());
        }
    }

    let mut results: Vec<_> = merged.into_values().collect();
    // Sort by rank descending
    results.sort_by_key(|b| std::cmp::Reverse(b.rank));

    results
}
