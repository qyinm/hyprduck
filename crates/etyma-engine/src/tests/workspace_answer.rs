use super::common::*;
use super::*;

#[test]
fn exact_project_load_uses_project_workspace_sources() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nWorkspace A evidence stays separate.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, mut manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nWorkspace B evidence is newer.\n",
        "source-b",
        "beta",
        99,
    );
    manifest_b.workspace_id = "workspace-b".into();
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save workspace a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save workspace b project");

    let previous_store = std::env::var_os("ETYMA_PROJECT_STORE");
    std::env::set_var("ETYMA_PROJECT_STORE", &store_path);
    let response = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: None,
    })
    .expect("load exact project");
    match previous_store {
        Some(value) => std::env::set_var("ETYMA_PROJECT_STORE", value),
        None => std::env::remove_var("ETYMA_PROJECT_STORE"),
    }

    assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
    assert_eq!(response.sources.len(), 1);
    assert_eq!(response.sources[0].source_id, "source-a");
    assert_eq!(
        response.project.expect("exact project").summary.project_id,
        project_a.summary.project_id
    );

    let previous_store = std::env::var_os("ETYMA_PROJECT_STORE");
    std::env::set_var("ETYMA_PROJECT_STORE", &store_path);
    let error = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: Some("workspace-b".into()),
    })
    .expect_err("stale workspace should not hydrate exact project");
    match previous_store {
        Some(value) => std::env::set_var("ETYMA_PROJECT_STORE", value),
        None => std::env::remove_var("ETYMA_PROJECT_STORE"),
    }
    assert!(error
        .to_string()
        .contains("belongs to workspace default, not workspace-b"));
}

#[test]
fn answer_project_supports_workspace_project_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project, manifest) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\n",
        "source-a",
        "alpha",
        10,
    );
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What does the shared context layer say?".into(),
        },
    )
    .expect("answer workspace project");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
}

#[test]
fn answer_empty_workspace_project_blocks_instead_of_error() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load empty workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What remains in the graph?".into(),
        },
    )
    .expect("answer empty workspace");

    assert_eq!(aggregate.summary.project_id, "workspace:default");
    assert_eq!(answer.status, AnswerStatus::Blocked);
    assert!(answer.text.is_none());
    assert!(answer.explanation.contains("No graph nodes"));
}

#[test]
fn workspace_answer_without_node_uses_matching_source_evidence() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What does the beta architecture context say?".into(),
        },
    )
    .expect("answer workspace project");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
    assert!(!answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
}

#[test]
fn materialized_workspace_rag_treats_selected_node_as_bias_only() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context says the release checklist owns quality gates.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context says the retry worker owns recovery.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    assert!(temp.path().join("default/brain-manifest.json").exists());
    assert!(temp.path().join("default/graph/evidence.json").exists());

    let previous_store = std::env::var_os("ETYMA_PROJECT_STORE");
    std::env::set_var("ETYMA_PROJECT_STORE", &store_path);
    let selected_bias_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("source:source-b".into()),
        question: "What does the architecture context say?".into(),
    })
    .expect("answer with selected source bias")
    .answer;
    let irrelevant_selection_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("source:source-b".into()),
        question: "What does the alpha planning context say?".into(),
    })
    .expect("answer across workspace despite irrelevant selected source")
    .answer;
    let missing_selection_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("concept-stale-selection".into()),
        question: "What does the beta retry worker own?".into(),
    })
    .expect("answer across workspace despite missing selected node")
    .answer;
    match previous_store {
        Some(value) => std::env::set_var("ETYMA_PROJECT_STORE", value),
        None => std::env::remove_var("ETYMA_PROJECT_STORE"),
    }

    assert_eq!(selected_bias_answer.status, AnswerStatus::Grounded);
    let selected_text = selected_bias_answer
        .text
        .as_deref()
        .expect("selected answer text");
    assert!(selected_text.contains("- Beta architecture context says"));
    assert!(!selected_text.contains("Best support:"));
    assert!(!selected_text.contains("strongest workspace match"));
    assert_eq!(
        selected_bias_answer
            .citations
            .first()
            .and_then(|citation| citation.source_id.as_deref()),
        Some("source-b")
    );
    assert!(irrelevant_selection_answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
    assert_eq!(
        irrelevant_selection_answer
            .citations
            .first()
            .and_then(|citation| citation.source_id.as_deref()),
        Some("source-a")
    );
    assert_ne!(missing_selection_answer.status, AnswerStatus::Blocked);
    assert!(missing_selection_answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
}

#[test]
fn workspace_answer_with_missing_selected_node_falls_back_to_question_match() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: Some("concept-stale-selection".into()),
            question: "What does the beta architecture context say?".into(),
        },
    )
    .expect("answer workspace project despite stale selected node");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
}

#[test]
fn answer_project_blocks_empty_question() {
    let temp = tempfile::tempdir().expect("temp dir");
    let project = compile_fixture_project(
        &temp,
        "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\n",
    );

    let answer = answer_project(
        &project,
        &AnswerProjectRequest {
            project_id: project.summary.project_id.clone(),
            node_id: None,
            question: "   ".into(),
        },
    )
    .expect("answer project");

    assert_eq!(answer.status, AnswerStatus::Blocked);
    assert!(answer.citations.is_empty());
}
