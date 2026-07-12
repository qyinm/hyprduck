-- Repair DBs that applied an earlier 0005 draft with redundant non-unique
-- live indexes (unique partial indexes already cover the same keys).
DROP INDEX IF EXISTS graph.idx_graph_nodes_workspace_live;
DROP INDEX IF EXISTS graph.idx_graph_relations_workspace_live;
DROP INDEX IF EXISTS graph.idx_graph_claims_workspace_live;
