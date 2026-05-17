use anyhow::{anyhow, Result};
use hyprduck_engine_types::{
    BrainProposalKind, BrainRelationKind, ReadGraphHistoryRequest, ReadRecentEventsRequest,
    ReconstructBrainRequest,
};

#[derive(Debug)]
pub struct Cli {
    pub command: Option<Commands>,
}

impl Cli {
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let command = match args.next().as_deref() {
            None | Some("tui") => None,
            Some("doctor") => Some(Commands::Doctor),
            Some("serve") => Some(Commands::Serve),
            Some("mcp") => {
                let subcommand = args.next().ok_or_else(|| {
                    anyhow!("usage: hyprduck mcp <serve|install claude-code|install codex>")
                })?;
                match subcommand.as_str() {
                    "serve" => Some(Commands::Mcp {
                        command: McpCommand::Serve,
                    }),
                    "install" => {
                        let target = args
                            .next()
                            .ok_or_else(|| anyhow!("usage: hyprduck mcp install claude-code"))?;
                        match target.as_str() {
                            "claude-code" => Some(Commands::Mcp {
                                command: McpCommand::InstallClaudeCode,
                            }),
                            "codex" => Some(Commands::Mcp {
                                command: McpCommand::InstallCodex,
                            }),
                            _ => return Err(anyhow!("unknown mcp install target: {target}")),
                        }
                    }
                    _ => return Err(anyhow!("unknown mcp subcommand: {subcommand}")),
                }
            }
            Some("parse") => {
                let input = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: hyprduck parse <input>"))?;
                Some(Commands::Parse { input })
            }
            Some("engines") => {
                let subcommand = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: hyprduck engines list"))?;
                match subcommand.as_str() {
                    "list" => Some(Commands::Engines {
                        command: EnginesCommand::List,
                    }),
                    _ => return Err(anyhow!("unknown engines subcommand: {subcommand}")),
                }
            }
            Some("eval") => {
                let subcommand = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: hyprduck eval golden-corpus [--fixtures <path>] [--mode source-evidence|hosted|local|all]"))?;
                Some(Commands::Eval {
                    command: parse_eval_command(subcommand, args.collect())?,
                })
            }
            Some("brain") => {
                let subcommand = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: hyprduck brain <search|context-pack|event-history|graph-history|inspect-state|rollback-state|propose-memory|propose-claim|propose-link|propose-wiki-page|append-observation|add-source-note>"))?;
                Some(Commands::Brain {
                    command: parse_brain_command(subcommand, args.collect())?,
                })
            }
            Some(other) => return Err(anyhow!("unknown command: {other}")),
        };

        Ok(Self { command })
    }
}

fn parse_eval_command(subcommand: String, args: Vec<String>) -> Result<EvalCommand> {
    match subcommand.as_str() {
        "golden-corpus" => {
            let mut fixtures = None;
            let mut mode = "source-evidence".to_string();
            let mut index = 0usize;
            while index < args.len() {
                match args[index].as_str() {
                    "--fixtures" => {
                        index += 1;
                        fixtures = Some(
                            args.get(index)
                                .cloned()
                                .ok_or_else(|| anyhow!("--fixtures needs a value"))?,
                        );
                    }
                    "--mode" => {
                        index += 1;
                        mode = args
                            .get(index)
                            .cloned()
                            .ok_or_else(|| anyhow!("--mode needs a value"))?;
                    }
                    value => return Err(anyhow!("unknown golden-corpus option: {value}")),
                }
                index += 1;
            }
            Ok(EvalCommand::GoldenCorpus { fixtures, mode })
        }
        _ => Err(anyhow!("unknown eval subcommand: {subcommand}")),
    }
}

fn parse_brain_command(subcommand: String, args: Vec<String>) -> Result<BrainCommand> {
    let mut workspace = "default".to_string();
    let mut root_dir = None;
    let mut budget = None;
    let mut limit = None;
    let mut actor = None;
    let mut target_node_id = None;
    let mut target_source_id = None;
    let mut run_id = None;
    let mut source_ref = None;
    let mut event_node_id = None;
    let mut edge_id = None;
    let mut claim_id = None;
    let mut memory_id = None;
    let mut change_type = None;
    let mut snapshot_id = None;
    let mut event_id = None;
    let mut relation_kind = None;
    let mut source_refs = Vec::new();
    let mut node_refs = Vec::new();
    let mut evidence_refs = Vec::new();
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| anyhow!("--workspace needs a value"))?;
            }
            "--root" => {
                index += 1;
                root_dir = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--root needs a value"))?,
                );
            }
            "--budget" => {
                index += 1;
                let raw = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--budget needs a value"))?;
                budget = Some(
                    raw.parse()
                        .map_err(|_| anyhow!("invalid --budget: {raw}"))?,
                );
            }
            "--limit" => {
                index += 1;
                let raw = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--limit needs a value"))?;
                limit = Some(raw.parse().map_err(|_| anyhow!("invalid --limit: {raw}"))?);
            }
            "--actor" => {
                index += 1;
                actor = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--actor needs a value"))?,
                );
            }
            "--run" => {
                index += 1;
                run_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--run needs a value"))?,
                );
            }
            "--source-ref" => {
                index += 1;
                source_ref = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--source-ref needs a value"))?,
                );
            }
            "--target-node" => {
                index += 1;
                target_node_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--target-node needs a value"))?,
                );
            }
            "--target-source" => {
                index += 1;
                target_source_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--target-source needs a value"))?,
                );
            }
            "--event-node" => {
                index += 1;
                event_node_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--event-node needs a value"))?,
                );
            }
            "--edge" => {
                index += 1;
                edge_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--edge needs a value"))?,
                );
            }
            "--claim" => {
                index += 1;
                claim_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--claim needs a value"))?,
                );
            }
            "--memory" => {
                index += 1;
                memory_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--memory needs a value"))?,
                );
            }
            "--change" => {
                index += 1;
                change_type = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--change needs a value"))?,
                );
            }
            "--snapshot" => {
                index += 1;
                snapshot_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--snapshot needs a value"))?,
                );
            }
            "--event" => {
                index += 1;
                event_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--event needs a value"))?,
                );
            }
            "--relation" => {
                index += 1;
                let raw = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--relation needs a value"))?;
                relation_kind = Some(parse_brain_relation_kind(raw)?);
            }
            "--source" => {
                index += 1;
                source_refs.push(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--source needs a value"))?,
                );
            }
            "--node" => {
                index += 1;
                node_refs.push(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--node needs a value"))?,
                );
            }
            "--evidence" => {
                index += 1;
                evidence_refs.push(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--evidence needs a value"))?,
                );
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    match subcommand.as_str() {
        "search" => {
            let query = positional.join(" ");
            if query.trim().is_empty() {
                return Err(anyhow!("hyprduck brain search needs a query"));
            }
            Ok(BrainCommand::Search {
                workspace,
                root_dir,
                query,
            })
        }
        "context-pack" => {
            let query = positional.join(" ");
            if query.trim().is_empty() {
                return Err(anyhow!("hyprduck brain context-pack needs a query"));
            }
            Ok(BrainCommand::ContextPack {
                workspace,
                root_dir,
                query,
                budget,
            })
        }
        "event-history" | "events" => Ok(BrainCommand::EventHistory {
            request: ReadRecentEventsRequest {
                scope: hyprduck_engine_types::BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                limit,
                run_id,
                source_ref: source_ref.or_else(|| source_refs.into_iter().next()),
                node_id: event_node_id
                    .or(target_node_id)
                    .or_else(|| node_refs.into_iter().next()),
                edge_id,
                claim_id,
                memory_id,
                change_type,
            },
        }),
        "graph-history" | "states" => Ok(BrainCommand::GraphHistory {
            request: ReadGraphHistoryRequest {
                scope: hyprduck_engine_types::BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                limit,
            },
        }),
        "inspect-state" | "state" => {
            let selector = match (snapshot_id, event_id, positional.first().cloned()) {
                (Some(snapshot_id), None, _) => GraphStateSelector::Snapshot(snapshot_id),
                (None, Some(event_id), _) => GraphStateSelector::Event(event_id),
                (None, None, Some(snapshot_id)) => GraphStateSelector::Snapshot(snapshot_id),
                (Some(_), Some(_), _) => {
                    return Err(anyhow!("use only one of --snapshot or --event"))
                }
                (None, None, None) => {
                    return Err(anyhow!(
                        "hyprduck brain inspect-state needs --snapshot <id> or --event <id>"
                    ))
                }
            };
            Ok(BrainCommand::InspectState {
                request: ReadGraphHistoryRequest {
                    scope: hyprduck_engine_types::BrainReadScope {
                        workspace_id: workspace,
                        root_dir,
                    },
                    limit,
                },
                selector,
            })
        }
        "rollback-state" | "rollback" => {
            let selector = match (snapshot_id, event_id, positional.first().cloned()) {
                (Some(snapshot_id), None, _) => GraphStateSelector::Snapshot(snapshot_id),
                (None, Some(event_id), _) => GraphStateSelector::Event(event_id),
                (None, None, Some(snapshot_id)) => GraphStateSelector::Snapshot(snapshot_id),
                (Some(_), Some(_), _) => {
                    return Err(anyhow!("use only one of --snapshot or --event"))
                }
                (None, None, None) => {
                    return Err(anyhow!(
                        "hyprduck brain rollback-state needs --snapshot <id> or --event <id>"
                    ))
                }
            };
            Ok(BrainCommand::RollbackState {
                history_request: ReadGraphHistoryRequest {
                    scope: hyprduck_engine_types::BrainReadScope {
                        workspace_id: workspace.clone(),
                        root_dir: root_dir.clone(),
                    },
                    limit,
                },
                request: ReconstructBrainRequest {
                    scope: hyprduck_engine_types::BrainReadScope {
                        workspace_id: workspace,
                        root_dir,
                    },
                    up_to_timestamp: None,
                    up_to_materialized_version: None,
                    up_to_event_id: None,
                    output_root: None,
                    write_materialized: true,
                },
                selector,
            })
        }
        "propose-memory" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::Memory,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "propose-node" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::Node,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "propose-claim" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::Claim,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "propose-link" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::Link,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "propose-wiki-page" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::WikiPage,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "append-observation" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::Observation,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        "add-source-note" => proposal_command(ProposalCommandOptions {
            kind: BrainProposalKind::SourceNote,
            workspace,
            root_dir,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
            positional,
        }),
        _ => Err(anyhow!("unknown brain subcommand: {subcommand}")),
    }
}

struct ProposalCommandOptions {
    kind: BrainProposalKind,
    workspace: String,
    root_dir: Option<String>,
    actor: Option<String>,
    target_node_id: Option<String>,
    target_source_id: Option<String>,
    relation_kind: Option<BrainRelationKind>,
    source_refs: Vec<String>,
    node_refs: Vec<String>,
    evidence_refs: Vec<String>,
    positional: Vec<String>,
}

fn proposal_command(options: ProposalCommandOptions) -> Result<BrainCommand> {
    let ProposalCommandOptions {
        kind,
        workspace,
        root_dir,
        actor,
        target_node_id,
        target_source_id,
        relation_kind,
        source_refs,
        node_refs,
        evidence_refs,
        positional,
    } = options;

    if positional.len() < 2 {
        return Err(anyhow!(
            "hyprduck brain proposal commands need <title> <body>"
        ));
    }
    Ok(BrainCommand::ProposeUpdate {
        workspace,
        root_dir,
        kind,
        title: positional[0].clone(),
        body: positional[1..].join(" "),
        actor: actor.unwrap_or_else(|| "hyprduck-cli".into()),
        target_node_id,
        target_source_id,
        relation_kind,
        source_refs,
        node_refs,
        evidence_refs,
    })
}

fn parse_brain_relation_kind(raw: &str) -> Result<BrainRelationKind> {
    match raw {
        "mentions" => Ok(BrainRelationKind::Mentions),
        "supports" => Ok(BrainRelationKind::Supports),
        "contradicts" => Ok(BrainRelationKind::Contradicts),
        "supersedes" => Ok(BrainRelationKind::Supersedes),
        "same_as" => Ok(BrainRelationKind::SameAs),
        "works_at" => Ok(BrainRelationKind::WorksAt),
        "founded" => Ok(BrainRelationKind::Founded),
        "invested_in" => Ok(BrainRelationKind::InvestedIn),
        "advises" => Ok(BrainRelationKind::Advises),
        "attended" => Ok(BrainRelationKind::Attended),
        "owns" => Ok(BrainRelationKind::Owns),
        "responsible_for" => Ok(BrainRelationKind::ResponsibleFor),
        "decided" => Ok(BrainRelationKind::Decided),
        "blocks" => Ok(BrainRelationKind::Blocks),
        "depends_on" => Ok(BrainRelationKind::DependsOn),
        "source_of" => Ok(BrainRelationKind::SourceOf),
        "derived_from" => Ok(BrainRelationKind::DerivedFrom),
        "related_to" => Ok(BrainRelationKind::RelatedTo),
        _ => Err(anyhow!("unknown brain relation kind: {raw}")),
    }
}

#[derive(Debug)]
pub enum Commands {
    Doctor,
    Serve,
    Mcp { command: McpCommand },
    Parse { input: String },
    Engines { command: EnginesCommand },
    Brain { command: BrainCommand },
    Eval { command: EvalCommand },
}

#[derive(Debug)]
pub enum McpCommand {
    Serve,
    InstallClaudeCode,
    InstallCodex,
}

#[derive(Debug)]
pub enum EnginesCommand {
    List,
}

#[derive(Debug)]
pub enum EvalCommand {
    GoldenCorpus {
        fixtures: Option<String>,
        mode: String,
    },
}

#[derive(Debug)]
pub enum BrainCommand {
    Search {
        workspace: String,
        root_dir: Option<String>,
        query: String,
    },
    ContextPack {
        workspace: String,
        root_dir: Option<String>,
        query: String,
        budget: Option<usize>,
    },
    EventHistory {
        request: ReadRecentEventsRequest,
    },
    GraphHistory {
        request: ReadGraphHistoryRequest,
    },
    InspectState {
        request: ReadGraphHistoryRequest,
        selector: GraphStateSelector,
    },
    RollbackState {
        history_request: ReadGraphHistoryRequest,
        request: ReconstructBrainRequest,
        selector: GraphStateSelector,
    },
    ProposeUpdate {
        workspace: String,
        root_dir: Option<String>,
        kind: BrainProposalKind,
        title: String,
        body: String,
        actor: String,
        target_node_id: Option<String>,
        target_source_id: Option<String>,
        relation_kind: Option<BrainRelationKind>,
        source_refs: Vec<String>,
        node_refs: Vec<String>,
        evidence_refs: Vec<String>,
    },
}

#[derive(Debug)]
pub enum GraphStateSelector {
    Snapshot(String),
    Event(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brain_proposal_options() {
        let command = parse_brain_command(
            "propose-link".to_string(),
            vec![
                "--workspace".into(),
                "team".into(),
                "--root".into(),
                "/tmp/hyprduck".into(),
                "--actor".into(),
                "worker-d".into(),
                "--target-node".into(),
                "node-a".into(),
                "--target-source".into(),
                "source-a".into(),
                "--relation".into(),
                "related_to".into(),
                "--source".into(),
                "source-b".into(),
                "--node".into(),
                "node-b".into(),
                "--evidence".into(),
                "evidence-a".into(),
                "Link title".into(),
                "body".into(),
                "text".into(),
            ],
        )
        .expect("proposal command should parse");

        let BrainCommand::ProposeUpdate {
            workspace,
            root_dir,
            kind,
            title,
            body,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
        } = command
        else {
            panic!("expected proposal command");
        };

        assert_eq!(workspace, "team");
        assert_eq!(root_dir.as_deref(), Some("/tmp/hyprduck"));
        assert!(matches!(kind, BrainProposalKind::Link));
        assert_eq!(title, "Link title");
        assert_eq!(body, "body text");
        assert_eq!(actor, "worker-d");
        assert_eq!(target_node_id.as_deref(), Some("node-a"));
        assert_eq!(target_source_id.as_deref(), Some("source-a"));
        assert!(matches!(relation_kind, Some(BrainRelationKind::RelatedTo)));
        assert_eq!(source_refs, vec!["source-b"]);
        assert_eq!(node_refs, vec!["node-b"]);
        assert_eq!(evidence_refs, vec!["evidence-a"]);
    }
}
