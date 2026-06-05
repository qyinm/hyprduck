#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationDecision {
    Automatic,
    RequiresApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolAutomationPolicy {
    pub(crate) decision: AutomationDecision,
    pub(crate) destructive: bool,
    pub(crate) reason: &'static str,
}

pub(crate) const AUTOMATIC_MCP_TOOLS: &[&str] = &[
    "import_source",
    "import_status",
    "import_cancel",
    "import_retry_graph",
    "get_context_pack",
    "read_context_pack",
    "search_documents",
    "search_brain",
    "read_source",
    "read_page_evidence",
    "read_wiki_page",
    "read_node",
    "read_recent_events",
    "read_graph_history",
    "read_graph_snapshot",
    "read_health",
    "graph_patch_apply",
    "write_propose",
    "write_commit",
    "write_commit_all",
    "write_list",
];

pub(crate) fn policy_for_mcp_tool(tool_name: &str) -> ToolAutomationPolicy {
    let name = normalize_hyprduck_tool_name(tool_name);
    match name.as_deref() {
        Some("write_reject") => ToolAutomationPolicy {
            decision: AutomationDecision::RequiresApproval,
            destructive: true,
            reason: "rejecting or removing HyprDuck write state requires approval",
        },
        Some(tool) if AUTOMATIC_MCP_TOOLS.contains(&tool) => ToolAutomationPolicy {
            decision: AutomationDecision::Automatic,
            destructive: false,
            reason: "known HyprDuck action covered by existing MCP scope, evidence, and audit policy",
        },
        _ => ToolAutomationPolicy {
            decision: AutomationDecision::RequiresApproval,
            destructive: false,
            reason: "unknown HyprDuck action is not eligible for automatic execution",
        },
    }
}

pub(crate) fn normalize_hyprduck_tool_name(tool_name: &str) -> Option<String> {
    if tool_name.contains("__hyprduck__") {
        return tool_name
            .rsplit("__")
            .next()
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned);
    }
    if tool_name.starts_with("mcp__") {
        return None;
    }
    Some(tool_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::{policy_for_mcp_tool, AutomationDecision};

    #[test]
    fn known_read_tools_are_automatic() {
        let policy = policy_for_mcp_tool("get_context_pack");
        assert_eq!(policy.decision, AutomationDecision::Automatic);
        assert!(!policy.destructive);
    }

    #[test]
    fn known_additive_write_tools_are_automatic() {
        for tool in ["graph_patch_apply", "write_propose", "write_commit"] {
            let policy = policy_for_mcp_tool(tool);
            assert_eq!(policy.decision, AutomationDecision::Automatic, "{tool}");
            assert!(!policy.destructive, "{tool}");
        }
    }

    #[test]
    fn reject_and_unknown_tools_require_approval() {
        let reject = policy_for_mcp_tool("write_reject");
        assert_eq!(reject.decision, AutomationDecision::RequiresApproval);
        assert!(reject.destructive);

        let unknown = policy_for_mcp_tool("future_delete_all_sources");
        assert_eq!(unknown.decision, AutomationDecision::RequiresApproval);
    }

    #[test]
    fn codex_mcp_tool_names_are_normalized() {
        let policy = policy_for_mcp_tool("mcp__hyprduck__read_health");
        assert_eq!(policy.decision, AutomationDecision::Automatic);

        let other = policy_for_mcp_tool("mcp__filesystem__read_file");
        assert_eq!(other.decision, AutomationDecision::RequiresApproval);
    }
}
