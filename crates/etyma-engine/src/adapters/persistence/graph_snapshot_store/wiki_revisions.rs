use anyhow::{Context, Result};
use graphqlite::Graph;
use etyma_engine_types::{BrainRepoSnapshot, WikiPage};
use std::collections::BTreeSet;

use super::{graph_record_version_id, graph_snapshot_created_by_event_id};

pub(super) fn persist_wiki_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let created_by_event_id = graph_snapshot_created_by_event_id(snapshot);
    let mut current_page_ids = BTreeSet::new();
    for page in &snapshot.wiki_pages {
        let stored_page_id = stored_wiki_page_id(graph, &snapshot.workspace_id, page)?;
        current_page_ids.insert(stored_page_id.clone());
        let previous = load_stored_wiki_page_state(graph, &snapshot.workspace_id, &stored_page_id)?;
        let same_event_revision = previous
            .as_ref()
            .is_some_and(|state| state.current_revision_event_id == created_by_event_id);
        let revision = if same_event_revision {
            previous.as_ref().map(|state| state.revision).unwrap_or(1)
        } else {
            previous
                .as_ref()
                .map(|state| state.revision.saturating_add(1))
                .unwrap_or(1)
        };
        if let Some(previous) = previous.as_ref().filter(|_| !same_event_revision) {
            close_wiki_revision(
                graph,
                &snapshot.workspace_id,
                &stored_page_id,
                previous.revision,
                snapshot.generated_at as i64,
                &created_by_event_id,
            )?;
        }
        let evidence_refs_json = serde_json::to_string(&page.evidence_refs)
            .context("failed encoding wiki evidence refs")?;
        let source_refs_json =
            serde_json::to_string(&page.source_refs).context("failed encoding wiki source refs")?;
        let node_refs_json =
            serde_json::to_string(&page.node_refs).context("failed encoding wiki node refs")?;
        let relation_refs_json = "[]";
        let predecessor_revision = (revision > 1).then_some(revision - 1);
        let version_id = wiki_revision_version_id(
            &snapshot.workspace_id,
            &stored_page_id,
            revision,
            &created_by_event_id,
        );
        let valid_from = page.updated_at.max(
            (page.updated_at == 0)
                .then_some(snapshot.generated_at)
                .unwrap_or(0),
        ) as i64;
        let approval_status = "materialized";
        let diff_json = "{}";
        sqlite
            .execute(
                "INSERT INTO wiki_pages (
                    wiki_page_id,
                    workspace_id,
                    path,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    revision,
                    current_revision_event_id,
                    current_revision_version_id,
                    valid_from,
                    valid_to,
                    superseded_by,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, '', ?12)
                ON CONFLICT(wiki_page_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    path=excluded.path,
                    title=excluded.title,
                    body=excluded.body,
                    approval_status=excluded.approval_status,
                    evidence_refs_json=excluded.evidence_refs_json,
                    revision=excluded.revision,
                    current_revision_event_id=excluded.current_revision_event_id,
                    current_revision_version_id=excluded.current_revision_version_id,
                    valid_from=excluded.valid_from,
                    valid_to=0,
                    superseded_by='',
                    updated_at=excluded.updated_at",
                (
                    stored_page_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    page.path.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    revision,
                    created_by_event_id.as_str(),
                    version_id.as_str(),
                    valid_from,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting wiki page row {}", page.page_id))?;
        sqlite
            .execute(
                "INSERT INTO wiki_revisions (
                    wiki_page_id,
                    revision,
                    workspace_id,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    source_refs_json,
                    node_refs_json,
                    relation_refs_json,
                    diff_json,
                    version_id,
                    created_by_event_id,
                    predecessor_revision,
                    superseded_by_event_id,
                    valid_from,
                    valid_to,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, '', ?15, 0, ?16)
                ON CONFLICT(wiki_page_id, revision) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    title=excluded.title,
                    body=excluded.body,
                    approval_status=excluded.approval_status,
                    evidence_refs_json=excluded.evidence_refs_json,
                    source_refs_json=excluded.source_refs_json,
                    node_refs_json=excluded.node_refs_json,
                    relation_refs_json=excluded.relation_refs_json,
                    diff_json=excluded.diff_json,
                    version_id=excluded.version_id,
                    created_by_event_id=excluded.created_by_event_id,
                    predecessor_revision=excluded.predecessor_revision,
                    valid_from=excluded.valid_from,
                    valid_to=0,
                    updated_at=excluded.updated_at",
                (
                    stored_page_id.as_str(),
                    revision,
                    snapshot.workspace_id.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    source_refs_json.as_str(),
                    node_refs_json.as_str(),
                    relation_refs_json,
                    diff_json,
                    version_id.as_str(),
                    created_by_event_id.as_str(),
                    predecessor_revision,
                    valid_from,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting wiki revision row {}", page.page_id))?;
        sqlite
            .execute(
                "DELETE FROM wiki_sections WHERE wiki_page_id = ?1 AND revision = ?2",
                (stored_page_id.as_str(), revision),
            )
            .with_context(|| format!("failed clearing wiki sections for {}", page.page_id))?;
        sqlite
            .execute(
                "DELETE FROM wiki_fts WHERE wiki_page_id = ?1 AND revision = ?2",
                (stored_page_id.as_str(), revision),
            )
            .with_context(|| format!("failed clearing wiki FTS rows for {}", page.page_id))?;
        for section in wiki_sections_from_page(page) {
            let section_evidence_refs_json = serde_json::to_string(&section.evidence_refs)
                .context("failed encoding wiki section evidence refs")?;
            sqlite
                .execute(
                    "INSERT INTO wiki_sections (
                        wiki_page_id,
                        revision,
                        section_index,
                        heading,
                        body,
                        evidence_refs_json,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (
                        stored_page_id.as_str(),
                        revision,
                        section.index,
                        section.heading.as_str(),
                        section.body.as_str(),
                        section_evidence_refs_json.as_str(),
                        page.updated_at as i64,
                    ),
                )
                .with_context(|| format!("failed inserting wiki section row {}", page.page_id))?;
            sqlite
                .execute(
                    "INSERT INTO wiki_fts (workspace_id, wiki_page_id, revision, section_index, title, text)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        snapshot.workspace_id.as_str(),
                        stored_page_id.as_str(),
                        revision,
                        section.index,
                        page.title.as_str(),
                        section.body.as_str(),
                    ),
                )
                .with_context(|| format!("failed indexing wiki section {}", page.page_id))?;
        }
    }
    invalidate_absent_wiki_pages(
        graph,
        &snapshot.workspace_id,
        &current_page_ids,
        snapshot.generated_at as i64,
        &created_by_event_id,
    )?;
    Ok(())
}

struct StoredWikiPageState {
    revision: i64,
    current_revision_event_id: String,
}

fn stored_wiki_page_id(graph: &Graph, workspace_id: &str, page: &WikiPage) -> Result<String> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare("SELECT workspace_id FROM wiki_pages WHERE wiki_page_id = ?1 LIMIT 1")
        .context("failed preparing wiki page id scope query")?;
    let mut rows = statement
        .query([page.page_id.as_str()])
        .context("failed checking existing wiki page id scope")?;
    let existing_workspace = rows
        .next()
        .context("failed reading existing wiki page id scope")?
        .map(|row| row.get::<_, String>(0))
        .transpose()
        .context("failed decoding existing wiki page id scope")?;
    if existing_workspace
        .as_deref()
        .is_some_and(|existing| existing != workspace_id)
    {
        Ok(format!("{}:{}", workspace_id, page.page_id))
    } else {
        Ok(page.page_id.clone())
    }
}

fn load_stored_wiki_page_state(
    graph: &Graph,
    workspace_id: &str,
    wiki_page_id: &str,
) -> Result<Option<StoredWikiPageState>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT revision, current_revision_event_id
             FROM wiki_pages
             WHERE workspace_id = ?1 AND wiki_page_id = ?2
             LIMIT 1",
        )
        .context("failed preparing wiki page state query")?;
    let mut rows = statement
        .query((workspace_id, wiki_page_id))
        .context("failed querying wiki page state")?;
    let Some(row) = rows.next().context("failed reading wiki page state")? else {
        return Ok(None);
    };
    Ok(Some(StoredWikiPageState {
        revision: row.get(0).context("read wiki revision")?,
        current_revision_event_id: row.get(1).context("read wiki current event")?,
    }))
}

fn close_wiki_revision(
    graph: &Graph,
    workspace_id: &str,
    wiki_page_id: &str,
    revision: i64,
    valid_to: i64,
    superseded_by_event_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "UPDATE wiki_revisions
             SET valid_to = ?4,
                 superseded_by_event_id = ?5
             WHERE workspace_id = ?1
               AND wiki_page_id = ?2
               AND revision = ?3
               AND valid_to = 0",
            (
                workspace_id,
                wiki_page_id,
                revision,
                valid_to,
                superseded_by_event_id,
            ),
        )
        .with_context(|| format!("failed closing wiki revision {wiki_page_id}:{revision}"))?;
    Ok(())
}

fn invalidate_absent_wiki_pages(
    graph: &Graph,
    workspace_id: &str,
    current_page_ids: &BTreeSet<String>,
    valid_to: i64,
    superseded_by_event_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, revision
             FROM wiki_pages
             WHERE workspace_id = ?1 AND valid_to = 0",
        )
        .context("failed preparing live wiki page query")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying live wiki pages")?;
    let mut stale_pages = Vec::new();
    while let Some(row) = rows.next().context("failed reading live wiki page")? {
        let wiki_page_id: String = row.get(0).context("read live wiki page id")?;
        let revision: i64 = row.get(1).context("read live wiki page revision")?;
        if !current_page_ids.contains(&wiki_page_id) {
            stale_pages.push((wiki_page_id, revision));
        }
    }
    drop(rows);
    drop(statement);
    for (wiki_page_id, revision) in stale_pages {
        sqlite
            .execute(
                "UPDATE wiki_pages
                 SET valid_to = ?3,
                     superseded_by = ?4
                 WHERE workspace_id = ?1
                   AND wiki_page_id = ?2
                   AND valid_to = 0",
                (
                    workspace_id,
                    wiki_page_id.as_str(),
                    valid_to,
                    superseded_by_event_id,
                ),
            )
            .with_context(|| format!("failed invalidating wiki page {wiki_page_id}"))?;
        close_wiki_revision(
            graph,
            workspace_id,
            &wiki_page_id,
            revision,
            valid_to,
            superseded_by_event_id,
        )?;
    }
    Ok(())
}

fn wiki_revision_version_id(
    workspace_id: &str,
    wiki_page_id: &str,
    revision: i64,
    created_by_event_id: &str,
) -> String {
    graph_record_version_id(
        "wiki",
        workspace_id,
        &format!("{wiki_page_id}:{revision}"),
        created_by_event_id,
    )
}

struct WikiSectionRow {
    index: i64,
    heading: String,
    body: String,
    evidence_refs: Vec<String>,
}

fn wiki_sections_from_page(page: &WikiPage) -> Vec<WikiSectionRow> {
    let mut sections = Vec::new();
    let mut current_heading = page.title.clone();
    let mut current_body = String::new();

    for line in page.body.lines() {
        if let Some(heading) = markdown_heading_text(line) {
            if !current_body.trim().is_empty() || !sections.is_empty() {
                sections.push(WikiSectionRow {
                    index: sections.len() as i64,
                    heading: current_heading,
                    body: current_body.trim().to_owned(),
                    evidence_refs: page.evidence_refs.clone(),
                });
                current_body.clear();
            }
            current_heading = heading.to_owned();
        } else {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line);
        }
    }

    if !current_body.trim().is_empty() || sections.is_empty() {
        sections.push(WikiSectionRow {
            index: sections.len() as i64,
            heading: current_heading,
            body: current_body.trim().to_owned(),
            evidence_refs: page.evidence_refs.clone(),
        });
    }

    sections
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|value| *value == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim_start();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}
