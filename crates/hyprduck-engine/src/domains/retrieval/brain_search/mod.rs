//! Query-time brain search / hybrid retrieval domain logic (evidence + graph + wiki).
//! Policy (intent, scoring, stage combination) lives here in the retrieval domain.
//! Heavy data access (specific FTS queries, graph expansion SQL/Cypher) still reaches
//! into persistence adapters for the raw mechanics.

mod fts_hybrid;
mod graph_expand;
mod scoring;
mod text_normalize;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct HybridRetrievalHit {
    pub(crate) evidence_id: String,
    pub(crate) source_id: String,
    pub(crate) evidence_type: String,
    pub(crate) snippet: String,
    pub(crate) quoted_text: Option<String>,
    pub(crate) lexical_rank: f64,
    pub(crate) graph_neighbor_count: i64,
    pub(crate) score: f64,
}

// Required stable re-exports for external in-crate callers.
pub(crate) use fts_hybrid::{hybrid_retrieve_from_db, search_brain_from_db};
pub(crate) use scoring::{db_context_window, EvidenceQueryIntent};
pub(crate) use text_normalize::db_search_terms;

// Preserve prior pub(crate) surface from the pre-split module.
#[allow(unused_imports)]
pub(crate) use fts_hybrid::{
    append_source_metadata_hits, append_source_page_fts_hits, append_wiki_fts_hits,
};
#[allow(unused_imports)]
pub(crate) use graph_expand::{append_graph_neighbor_hits, evidence_graph_neighbor_counts};
#[allow(unused_imports)]
pub(crate) use scoring::{db_best_snippet, db_float_score, db_match_score};
#[allow(unused_imports)]
pub(crate) use text_normalize::fts_phrase_query;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_window_prefers_dynamic_hashing_section_over_toc() {
        let text = "Hashing\nChapter 8\nContents\n8.1 Introduction\n8.2 Static Hashing\n8.3 Dynamic Hashing\n\nRemind: Dictionaries\nCollection of pairs. Operations include Search, Delete, and Insert.\n\nStatic hashing\nStatic hashing identifiers are stored in a fixed size hash table.\n\nDynamic Hashing\nDynamic hashing using directories grows and shrinks a directory of bucket pointers. A bucket split redistributes records using additional hash bits.";
        let terms = db_search_terms("dynamic hashing 내용 설명해");

        let window = db_context_window(text, &terms, 700);

        assert!(window.contains("Dynamic hashing using directories"));
        assert!(window.contains("bucket split redistributes records"));
        assert!(!window.starts_with("Hashing\nChapter 8\nContents"));
    }

    #[test]
    fn context_window_prefers_static_hashing_section_over_toc() {
        let text = "Hashing\nChapter 8\nContents\n8.1 Introduction\n8.2 Static Hashing\n8.3 Dynamic Hashing\n\nRemind: Dictionaries\nCollection of pairs. Operations include Search, Delete, and Insert.\n\nStatic hashing\nStatic hashing identifiers are stored in a fixed size hash table and collision chains handle overflow.\n\nDynamic Hashing\nDynamic hashing using directories grows and shrinks a directory of bucket pointers.";
        let terms = db_search_terms("Static Hashing 에 대해서 알려줘");

        let window = db_context_window(text, &terms, 700);

        assert!(window.contains("fixed size hash table"));
        assert!(window.contains("collision chains handle overflow"));
        assert!(!window.starts_with("Hashing\nChapter 8\nContents"));
    }

    #[test]
    fn context_window_handles_multibyte_text_when_backing_up_from_match() {
        let prefix = "가".repeat(90);
        let text = format!(
            "{prefix}x needle section explains 입력 문서는 먼저 점검 단계를 거쳐 텍스트 레이어 상태를 분석한다."
        );
        let terms = vec!["needle".to_string()];

        let window = db_context_window(&text, &terms, 240);

        assert!(window.contains("needle section"));
        assert!(window.contains("입력 문서는"));
    }
}
