use crate::*;

pub(crate) struct BrainReader {
    pub(crate) repo: BrainArtifactRepository,
    pub(crate) snapshot: BrainRepoSnapshot,
    pub(crate) events: Vec<BrainEvent>,
}

impl BrainReader {
    pub(crate) fn open(scope: &BrainReadScope) -> Result<Self> {
        let root = resolve_brain_workspace_root(scope)?;
        Self::open_workspace_root(root, &scope.workspace_id)
    }

    pub(crate) fn open_workspace_root(root: PathBuf, workspace_id: &str) -> Result<Self> {
        let repo = BrainArtifactRepository::new(root);
        let manifest_path = repo.brain_manifest_path();
        if !manifest_path.exists() {
            return Ok(Self {
                repo,
                snapshot: empty_replayed_brain_snapshot(workspace_id),
                events: Vec::new(),
            });
        }
        let mut snapshot = repo.read_brain_manifest()?;
        if snapshot.workspace_id != workspace_id {
            bail!(
                "brain manifest workspace_id {} does not match requested workspace {}",
                snapshot.workspace_id,
                workspace_id
            );
        }
        // LEGACY: full snapshot artifact load (graph/*.json etc). DB canvas migration guard below.
        // search + context_pack* below are legacy-only (services prefer DB assemble; see context_pack_service).
        // Phase 4 continuation: context pack scoring/capping extracted to policy.rs (reader shrunk ~200 LOC).
        // No behavior change.
        snapshot.nodes = repo.read_json_artifact("graph/nodes.json")?;
        snapshot.relations = repo.read_json_artifact("graph/edges.json")?;
        snapshot.evidence = repo.read_json_artifact("graph/evidence.json")?;
        if let Ok(entities) = repo.read_json_artifact("graph/entities.json") {
            snapshot.entities = entities;
        }
        if let Ok(claims) = repo.read_json_artifact("graph/claims.json") {
            snapshot.claims = claims;
        }
        snapshot.memories = repo.read_optional_json_artifact("memory/records.json")?;
        let events = repo.read_brain_events()?;
        snapshot.events = events.clone();
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(repo.root()))?;
        if store
            .read_graph_canvas_projection_from_db(workspace_id)?
            .is_none()
        {
            store.persist_graph_snapshot(&snapshot)?;
        }
        let artifact_metadata =
            build_context_pack_artifact_metadata(repo.root(), &snapshot.sources);
        store.preserve_artifact_metadata(&artifact_metadata)?;
        store.preserve_context_pack_exports(repo.root(), workspace_id)?;
        Ok(Self {
            repo,
            snapshot,
            events,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        self.repo.root()
    }

    // LEGACY-only: snapshot search used by context_pack and direct legacy callers (brain_read_service fallback etc).
    // Gradually moving assembly to DB side per prior tasks.
    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<BrainSearchResult> {
        let terms = search_terms(query);
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for source in &self.snapshot.sources {
            let haystack = format!(
                "{} {} {} {}",
                source.source_id, source.original_path, source.source_path, source.markdown_path
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Source,
                    id: source.source_id.clone(),
                    title: source.source_id.clone(),
                    path: Some(source.source_path.clone()),
                    score,
                    snippet: source.original_path.clone(),
                });
            }
        }
        for node in self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.valid_to.is_none())
        {
            let haystack = format!("{} {} {}", node.node_id, node.label, node.aliases.join(" "));
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Node,
                    id: node.node_id.clone(),
                    title: node.label.clone(),
                    path: None,
                    score,
                    snippet: evidence_snippet(&node.evidence_ids),
                });
            }
        }
        for entity in &self.snapshot.entities {
            let haystack = format!(
                "{} {:?} {} {} {}",
                entity.entity_id,
                entity.kind,
                entity.name,
                entity.aliases.join(" "),
                entity.source_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Entity,
                    id: entity.entity_id.clone(),
                    title: entity.name.clone(),
                    path: None,
                    score,
                    snippet: evidence_snippet(&entity.evidence_refs),
                });
            }
        }
        for claim in &self.snapshot.claims {
            let haystack = format!(
                "{} {} {} {} {}",
                claim.claim_id,
                claim.statement,
                claim.topic_refs.join(" "),
                claim.source_refs.join(" "),
                claim.evidence_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Claim,
                    id: claim.claim_id.clone(),
                    title: claim.statement.clone(),
                    path: Some("graph/claims.json".into()),
                    score,
                    snippet: format!(
                        "{}; {}",
                        evidence_snippet(&claim.evidence_refs),
                        best_snippet(&claim.statement, &terms)
                    ),
                });
            }
        }
        for relation in self
            .snapshot
            .relations
            .iter()
            .filter(|relation| relation.valid_to.is_none())
        {
            let haystack = format!(
                "{} {:?} {} {} {} {}",
                relation.relation_id,
                relation.kind,
                relation.source_node_id,
                relation.target_node_id,
                relation.label,
                relation.evidence_ids.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Relation,
                    id: relation.relation_id.clone(),
                    title: relation.label.clone(),
                    path: Some("graph/edges.json".into()),
                    score,
                    snippet: format!(
                        "{:?}: {} -> {}; {}",
                        relation.kind,
                        relation.source_node_id,
                        relation.target_node_id,
                        evidence_snippet(&relation.evidence_ids)
                    ),
                });
            }
        }
        for memory in &self.snapshot.memories {
            let haystack = format!(
                "{} {} {} {} {}",
                memory.memory_id,
                memory.title,
                memory.body,
                memory.source_refs.join(" "),
                memory.evidence_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Memory,
                    id: memory.memory_id.clone(),
                    title: memory.title.clone(),
                    path: Some("memory/records.json".into()),
                    score,
                    snippet: format!(
                        "{}; {}",
                        evidence_snippet(&memory.evidence_refs),
                        best_snippet(&memory.body, &terms)
                    ),
                });
            }
        }
        for page in &self.snapshot.wiki_pages {
            let page = self
                .read_wiki_page_body(page.clone())
                .unwrap_or_else(|_| page.clone());
            let haystack = format!("{} {} {}", page.path, page.title, page.body);
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::WikiPage,
                    id: page.page_id,
                    title: page.title,
                    path: Some(page.path),
                    score,
                    snippet: best_snippet(&page.body, &terms),
                });
            }
        }
        for evidence in &self.snapshot.evidence {
            let haystack = format!(
                "{} {} {}",
                evidence.id, evidence.page_label, evidence.snippet
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Evidence,
                    id: evidence.id.clone(),
                    title: evidence.page_label.clone(),
                    path: evidence.markdown_path.clone(),
                    score,
                    snippet: best_snippet(&evidence.snippet, &terms),
                });
            }
        }
        for event in &self.events {
            let haystack = format!(
                "{} {:?} {} {}",
                event.event_id, event.event_type, event.policy_result, event.payload_json
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Event,
                    id: event.event_id.clone(),
                    title: format!("{:?}", event.event_type),
                    path: Some("events/brain_events.jsonl".into()),
                    score,
                    snippet: event.payload_json.clone(),
                });
            }
        }
        results.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.title.cmp(&right.title))
        });
        results.truncate(limit);
        results
    }

    pub(crate) fn read_wiki_page(&self, path: &str) -> Result<WikiPage> {
        let normalized_path = normalize_wiki_path(path)?;
        let page = self
            .snapshot
            .wiki_pages
            .iter()
            .find(|page| page.path == normalized_path)
            .cloned()
            .ok_or_else(|| anyhow!("wiki page {normalized_path} was not found"))?;
        self.read_wiki_page_body(page)
    }

    pub(crate) fn read_wiki_page_body(&self, mut page: WikiPage) -> Result<WikiPage> {
        page.body = self.repo.read_text_artifact(&page.path)?;
        Ok(page)
    }

    pub(crate) fn read_all_wiki_pages(&self) -> Result<Vec<WikiPage>> {
        self.snapshot
            .wiki_pages
            .iter()
            .cloned()
            .map(|page| self.read_wiki_page_body(page))
            .collect()
    }

    pub(crate) fn recent_events(&self, request: &ReadRecentEventsRequest) -> Vec<BrainEvent> {
        let mut events = self
            .events
            .iter()
            .filter(|event| event_matches_recent_events_request(event, request))
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.event_id.cmp(&left.event_id))
        });
        events.truncate(request.limit.unwrap_or(20));
        events
    }

    pub(crate) fn context_pack(&self, query: &str, budget: usize) -> Result<BrainContextPack> {
        self.context_pack_with_selection(query, budget, None)
    }

    // LEGACY: context_pack* assemble from snapshot. DB assemble (assemble_context_pack_v1_from_db) preferred in
    // context_pack_service; reader path kept only for selected_node case + fallbacks (no behavior change here).
    // Phase 4: moved scoring/capping helpers to policy.rs to shrink reader (continuation).
    pub(crate) fn context_pack_with_selection(
        &self,
        query: &str,
        budget: usize,
        selected_node_id: Option<&str>,
    ) -> Result<BrainContextPack> {
        let results = self.search(query, 24);
        let mut page_paths = BTreeSet::new();
        let mut node_ids = BTreeSet::new();
        let mut entity_ids = BTreeSet::new();
        let mut claim_ids = BTreeSet::new();
        let mut relation_ids = BTreeSet::new();
        let mut source_ids = BTreeSet::new();
        let mut evidence_ids = BTreeSet::new();
        let mut memory_ids = BTreeSet::new();
        let mut selected_bias_node_ids = BTreeSet::new();
        let mut selected_bias_evidence_ids = BTreeSet::new();
        let mut query_seed_evidence_ids = BTreeSet::new();
        let mut extra_warnings = Vec::new();
        for result in &results {
            match result.kind {
                BrainSearchResultKind::WikiPage => {
                    if let Some(path) = &result.path {
                        page_paths.insert(path.clone());
                    }
                }
                BrainSearchResultKind::Node => {
                    if let Some(node) = self
                        .snapshot
                        .nodes
                        .iter()
                        .find(|node| node.node_id == result.id && node.valid_to.is_none())
                    {
                        query_seed_evidence_ids.extend(node.evidence_ids.iter().cloned());
                    }
                    node_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Entity => {
                    entity_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Claim => {
                    if let Some(claim) = self
                        .snapshot
                        .claims
                        .iter()
                        .find(|claim| claim.claim_id == result.id)
                    {
                        query_seed_evidence_ids.extend(claim.evidence_refs.iter().cloned());
                    }
                    claim_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Relation => {
                    if let Some(relation) = self
                        .snapshot
                        .relations
                        .iter()
                        .find(|relation| relation.relation_id == result.id)
                    {
                        query_seed_evidence_ids.extend(relation.evidence_ids.iter().cloned());
                    }
                    relation_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Source => {
                    source_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Memory => {
                    memory_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Evidence => {
                    evidence_ids.insert(result.id.clone());
                    query_seed_evidence_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Event => {
                    if let Some(event) =
                        self.events.iter().find(|event| event.event_id == result.id)
                    {
                        source_ids.extend(event.source_refs.iter().cloned());
                        node_ids.extend(event.node_refs.iter().cloned());
                        relation_ids.extend(event.relation_refs.iter().cloned());
                        evidence_ids.extend(event.evidence_refs.iter().cloned());
                    }
                }
            }
        }
        if let Some(selected_node_id) = selected_node_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(node) = self
                .snapshot
                .nodes
                .iter()
                .find(|node| node.node_id == selected_node_id && node.valid_to.is_none())
            {
                selected_bias_node_ids.insert(node.node_id.clone());
                selected_bias_evidence_ids.extend(node.evidence_ids.iter().cloned());
                query_seed_evidence_ids.extend(node.evidence_ids.iter().cloned());
                node_ids.insert(node.node_id.clone());
                source_ids.extend(node.source_ids.iter().cloned());
                evidence_ids.extend(node.evidence_ids.iter().cloned());
            } else {
                extra_warnings.push(format!(
                    "Selected node {selected_node_id} was not found; Context Pack used query relevance only."
                ));
            }
        }

        for evidence in &self.snapshot.evidence {
            if evidence_ids.contains(&evidence.id) {
                if let Some(source_id) = &evidence.source_id {
                    source_ids.insert(source_id.clone());
                }
            }
            if evidence
                .source_id
                .as_ref()
                .is_some_and(|source_id| source_ids.contains(source_id))
            {
                evidence_ids.insert(evidence.id.clone());
            }
        }
        protect_context_pack_source_evidence(
            &self.snapshot.evidence,
            query,
            &source_ids,
            &query_seed_evidence_ids,
            &mut selected_bias_evidence_ids,
        );
        for node in self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.valid_to.is_none())
        {
            if node_ids.contains(&node.node_id)
                || node
                    .source_ids
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || node
                    .evidence_ids
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                node_ids.insert(node.node_id.clone());
                source_ids.extend(node.source_ids.iter().cloned());
                evidence_ids.extend(node.evidence_ids.iter().cloned());
            }
        }
        for entity in &self.snapshot.entities {
            if entity_ids.contains(&entity.entity_id)
                || entity
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || entity
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                entity_ids.insert(entity.entity_id.clone());
                node_ids.insert(entity.entity_id.clone());
                source_ids.extend(entity.source_refs.iter().cloned());
                evidence_ids.extend(entity.evidence_refs.iter().cloned());
            }
        }
        for claim in &self.snapshot.claims {
            if claim_ids.contains(&claim.claim_id)
                || claim
                    .topic_refs
                    .iter()
                    .any(|node_id| node_ids.contains(node_id))
                || claim
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || claim
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                claim_ids.insert(claim.claim_id.clone());
                node_ids.extend(claim.topic_refs.iter().cloned());
                source_ids.extend(claim.source_refs.iter().cloned());
                evidence_ids.extend(claim.evidence_refs.iter().cloned());
            }
        }
        for relation in self
            .snapshot
            .relations
            .iter()
            .filter(|relation| relation.valid_to.is_none())
        {
            if relation_ids.contains(&relation.relation_id)
                || node_ids.contains(&relation.source_node_id)
                || node_ids.contains(&relation.target_node_id)
                || relation
                    .evidence_ids
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                relation_ids.insert(relation.relation_id.clone());
                node_ids.insert(relation.source_node_id.clone());
                node_ids.insert(relation.target_node_id.clone());
                evidence_ids.extend(relation.evidence_ids.iter().cloned());
            }
        }
        for node in self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.valid_to.is_none())
        {
            if node_ids.contains(&node.node_id) {
                source_ids.extend(node.source_ids.iter().cloned());
                evidence_ids.extend(node.evidence_ids.iter().cloned());
            }
        }

        let entities = self
            .snapshot
            .entities
            .iter()
            .filter(|entity| entity_ids.contains(&entity.entity_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut claims = self
            .snapshot
            .claims
            .iter()
            .filter(|claim| claim_ids.contains(&claim.claim_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut nodes = self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node.valid_to.is_none())
            .filter(|node| node_ids.contains(&node.node_id))
            .cloned()
            .collect::<Vec<_>>();
        let selected_node_ids = nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<BTreeSet<_>>();
        let mut relations = self
            .snapshot
            .relations
            .iter()
            .filter(|relation| relation.valid_to.is_none())
            .filter(|relation| {
                relation_ids.contains(&relation.relation_id)
                    || selected_node_ids.contains(&relation.source_node_id)
                    || selected_node_ids.contains(&relation.target_node_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let memory_terms = search_terms(query);
        let memories = self
            .snapshot
            .memories
            .iter()
            .filter(|memory| {
                let haystack = format!("{} {}", memory.title, memory.body);
                memory_ids.contains(&memory.memory_id)
                    || (!memory_terms.is_empty() && match_score(&memory_terms, &haystack).is_some())
            })
            .cloned()
            .collect::<Vec<_>>();
        for memory in &memories {
            source_ids.extend(memory.source_refs.iter().cloned());
            evidence_ids.extend(memory.evidence_refs.iter().cloned());
        }
        let sources = self
            .snapshot
            .sources
            .iter()
            .filter(|source| source_ids.contains(&source.source_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut evidence = self
            .snapshot
            .evidence
            .iter()
            .filter(|evidence| evidence_ids.contains(&evidence.id))
            .cloned()
            .collect::<Vec<_>>();

        let mut wiki_pages = Vec::new();
        for page in &self.snapshot.wiki_pages {
            if page
                .node_refs
                .iter()
                .any(|node_id| node_ids.contains(node_id))
                || page
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || page
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                page_paths.insert(page.path.clone());
            }
        }
        for path in page_paths {
            if let Ok(page) = self.read_wiki_page(&path) {
                wiki_pages.push(page);
            }
        }
        if wiki_pages.is_empty() {
            for path in ["wiki/index.md", "wiki/overview.md"] {
                if let Ok(page) = self.read_wiki_page(path) {
                    wiki_pages.push(page);
                }
            }
        }

        let (evidence_before_cap, graph_fact_before_cap) =
            (evidence.len(), claims.len() + relations.len());
        policy::cap_context_pack_records(
            query,
            budget,
            &selected_bias_node_ids,
            &selected_bias_evidence_ids,
            &mut nodes,
            &mut claims,
            &mut relations,
            &mut evidence,
        );
        let mut warnings = context_pack_warnings(&nodes, &evidence, budget);
        warnings.extend(extra_warnings);
        if evidence.len() < evidence_before_cap {
            warnings.push(format!(
                "Context Pack selected evidence was capped at {} refs.",
                evidence.len()
            ));
        }
        if claims.len() + relations.len() < graph_fact_before_cap {
            warnings.push(format!(
                "Context Pack selected graph facts were capped at {} records.",
                claims.len() + relations.len()
            ));
        }
        trim_context_pack_to_budget(budget, &mut wiki_pages, &mut nodes);
        let recent_events = self.recent_events(&ReadRecentEventsRequest {
            scope: BrainReadScope {
                workspace_id: self.snapshot.workspace_id.clone(),
                root_dir: Some(self.repo.root().display().to_string()),
            },
            limit: Some(5),
            run_id: None,
            source_ref: None,
            node_id: None,
            edge_id: None,
            claim_id: None,
            memory_id: None,
            change_type: None,
        });
        Ok(BrainContextPack {
            workspace_id: self.snapshot.workspace_id.clone(),
            query: query.to_string(),
            token_budget: budget,
            summary: format!(
                "Context pack for \"{}\" with {} wiki pages, {} nodes, {} entities, {} claims, {} relations, {} sources, {} memories, {} evidence refs, and {} recent events.",
                query,
                wiki_pages.len(),
                nodes.len(),
                entities.len(),
                claims.len(),
                relations.len(),
                sources.len(),
                memories.len(),
                evidence.len(),
                recent_events.len()
            ),
            wiki_pages,
            nodes,
            sources,
            memories,
            entities,
            claims,
            relations,
            evidence,
            recent_events,
            warnings,
        })
    }
}

fn protect_context_pack_source_evidence(
    evidence: &[EvidenceRef],
    query: &str,
    source_ids: &BTreeSet<String>,
    seed_evidence_ids: &BTreeSet<String>,
    selected_bias_evidence_ids: &mut BTreeSet<String>,
) {
    selected_bias_evidence_ids.extend(seed_evidence_ids.iter().cloned());
    protect_follow_up_evidence(evidence, seed_evidence_ids, selected_bias_evidence_ids);

    let terms = search_terms(query);
    if terms.is_empty() || source_ids.is_empty() {
        return;
    }

    let mut query_matches = evidence
        .iter()
        .filter(|evidence| {
            evidence
                .source_id
                .as_ref()
                .is_some_and(|source_id| source_ids.contains(source_id))
        })
        .filter_map(|evidence| {
            match_score(&terms, &evidence.snippet).map(|score| (score, evidence))
        })
        .collect::<Vec<_>>();
    query_matches.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then(left.page_index.cmp(&right.page_index))
            .then(left.id.cmp(&right.id))
    });

    let matched_evidence_ids = query_matches
        .into_iter()
        .take(6)
        .map(|(_, evidence)| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    selected_bias_evidence_ids.extend(matched_evidence_ids.iter().cloned());
    protect_follow_up_evidence(evidence, &matched_evidence_ids, selected_bias_evidence_ids);
}

fn protect_follow_up_evidence(
    evidence: &[EvidenceRef],
    seed_evidence_ids: &BTreeSet<String>,
    selected_bias_evidence_ids: &mut BTreeSet<String>,
) {
    if seed_evidence_ids.is_empty() {
        return;
    }

    let mut evidence_by_source: BTreeMap<String, Vec<&EvidenceRef>> = BTreeMap::new();
    for evidence_ref in evidence {
        let Some(source_id) = &evidence_ref.source_id else {
            continue;
        };
        evidence_by_source
            .entry(source_id.clone())
            .or_default()
            .push(evidence_ref);
    }

    for source_evidence in evidence_by_source.values_mut() {
        source_evidence.sort_by(|left, right| {
            left.page_index
                .cmp(&right.page_index)
                .then(left.id.cmp(&right.id))
        });
        for (index, evidence_ref) in source_evidence.iter().enumerate() {
            if !seed_evidence_ids.contains(&evidence_ref.id) {
                continue;
            }
            let seed_page = evidence_ref.page_index;
            for candidate in source_evidence.iter().skip(index).take(5) {
                selected_bias_evidence_ids.insert(candidate.id.clone());
            }
            if let Some(seed_page) = seed_page {
                for candidate in source_evidence.iter().filter(|candidate| {
                    candidate
                        .page_index
                        .is_some_and(|page| page >= seed_page && page <= seed_page + 2)
                }) {
                    selected_bias_evidence_ids.insert(candidate.id.clone());
                }
            }
        }
    }
}

// Context pack scoring/capping helpers moved to policy.rs (Phase 4 shrink).
// Only cap_context_pack_records (and supporting fns) live in policy now for reuse
// and to keep reader.rs focused on thin legacy wrappers + artifact loading.
