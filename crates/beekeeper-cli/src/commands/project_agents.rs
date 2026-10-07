//! `bee projects agents` — a project's agents, as their owners published them.
//!
//! An agent belongs to a project when its owner's computer records the
//! association and publishes it on the agent's owner-signed kind:30177 as
//! `project_digest` ([`beekeeper_core::project_agent_association`]), a compact
//! equality key for the coordinate. Associations are published only for
//! projects whose head is public; a private project publishes none, so this
//! command lists no agents for one (see [`cmd_agents`]).
//!
//! That publication is a *claim by its author*, and the relay does not check
//! it. This command counts a claim only when its author is the project's
//! creator or a roster owner or collaborator — never a viewer, never a
//! stranger — reads only the newest event at each `(author, d)` address, so a
//! withdrawn association stays withdrawn, and lists each agent once, by the
//! newest claim any authorized author made for it. Every printed row carries
//! `verified: true`: an unverified claim is never printed.
//!
//! What this list is **not**: the set of agents a hire can seat. A hire is
//! answered by the session founder's computer, which seats only agents *it*
//! holds that belong to the project. An agent owned by someone else appears
//! here and runs on their computer.

use std::collections::BTreeMap;

use beekeeper_core::kind::{
    normalize_project_coordinate, KIND_MANAGED_AGENT, KIND_PROJECT, PROJECT_ACCESS_PRIVATE,
    PROJECT_ACCESS_TAG, PROJECT_ROLE_COLLABORATOR, PROJECT_ROLE_OWNER,
};
use beekeeper_core::project_agent_association::{
    project_agent_digest, PROJECT_AGENT_DIGEST_CONTENT_KEY, PROJECT_AGENT_ROLE_CONTENT_KEY,
};
use serde::Serialize;
use serde_json::Value;

use crate::client::BeekeeperClient;
use crate::commands::projects::{
    project_roster, roster_with_creator, validate_member_pubkey, validate_project_slug,
};
use crate::error::CliError;

/// The environment variable a session provider sets on a project seat to the
/// umbrella's project coordinate; the default project for this command.
pub const PROJECT_ENV: &str = "BEEKEEPER_PULSE_PROJECT";

/// One agent of a project, as its owner published it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectAgentRow {
    /// The agent's pubkey (the kind:30177 `d` tag).
    pub pubkey: String,
    /// The agent's display name from the owner's newest publication.
    pub name: String,
    /// The agent's primary role (`home_role`), or `None` when none was
    /// published — such an agent cannot be hired by role.
    pub role: Option<String>,
    /// The pubkey that signed the claim: the agent's owner, on whose computer
    /// the agent runs.
    pub owner: String,
    /// The owner's role on this project (`owner` or `collaborator`), as the
    /// signed roster states it.
    pub owner_role: String,
}

/// The `verified` value every printed row carries: the claim's author was
/// checked against the project's signed roster (the relay-signed kind:39010
/// projection, or the head's bootstrap `p` tags when no projection exists)
/// and holds owner or collaborator authority. There is no `false`: a claim
/// that cannot be verified is not printed at all.
pub const ROW_VERIFIED: bool = true;

/// Resolve which project `bee projects agents` reads, as a normalized
/// `30621:<owner-hex>:<slug>` coordinate.
///
/// Precedence: `--project`, then `SLUG` (with `--owner`, defaulting to
/// `caller_hex`, exactly as `bee projects members` does), then the value of
/// [`PROJECT_ENV`]. A value that does not name a project an agent can be
/// associated with is a usage error, and so is resolving nothing: an empty
/// list for a project nobody named would read as "this project has no
/// agents".
pub fn resolve_project_coordinate(
    slug: Option<&str>,
    owner: Option<&str>,
    project: Option<&str>,
    env_project: Option<&str>,
    caller_hex: &str,
) -> Result<String, CliError> {
    if let Some(project) = project {
        return normalize_project_coordinate(project.trim()).ok_or_else(|| {
            CliError::Usage(format!(
                "--project must be a project coordinate 30621:<owner-hex>:<slug> (got {project:?})"
            ))
        });
    }
    if let Some(slug) = slug {
        validate_project_slug(slug)?;
        let owner = match owner {
            Some(owner) => {
                validate_member_pubkey(owner)?;
                owner
            }
            None => caller_hex,
        };
        let coordinate = format!("{KIND_PROJECT}:{owner}:{slug}");
        return normalize_project_coordinate(&coordinate).ok_or_else(|| {
            CliError::Usage(format!(
                "project slug {slug:?} cannot carry agents: an agent's project must be a \
                 coordinate whose slug is 1-64 characters with no control characters"
            ))
        });
    }
    if owner.is_some() {
        return Err(CliError::Usage("--owner needs a project SLUG".into()));
    }
    match env_project.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => normalize_project_coordinate(value).ok_or_else(|| {
            CliError::Usage(format!(
                "{PROJECT_ENV} is not a project coordinate 30621:<owner-hex>:<slug> (got {value:?})"
            ))
        }),
        None => Err(CliError::Usage(format!(
            "no project named: pass SLUG [--owner <hex>] or --project <30621:owner:slug>; \
             inside a project seat {PROJECT_ENV} supplies it"
        ))),
    }
}

/// The roster members whose kind:30177 claims count for this project: the
/// creator (implicit owner) and every roster owner or collaborator, keyed by
/// lowercase pubkey to their project role. Viewers are excluded.
fn authorized_authors(coordinate: &str, roster: Vec<(String, String)>) -> BTreeMap<String, String> {
    let creator = coordinate.split(':').nth(1).unwrap_or_default();
    roster_with_creator(creator, roster)
        .into_iter()
        .filter(|(pubkey, role)| {
            !pubkey.is_empty() && (role == PROJECT_ROLE_OWNER || role == PROJECT_ROLE_COLLABORATOR)
        })
        .map(|(pubkey, role)| (pubkey.to_ascii_lowercase(), role))
        .collect()
}

fn is_lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn event_d_tag(event: &Value) -> Option<&str> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|tag| tag.first().and_then(Value::as_str) == Some("d"))?
        .get(1)?
        .as_str()
}

/// One readable kind:30177 claim, before its digest is compared.
struct Claim<'a> {
    event: &'a Value,
    author: String,
    agent: String,
    name: String,
    role: Option<String>,
    digest: Option<String>,
}

/// Read one event as a claim, mirroring the desktop's
/// `readPublishedAgentAssociation`: a kind:30177 whose `d` is a 64-hex agent
/// pubkey and whose content is a JSON object with a string `name`. Anything
/// else is not a claim at all, so it never supersedes a readable one.
fn read_claim(event: &Value) -> Option<Claim<'_>> {
    if event.get("kind").and_then(Value::as_u64) != Some(u64::from(KIND_MANAGED_AGENT)) {
        return None;
    }
    let author = event.get("pubkey")?.as_str()?.to_ascii_lowercase();
    let agent = event_d_tag(event).filter(|d| is_lower_hex64(d))?.to_owned();
    let content: Value = serde_json::from_str(event.get("content")?.as_str()?).ok()?;
    let content = content.as_object()?;
    let name = content.get("name")?.as_str()?.to_owned();
    let role = content
        .get(PROJECT_AGENT_ROLE_CONTENT_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_owned);
    let digest = content
        .get(PROJECT_AGENT_DIGEST_CONTENT_KEY)
        .and_then(Value::as_str)
        .filter(|digest| is_lower_hex64(digest))
        .map(str::to_owned);
    Some(Claim {
        event,
        author,
        agent,
        name,
        role,
        digest,
    })
}

/// Fold kind:30177 events into this project's agents.
///
/// Pure, so the whole authority rule is testable offline. The order is the
/// desktop's (`acceptPublishedProjectAgents` in
/// `desktop/src/features/project-agents/lib/publishedProjectAgents.ts`), so
/// the CLI and the Agents tab list the same agents:
/// 1. events from anyone but the project's creator or a roster owner or
///    collaborator are dropped (viewers and strangers do not count);
/// 2. unreadable events are dropped ([`read_claim`]);
/// 3. the newest claim at each `(author, d)` address is that author's current
///    claim — a later publication that drops or changes the association wins
///    over an older one that carried it;
/// 4. per agent pubkey, the newest current claim **across authors** decides,
///    so an agent two authors claim is listed once;
/// 5. that claim counts only when its `project_digest` is
///    [`project_agent_digest`] of `coordinate` — so a newer claim for another
///    project hides an older claim for this one.
///
/// "Newest" is NIP-01 replaceable ordering: later `created_at`, then lowest
/// event id. Rows sort by role (role-less agents last), then name, then
/// pubkey. An unnormalizable `coordinate` yields no rows.
pub fn fold_project_agents(
    coordinate: &str,
    roster: Vec<(String, String)>,
    events: &[Value],
) -> Vec<ProjectAgentRow> {
    // The signed roster is the only authority source: there is no fallback
    // that accepts a claim whose author's project role was not read here.
    let authorities = authorized_authors(coordinate, roster);
    let Some(digest) = project_agent_digest(coordinate) else {
        return Vec::new();
    };

    let mut by_address: BTreeMap<(String, String), Claim<'_>> = BTreeMap::new();
    for claim in events.iter().filter_map(read_claim) {
        if !authorities.contains_key(&claim.author) {
            continue;
        }
        let key = (claim.author.clone(), claim.agent.clone());
        if by_address
            .get(&key)
            .is_none_or(|current| newer_than(claim.event, current.event))
        {
            by_address.insert(key, claim);
        }
    }

    let mut by_agent: BTreeMap<String, Claim<'_>> = BTreeMap::new();
    for claim in by_address.into_values() {
        if by_agent
            .get(&claim.agent)
            .is_none_or(|current| newer_than(claim.event, current.event))
        {
            by_agent.insert(claim.agent.clone(), claim);
        }
    }

    let mut rows: Vec<ProjectAgentRow> = by_agent
        .into_values()
        .filter(|claim| claim.digest.as_deref() == Some(digest.as_str()))
        .filter_map(|claim| {
            let owner_role = authorities.get(&claim.author)?.clone();
            Some(ProjectAgentRow {
                pubkey: claim.agent,
                name: claim.name,
                role: claim.role,
                owner: claim.author,
                owner_role,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        (a.role.is_none(), &a.role, &a.name, &a.pubkey).cmp(&(
            b.role.is_none(),
            &b.role,
            &b.name,
            &b.pubkey,
        ))
    });
    rows
}

/// NIP-01 replaceable ordering: later `created_at` wins; on a tie the lowest
/// event id wins.
fn newer_than(candidate: &Value, current: &Value) -> bool {
    let at = |event: &Value| event.get("created_at").and_then(Value::as_u64).unwrap_or(0);
    let id = |event: &Value| {
        event
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match at(candidate).cmp(&at(current)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => id(candidate) < id(current),
    }
}

/// One printed row for `format`: the full row as JSON, or the reduced
/// `{pubkey, name, role, owner, verified}` for `--format compact`. Both carry
/// `verified` ([`ROW_VERIFIED`]), so a caller reading either format never has
/// to infer that a row's authority was checked.
fn render_row(row: &ProjectAgentRow, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Json => serde_json::json!({
            "pubkey": row.pubkey,
            "name": row.name,
            "role": row.role,
            "owner": row.owner,
            "owner_role": row.owner_role,
            "verified": ROW_VERIFIED,
        }),
        crate::OutputFormat::Compact => serde_json::json!({
            "pubkey": row.pubkey,
            "name": row.name,
            "role": row.role,
            "owner": row.owner,
            "verified": ROW_VERIFIED,
        }),
    }
}

/// The not-found message for a project this identity cannot read. `reason`
/// is the underlying lookup's own message.
///
/// No rows are printed in that case — not even the claims of the owner that
/// attested this identity — because without the signed roster no claim's
/// authority can be verified. The message says where the answer is instead.
pub fn unreadable_project_message(reason: &str, coordinate: &str) -> String {
    format!(
        "{reason}: {coordinate} is not readable by this identity (a private project hides \
         its head and roster from identities not on it). Private projects do not publish \
         their agents, so none are listed. Inside a lead seat, the session's first message \
         lists the project's agents on the hosting computer. Hires are unaffected: the host \
         seats by its own records"
    )
}

/// Whether a kind:30621 head (as relay JSON) is private: it carries
/// `["buzz-access","private"]`. Fails closed like
/// [`beekeeper_core::kind::is_private_project_event`]: any `buzz-access` tag whose
/// value is `private` counts, whatever else the tag list holds.
pub fn head_is_private(head: &Value) -> bool {
    head.get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| {
            tags.iter().filter_map(Value::as_array).any(|tag| {
                tag.first().and_then(Value::as_str) == Some(PROJECT_ACCESS_TAG)
                    && tag.get(1).and_then(Value::as_str) == Some(PROJECT_ACCESS_PRIVATE)
            })
        })
}

/// The newest kind:30621 head at `coordinate` this identity can read, or
/// `None` when the relay returns none (absent, or private and withheld).
async fn read_project_head(
    client: &BeekeeperClient,
    coordinate: &str,
) -> Result<Option<Value>, CliError> {
    let mut parts = coordinate.splitn(3, ':');
    let (Some(_), Some(creator), Some(slug)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let raw = client
        .query(&serde_json::json!({
            "kinds": [KIND_PROJECT],
            "authors": [creator],
            "#d": [slug],
            "limit": 1,
        }))
        .await?;
    let heads: Vec<Value> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
    Ok(heads
        .into_iter()
        .max_by_key(|head| head.get("created_at").and_then(Value::as_u64)))
}

/// `bee projects agents` — print the project's agents as published by their
/// owners.
///
/// Reads the project head first. No readable head is a not-found error
/// ([`unreadable_project_message`]) with nothing on stdout. A readable private
/// head prints `[]` with a stderr note, because private projects do not
/// publish agent associations. Otherwise the signed roster decides whose
/// kind:30177 claims count ([`fold_project_agents`]).
pub async fn cmd_agents(
    client: &BeekeeperClient,
    slug: Option<&str>,
    owner: Option<&str>,
    project: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let env_project = std::env::var(PROJECT_ENV).ok();
    let caller = client.keys().public_key().to_hex();
    let coordinate =
        resolve_project_coordinate(slug, owner, project, env_project.as_deref(), &caller)?;

    let Some(head) = read_project_head(client, &coordinate).await? else {
        return Err(CliError::NotFound(unreadable_project_message(
            "project not found",
            &coordinate,
        )));
    };
    if head_is_private(&head) {
        eprintln!(
            "{coordinate} is a private project: private projects do not publish agent \
             associations, so none are listed. Inside a lead seat, the session's first \
             message lists the project's agents on the hosting computer"
        );
        println!("[]");
        return Ok(());
    }

    let roster = match project_roster(client, &coordinate).await {
        Ok(roster) => roster,
        Err(CliError::NotFound(reason)) => {
            return Err(CliError::NotFound(unreadable_project_message(
                &reason,
                &coordinate,
            )))
        }
        Err(error) => return Err(error),
    };
    let authors: Vec<String> = authorized_authors(&coordinate, roster.clone())
        .into_keys()
        .collect();
    let events = if authors.is_empty() {
        Vec::new()
    } else {
        client
            .query_all(serde_json::json!({
                "kinds": [KIND_MANAGED_AGENT],
                "authors": authors,
            }))
            .await?
    };

    let rows = fold_project_agents(&coordinate, roster, &events);
    if rows.is_empty() {
        eprintln!(
            "no agents are published for {coordinate}: an agent appears here once its owner's \
             app has associated it with the project and published that association"
        );
    }
    let output: Vec<Value> = rows.iter().map(|row| render_row(row, format)).collect();
    println!("{}", Value::Array(output));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn coordinate() -> String {
        format!("30621:{}:alpha", hex('c'))
    }

    fn content(name: &str, role: Option<&str>, project: &str) -> String {
        let mut value = json!({ "name": name, "parallelism": 1, "respond_to": "anyone" });
        if let Some(role) = role {
            value["home_role"] = json!(role);
        }
        if let Some(digest) = project_agent_digest(project) {
            value["project_digest"] = json!(digest);
        }
        value.to_string()
    }

    fn agent_event(author: &str, agent: &str, created_at: u64, content: String) -> Value {
        json!({
            "id": format!("{author}{agent}{created_at}"),
            "pubkey": author,
            "created_at": created_at,
            "kind": KIND_MANAGED_AGENT,
            "tags": [["d", agent]],
            "content": content,
        })
    }

    fn roster() -> Vec<(String, String)> {
        vec![
            (hex('1'), PROJECT_ROLE_OWNER.to_owned()),
            (hex('2'), PROJECT_ROLE_COLLABORATOR.to_owned()),
            (hex('3'), "viewer".to_owned()),
        ]
    }

    #[test]
    fn a_claim_counts_from_the_creator_an_owner_and_a_collaborator() {
        let project = coordinate();
        let events = vec![
            agent_event(
                &hex('c'),
                &hex('a'),
                10,
                content("Ada", Some("builder"), &project),
            ),
            agent_event(
                &hex('1'),
                &hex('b'),
                10,
                content("Bo", Some("runner"), &project),
            ),
            agent_event(
                &hex('2'),
                &hex('d'),
                10,
                content("Cy", Some("builder"), &project),
            ),
        ];
        let rows = fold_project_agents(&project, roster(), &events);
        let summary: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.name.as_str(),
                    row.role.as_deref(),
                    row.owner_role.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Ada", Some("builder"), "owner"),
                ("Cy", Some("builder"), "collaborator"),
                ("Bo", Some("runner"), "owner"),
            ]
        );
        assert_eq!(rows[0].owner, hex('c'), "the creator counts as an owner");
        assert_eq!(rows[0].pubkey, hex('a'));
    }

    #[test]
    fn without_a_roster_no_claim_is_accepted_not_even_the_attesting_owners() {
        // The removed fallback accepted an unreadable project's claims from
        // the seat's attesting owner. With no roster read, the only authority
        // left is the coordinate's creator; any other author's claim —
        // including an owner that attested this identity — yields no row.
        let project = coordinate();
        let attesting_owner = hex('a');
        let events = vec![
            agent_event(
                &attesting_owner,
                &hex('1'),
                10,
                content("Zephyr", Some("builder"), &project),
            ),
            agent_event(
                &hex('9'),
                &hex('2'),
                10,
                content("Stranger", Some("builder"), &project),
            ),
        ];
        assert!(fold_project_agents(&project, Vec::new(), &events).is_empty());
        assert!(fold_project_agents(&project, roster(), &events).is_empty());
    }

    #[test]
    fn the_unreadable_project_message_says_why_and_where_to_look() {
        let project = coordinate();
        let message = unreadable_project_message("project \"alpha\" not found", &project);
        for part in [
            "project \"alpha\" not found",
            project.as_str(),
            "not readable by this identity",
            "a private project hides its head and roster from identities not on it",
            "Private projects do not publish their agents",
            "the session's first message lists the project's agents on the hosting computer",
            "Hires are unaffected: the host seats by its own records",
        ] {
            assert!(message.contains(part), "{message:?} omits {part:?}");
        }
    }

    #[test]
    fn a_head_is_private_only_when_it_says_so() {
        let head = |tags: Value| json!({ "kind": KIND_PROJECT, "tags": tags });
        assert!(head_is_private(&head(json!([
            ["d", "alpha"],
            ["buzz-access", "private"]
        ]))));
        // Fails closed on shapes ingest would reject.
        assert!(head_is_private(&head(json!([
            ["buzz-access", "public"],
            ["buzz-access", "private", "extra"]
        ]))));
        assert!(!head_is_private(&head(json!([
            ["d", "alpha"],
            ["buzz-access", "public"]
        ]))));
        assert!(!head_is_private(&head(json!([["d", "alpha"]]))));
        assert!(!head_is_private(&head(json!([["private", "buzz-access"]]))));
        assert!(!head_is_private(&json!({ "kind": KIND_PROJECT })));
    }

    #[test]
    fn a_viewer_authored_claim_is_excluded() {
        let project = coordinate();
        let events = vec![agent_event(
            &hex('3'),
            &hex('a'),
            10,
            content("Viewer's", Some("builder"), &project),
        )];
        assert!(fold_project_agents(&project, roster(), &events).is_empty());
    }

    #[test]
    fn an_author_outside_the_roster_is_excluded() {
        let project = coordinate();
        let events = vec![agent_event(
            &hex('9'),
            &hex('a'),
            10,
            content("Stranger's", Some("builder"), &project),
        )];
        assert!(fold_project_agents(&project, roster(), &events).is_empty());
    }

    #[test]
    fn the_newest_publication_at_an_address_wins() {
        let project = coordinate();
        let events = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Old", Some("builder"), &project),
            ),
            agent_event(
                &hex('1'),
                &hex('a'),
                20,
                content("Renamed", Some("builder"), &project),
            ),
        ];
        let rows = fold_project_agents(&project, roster(), &events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Renamed");

        // A newer publication that withdraws the association is not undone by
        // the older one that carried it.
        let other = format!("30621:{}:beta", hex('c'));
        let withdrawn = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Old", Some("builder"), &project),
            ),
            agent_event(
                &hex('1'),
                &hex('a'),
                20,
                content("Moved", Some("builder"), &other),
            ),
        ];
        assert!(fold_project_agents(&project, roster(), &withdrawn).is_empty());
    }

    #[test]
    fn an_agent_two_authors_claim_is_listed_once_by_the_newest_claim() {
        let project = coordinate();
        let events = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Older", Some("builder"), &project),
            ),
            agent_event(
                &hex('2'),
                &hex('a'),
                20,
                content("Newer", Some("builder"), &project),
            ),
        ];
        let rows = fold_project_agents(&project, roster(), &events);
        assert_eq!(rows.len(), 1, "one row per agent, not per author");
        assert_eq!(rows[0].name, "Newer");
        assert_eq!(rows[0].owner, hex('2'));
        assert_eq!(rows[0].owner_role, "collaborator");

        // Equal timestamps: the lowest event id wins.
        let mut tied = events.clone();
        tied[1]["created_at"] = json!(10);
        tied[0]["id"] = json!("0001");
        tied[1]["id"] = json!("0002");
        let rows = fold_project_agents(&project, roster(), &tied);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Older");
    }

    #[test]
    fn a_newer_claim_by_another_author_for_another_project_hides_this_one() {
        // Digest is matched after the newest claim per agent is chosen, as the
        // desktop does: the agent's newest claim says it is elsewhere.
        let project = coordinate();
        let other = format!("30621:{}:beta", hex('c'));
        let events = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Here", Some("builder"), &project),
            ),
            agent_event(
                &hex('2'),
                &hex('a'),
                20,
                content("There", Some("builder"), &other),
            ),
        ];
        assert!(fold_project_agents(&project, roster(), &events).is_empty());
    }

    #[test]
    fn an_unreadable_newer_event_does_not_supersede_a_readable_claim() {
        let project = coordinate();
        let events = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Readable", Some("builder"), &project),
            ),
            agent_event(&hex('1'), &hex('a'), 20, "not json".to_owned()),
        ];
        let rows = fold_project_agents(&project, roster(), &events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Readable");
    }

    #[test]
    fn another_projects_digest_is_excluded() {
        let project = coordinate();
        let other = format!("30621:{}:beta", hex('c'));
        let events = vec![
            agent_event(
                &hex('1'),
                &hex('a'),
                10,
                content("Theirs", Some("builder"), &other),
            ),
            agent_event(
                &hex('1'),
                &hex('b'),
                10,
                content("Nobody's", Some("builder"), ""),
            ),
        ];
        assert!(fold_project_agents(&project, roster(), &events).is_empty());
    }

    #[test]
    fn malformed_content_is_skipped() {
        let project = coordinate();
        let digest = project_agent_digest(&project).expect("digest");
        let events = vec![
            agent_event(&hex('1'), &hex('a'), 10, "not json".to_owned()),
            agent_event(&hex('1'), &hex('b'), 10, json!(["array"]).to_string()),
            agent_event(
                &hex('1'),
                &hex('d'),
                10,
                json!({ "project_digest": digest }).to_string(),
            ),
            agent_event(
                &hex('1'),
                "not-a-pubkey",
                10,
                content("Bad d", None, &project),
            ),
            agent_event(&hex('1'), &hex('e'), 10, content("Good", None, &project)),
        ];
        let rows = fold_project_agents(&project, roster(), &events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Good");
        assert_eq!(rows[0].role, None);
    }

    #[test]
    fn role_less_agents_sort_after_roles() {
        let project = coordinate();
        let events = vec![
            agent_event(&hex('1'), &hex('a'), 10, content("Aaron", None, &project)),
            agent_event(
                &hex('1'),
                &hex('b'),
                10,
                content("Zed", Some("verifier"), &project),
            ),
        ];
        let names: Vec<_> = fold_project_agents(&project, roster(), &events)
            .into_iter()
            .map(|row| row.name)
            .collect();
        assert_eq!(names, vec!["Zed", "Aaron"]);
    }

    #[test]
    fn resolution_prefers_project_then_slug_then_environment() {
        let caller = hex('f');
        let env = format!("30621:{}:from-env", hex('E'));
        assert_eq!(
            resolve_project_coordinate(None, None, Some(&coordinate()), Some(&env), &caller)
                .expect("project"),
            coordinate()
        );
        assert_eq!(
            resolve_project_coordinate(Some("alpha"), None, None, Some(&env), &caller)
                .expect("slug"),
            format!("30621:{caller}:alpha")
        );
        assert_eq!(
            resolve_project_coordinate(Some("alpha"), Some(&hex('c')), None, None, &caller)
                .expect("slug and owner"),
            coordinate()
        );
        assert_eq!(
            resolve_project_coordinate(None, None, None, Some(&env), &caller).expect("env"),
            format!("30621:{}:from-env", hex('e'))
        );
    }

    #[test]
    fn resolving_nothing_or_garbage_is_a_usage_error() {
        let caller = hex('f');
        for (slug, owner, project, env) in [
            (None, None, None, None),
            (None, None, None, Some("  ")),
            (None, None, None, Some("30617:abc:repo")),
            (None, None, Some("alpha"), None),
            (None, Some(hex('c')), None, None),
            (Some("alpha"), Some("ABC".to_owned()), None, None),
        ] {
            let result = resolve_project_coordinate(slug, owner.as_deref(), project, env, &caller);
            assert!(
                matches!(result, Err(CliError::Usage(_))),
                "{slug:?} {owner:?} {project:?} {env:?}: {:?}",
                result.map_err(|error| error.to_string())
            );
        }
        let long = "x".repeat(65);
        assert!(matches!(
            resolve_project_coordinate(Some(&long), None, None, None, &caller),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn every_format_carries_verified_and_compact_drops_only_the_owner_role() {
        let row = ProjectAgentRow {
            pubkey: hex('a'),
            name: "Ada".into(),
            role: Some("builder".into()),
            owner: hex('1'),
            owner_role: "owner".into(),
        };
        let full = render_row(&row, &crate::OutputFormat::Json);
        assert_eq!(
            full,
            json!({
                "pubkey": hex('a'),
                "name": "Ada",
                "role": "builder",
                "owner": hex('1'),
                "owner_role": "owner",
                "verified": true,
            })
        );
        let compact = render_row(&row, &crate::OutputFormat::Compact);
        assert_eq!(
            compact,
            json!({
                "pubkey": hex('a'),
                "name": "Ada",
                "role": "builder",
                "owner": hex('1'),
                "verified": true,
            })
        );
    }
}
