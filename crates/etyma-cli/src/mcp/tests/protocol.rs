use super::super::*;
use super::*;

#[test]
fn mcp_error_classifier_returns_stable_categories() {
    assert_eq!(
        classify_mcp_error("rootDir is disabled unless ETYMA_MCP_ALLOW_ROOT_DIR=1"),
        "path_policy"
    );
    assert_eq!(
        classify_mcp_error("graphPatch references unknown or out-of-scope evidence ref ev-1"),
        "evidence_scope"
    );
    assert_eq!(
        classify_mcp_error("argument graphPatch does not match Etyma graph patch schema"),
        "schema"
    );
    assert_eq!(
        classify_mcp_error("OpenRouter API key is missing"),
        "provider"
    );
    assert_eq!(
        classify_mcp_error("failed writing graph materialization snapshot"),
        "graph_materialization"
    );
    assert_eq!(
        classify_mcp_error("import job not found after graph retry"),
        "lifecycle"
    );
    assert_eq!(
        classify_mcp_error("GraphQLite failed to open knowledge DB"),
        "persistence"
    );
}

#[test]
fn redacts_local_paths_embedded_in_markdown_text() {
    let text = "Plain /Users/hippoo/file.md, link [doc](/Users/hippoo/doc.pdf), code `/tmp/raw.txt`, file URL file:///Users/hippoo/source.pdf and windows C:\\Users\\hippoo\\note.txt";
    let redacted = redact_local_path_text(text);

    assert!(!redacted.contains("/Users/hippoo"));
    assert!(!redacted.contains("/tmp/raw.txt"));
    assert!(!redacted.contains("file:///"));
    assert!(!redacted.contains("C:\\Users\\hippoo"));
    assert_eq!(redacted.matches("[redacted-local-path]").count(), 5);
    assert!(redacted.contains("[doc]([redacted-local-path])"));
    assert!(redacted.contains("`[redacted-local-path]`"));
    assert_eq!(
        redact_local_path_text("relative state/latest-readable-snapshot.json stays"),
        "relative state/latest-readable-snapshot.json stays"
    );
}

#[test]
fn include_local_paths_requires_server_opt_in_and_supported_tool() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    clear_root_dir_env();

    let mut arguments = Map::new();
    arguments.insert("includeLocalPaths".into(), Value::Bool(true));
    let error = local_path_disclosure_for_tool("read_source", &arguments)
        .expect_err("server opt-in required");
    assert!(error
        .to_string()
        .contains("ETYMA_MCP_ALLOW_LOCAL_PATHS=1"));

    std::env::set_var(LOCAL_PATH_DISCLOSURE_ENV, "1");
    let unsupported = local_path_disclosure_for_tool("search_documents", &arguments)
        .expect_err("unsupported tool rejects local path disclosure");
    assert!(unsupported
        .to_string()
        .contains("includeLocalPaths is not supported"));

    assert!(local_path_disclosure_for_tool("read_source", &arguments)
        .expect("supported tool can disclose paths when server opted in"));

    clear_root_dir_env();
}
