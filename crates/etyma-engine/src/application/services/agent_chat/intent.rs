use etyma_engine_types::{AgentChatAskRequest, AgentChatScopeMode};

use crate::domains::retrieval::brain_search::db_search_terms;

pub(super) fn should_answer_as_general_chat(request: &AgentChatAskRequest) -> bool {
    is_general_chat_question(&request.question)
}

pub(super) fn should_retrieve_context(request: &AgentChatAskRequest, general_intent: bool) -> bool {
    !general_intent
        && (matches!(
            request.mode,
            AgentChatScopeMode::Auto
                | AgentChatScopeMode::AllDocs
                | AgentChatScopeMode::SelectedSource
                | AgentChatScopeMode::GraphContext
        ) || looks_like_evidence_question(&request.question))
}

pub(super) fn should_reuse_previous_topic_for_context(question: &str) -> bool {
    let terms = db_search_terms(question);
    terms.is_empty()
        || (terms.len() <= 1 && looks_like_evidence_question(question))
        || terms.iter().all(|term| is_generic_evidence_term(term))
}

pub(super) fn is_generic_evidence_term(term: &str) -> bool {
    matches!(
        term,
        "document"
            | "documents"
            | "doc"
            | "docs"
            | "source"
            | "sources"
            | "citation"
            | "citations"
            | "evidence"
            | "graph"
            | "node"
            | "context"
            | "pdf"
            | "docx"
            | "file"
            | "files"
            | "paper"
            | "papers"
            | "article"
            | "articles"
            | "research"
            | "page"
            | "pages"
            | "summarize"
            | "summary"
            | "문서"
            | "자료"
            | "파일"
            | "논문"
            | "연구"
            | "출처"
            | "근거"
            | "인용"
            | "그래프"
            | "노드"
            | "페이지"
            | "요약"
            | "정리"
    )
}

pub(super) fn looks_like_evidence_question(question: &str) -> bool {
    let normalized = question.trim().to_lowercase();
    let keywords = [
        "document",
        "documents",
        "doc",
        "docs",
        "source",
        "sources",
        "citation",
        "citations",
        "evidence",
        "graph",
        "node",
        "context",
        "pdf",
        "docx",
        "file",
        "files",
        "paper",
        "papers",
        "article",
        "articles",
        "research",
        "page",
        "pages",
        "summarize",
        "summary",
        "문서",
        "자료",
        "파일",
        "논문",
        "연구",
        "출처",
        "근거",
        "인용",
        "그래프",
        "노드",
        "페이지",
        "요약",
        "정리",
    ];
    keywords.iter().any(|keyword| normalized.contains(keyword))
}

pub(super) fn is_general_chat_question(question: &str) -> bool {
    let normalized = question
        .trim()
        .trim_matches(|ch: char| {
            ch.is_ascii_punctuation()
                || matches!(
                    ch,
                    '。' | '，' | '、' | '！' | '？' | '…' | '·' | 'ㅋ' | 'ㅎ'
                )
        })
        .to_lowercase();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(
        compact.as_str(),
        "hi" | "hello"
            | "hey"
            | "yo"
            | "good morning"
            | "good afternoon"
            | "good evening"
            | "thanks"
            | "thank you"
            | "안녕"
            | "안녕하세요"
            | "하이"
            | "고마워"
            | "고맙습니다"
            | "감사합니다"
            | "반가워"
            | "반갑습니다"
            | "뭐 할 수 있어"
            | "무엇을 할 수 있어"
            | "what can you do"
    )
}
