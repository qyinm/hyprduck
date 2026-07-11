use super::super::*;

#[test]
fn tool_definitions_expose_agent_session_write_tools_as_mutating_tools() {
    let tools = tool_definitions();
    let tool_by_name = |name: &str| {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))
    };

    for name in [
        "import_source",
        "import_cancel",
        "import_retry_graph",
        "graph_patch_apply",
        "write_propose",
        "write_commit",
        "write_commit_all",
        "write_list",
        "write_reject",
    ] {
        let tool = tool_by_name(name);
        assert_eq!(tool["name"], name);
        assert!(tool["inputSchema"]["properties"]
            .get("workspaceId")
            .is_some());
        if name != "write_list" {
            assert!(tool["annotations"].get("etymaMutationPolicy").is_some());
        }
    }
    assert_eq!(
        tool_by_name("write_propose")["inputSchema"]["required"],
        json!(["contentType", "title", "body", "evidenceRefs"])
    );
    assert_eq!(
        tool_by_name("write_propose")["inputSchema"]["properties"]["contentType"]["enum"],
        json!(WRITE_CONTENT_TYPES)
    );
    assert_eq!(
        tool_by_name("write_propose")["inputSchema"]["properties"]["evidenceRefs"]["minItems"],
        json!(1)
    );
    assert_eq!(
        tool_by_name("write_propose")["inputSchema"]["properties"]["evidenceRefs"]["uniqueItems"],
        json!(true)
    );
    assert_eq!(
        tool_by_name("import_source")["inputSchema"]["required"],
        json!(["sourcePath"])
    );
    assert_eq!(
        tool_by_name("import_source")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("import_source")["annotations"]["idempotentHint"],
        false
    );
    assert_eq!(
        tool_by_name("import_status")["inputSchema"]["required"],
        json!([])
    );
    assert!(tool_by_name("import_status")["inputSchema"]["properties"]
        .get("sourceId")
        .is_some());
    assert_eq!(
        tool_by_name("import_status")["annotations"]["readOnlyHint"],
        true
    );
    assert_eq!(
        tool_by_name("read_graph_history")["inputSchema"]["properties"]["recordKind"]["enum"],
        json!(["node", "relation", "wiki_page"])
    );
    assert!(
        tool_by_name("read_graph_history")["inputSchema"]["properties"]
            .get("recordId")
            .is_some()
    );
    assert!(
        tool_by_name("read_graph_history")["inputSchema"]["properties"]
            .get("wikiPath")
            .is_some()
    );
    assert_eq!(
        tool_by_name("read_graph_history")["inputSchema"]["properties"]["includeDiff"]["type"],
        json!("boolean")
    );
    assert_eq!(
        tool_by_name("import_cancel")["inputSchema"]["required"],
        json!(["jobId"])
    );
    assert_eq!(
        tool_by_name("import_cancel")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("import_retry_graph")["inputSchema"]["required"],
        json!([])
    );
    assert!(
        tool_by_name("import_retry_graph")["inputSchema"]["properties"]
            .get("sourceId")
            .is_some()
    );
    assert_eq!(
        tool_by_name("import_retry_graph")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("graph_patch_apply")["inputSchema"]["required"],
        json!(["graphPatch"])
    );
    assert_eq!(
        tool_by_name("graph_patch_apply")["inputSchema"]["properties"]["graphPatch"]["properties"]
            ["schemaVersion"]["const"],
        json!(etyma_engine_types::GRAPH_PATCH_SCHEMA_VERSION)
    );
    assert_eq!(
        tool_by_name("graph_patch_apply")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("write_commit_all")["inputSchema"]["required"],
        json!(["proposalIds"])
    );
    assert_eq!(
        tool_by_name("write_commit")["inputSchema"]["properties"]["proposalId"]["pattern"],
        json!(PROPOSAL_ID_PATTERN)
    );
    assert_eq!(
        tool_by_name("write_commit")["inputSchema"]["properties"]
            .get("userApproved")
            .and_then(Value::as_object)
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str),
        Some("boolean")
    );
    assert_eq!(
        tool_by_name("write_commit_all")["inputSchema"]["properties"]["proposalIds"]["minItems"],
        json!(1)
    );
    assert_eq!(
        tool_by_name("write_commit_all")["inputSchema"]["properties"]["proposalIds"]["items"]
            ["pattern"],
        json!(PROPOSAL_ID_PATTERN)
    );
    assert_eq!(
        tool_by_name("write_reject")["inputSchema"]["properties"]["proposalId"]["pattern"],
        json!(PROPOSAL_ID_PATTERN)
    );
    assert_eq!(
        tool_by_name("write_propose")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("write_commit")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("write_commit_all")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("write_reject")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        tool_by_name("write_commit")["annotations"]["etymaMutationPolicy"]["replayPolicy"],
        json!("already committed, rejected, or missing proposal IDs fail with proposal_state")
    );
}

#[test]
fn graph_patch_mcp_schema_covers_engine_contract_fields() {
    let tools = tool_definitions();
    let graph_patch_schema = tools
        .iter()
        .find(|tool| tool["name"] == "graph_patch_apply")
        .expect("graph_patch_apply tool")["inputSchema"]["properties"]["graphPatch"]
        .clone();
    let graph_patch_properties = graph_patch_schema["properties"]
        .as_object()
        .expect("graphPatch schema properties");
    for field in [
        "schemaVersion",
        "sourceIds",
        "evidenceRefs",
        "nodes",
        "relations",
        "claims",
        "wikiPages",
        "agentMetadata",
    ] {
        assert!(
            graph_patch_properties.contains_key(field),
            "missing graphPatch schema field {field}"
        );
    }

    let object_item_properties = |field: &str| {
        graph_patch_properties[field]["items"]["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("missing {field} item properties"))
    };
    for field in [
        "nodeId",
        "kind",
        "label",
        "scope",
        "aliases",
        "sourceIds",
        "evidenceIds",
    ] {
        assert!(object_item_properties("nodes").contains_key(field));
    }
    for field in [
        "relationId",
        "kind",
        "sourceNodeId",
        "targetNodeId",
        "label",
        "evidenceIds",
    ] {
        assert!(object_item_properties("relations").contains_key(field));
    }
    for field in [
        "claimId",
        "statement",
        "topicRefs",
        "sourceRefs",
        "evidenceRefs",
        "status",
    ] {
        assert!(object_item_properties("claims").contains_key(field));
    }
    for field in [
        "pageId",
        "path",
        "title",
        "body",
        "nodeRefs",
        "sourceRefs",
        "evidenceRefs",
    ] {
        assert!(object_item_properties("wikiPages").contains_key(field));
    }

    let graph_patch_value = json!({
        "schemaVersion": etyma_engine_types::GRAPH_PATCH_SCHEMA_VERSION,
        "sourceIds": ["source-agent"],
        "evidenceRefs": ["ev-agent-1"],
        "nodes": [{
            "nodeId": "concept-agent",
            "kind": "concept",
            "label": "Agent",
            "scope": "project",
            "aliases": ["Codex"],
            "sourceIds": ["source-agent"],
            "evidenceIds": ["ev-agent-1"]
        }],
        "relations": [{
            "relationId": "rel-agent-source",
            "kind": "mentions",
            "sourceNodeId": "source:source-agent",
            "targetNodeId": "concept-agent",
            "label": "mentions",
            "evidenceIds": ["ev-agent-1"]
        }],
        "claims": [{
            "claimId": "claim-agent",
            "statement": "Agents can submit graph patches.",
            "topicRefs": ["concept-agent"],
            "sourceRefs": ["source-agent"],
            "evidenceRefs": ["ev-agent-1"],
            "status": "agent_generated"
        }],
        "wikiPages": [{
            "pageId": "wiki-agent",
            "path": "wiki/agent.md",
            "title": "Agent",
            "body": "Evidence-backed agent page.",
            "nodeRefs": ["concept-agent"],
            "sourceRefs": ["source-agent"],
            "evidenceRefs": ["ev-agent-1"]
        }],
        "agentMetadata": { "agent": "codex" }
    });
    let graph_patch: etyma_engine_types::GraphPatch =
        serde_json::from_value(graph_patch_value).expect("deserialize graph patch");

    assert_eq!(
        graph_patch.nodes[0].scope,
        Some(etyma_engine_types::BrainScope::Project)
    );
    assert_eq!(graph_patch.relations[0].label, "mentions");
    assert_eq!(graph_patch.claims[0].status, "agent_generated");
    assert_eq!(
        graph_patch.agent_metadata.get("agent"),
        Some(&json!("codex"))
    );
}

#[test]
fn mcp_write_arguments_reject_broad_or_unauditable_inputs() {
    assert!(validate_mcp_write_content_type("memory").is_ok());
    assert!(validate_mcp_write_content_type("wiki_page").is_err());
    assert!(validate_mcp_write_content_type("shell_command").is_err());
    assert!(validate_mcp_write_content_type("../memory").is_err());

    assert!(validate_mcp_proposal_id("prop-0123456789abcdef0123456789ABCDEF").is_ok());
    assert!(validate_mcp_proposal_id("prop-1234").is_err());
    assert!(validate_mcp_proposal_id("../prop-0123456789abcdef0123456789abcdef").is_err());

    let mut arguments = Map::new();
    arguments.insert("evidenceRefs".into(), json!([]));
    let error = required_string_array(&arguments, "evidenceRefs")
        .expect_err("empty evidence refs rejected");
    assert!(error
        .to_string()
        .contains("evidenceRefs must contain at least one item"));
}

#[test]
fn graph_ready_requires_materialized_graph_status() {
    assert!(graph_status_is_ready(Some("rebuilt")));
    assert!(graph_status_is_ready(Some("partially_applied")));
    assert!(!graph_status_is_ready(Some("skipped")));
    assert!(!graph_status_is_ready(Some("empty")));
    assert!(!graph_status_is_ready(Some("failed")));
    assert!(!graph_status_is_ready(Some("failed_no_materialization")));
    assert!(!graph_status_is_ready(None));
}
