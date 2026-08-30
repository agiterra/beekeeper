//! `bee sessions rubric check` — is the lead's model rubric still true?
//!
//! Brian's ruling: *"I still think we need a rubric. They have worked the best
//! for me. We just need to make sure we update it from time to time, or
//! trigger an update to it."* A rubric is a good instrument and a stale one is
//! a quiet lie — it names a model nobody serves, or it silently omits a model
//! that arrived last week, and either way a lead reads it and picks wrong.
//!
//! So the rubric is written down, versioned, and **checked against the live
//! kind:44222 catalog**. This command is that check. It never edits the
//! rubric and it never translates an id: an id the catalog does not offer is
//! reported as not offered, and a catalog id no row names is reported as
//! unassigned. Both lists empty is the only clean result.
//!
//! # The rubric block
//!
//! The rubric lives in the lead role pack, at
//! `personas/roles/lead/skills/choose-model/SKILL.md`, inside a fenced block
//! whose info string starts with `rubric`:
//!
//! ~~~text
//! ```rubric v3
//! | tier | role(s) | provider | model id | reason |
//! | --- | --- | --- | --- | --- |
//! | deep | lead, architect | claude-primary | opus[1m] | long-context planning |
//! | fast | builder, runner | claude-primary | sonnet | throughput |
//! | any  | poker           | *              | haiku  | cheap adversarial passes |
//! ```
//! ~~~
//!
//! Five columns, in that order. The token after `rubric` on the fence line is
//! the rubric's version and is reported back; a block with no version is
//! accepted and reports `null`, because a missing version is a fact about the
//! rubric rather than a reason to refuse it. `*` in the provider column means
//! "whichever provider offers it". A model id may be wrapped in backticks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;

use super::catalog::load_catalogs;

/// Where the lead pack keeps its rubric, relative to a repository root.
pub const DEFAULT_RUBRIC_RELATIVE_PATH: &str = "personas/roles/lead/skills/choose-model/SKILL.md";

/// The provider column value meaning "whichever provider offers it".
const ANY_PROVIDER: &str = "*";

/// One rubric row: a tier, the roles it covers, and the model it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RubricRow {
    /// Free-form tier label (`deep`, `fast`, …).
    pub tier: String,
    /// Roles this row applies to, in the order written.
    pub roles: Vec<String>,
    /// `providerInstanceRef`, or [`ANY_PROVIDER`].
    pub provider: String,
    /// The catalog model id this row names.
    pub model: String,
    /// Why, verbatim.
    pub reason: String,
}

impl RubricRow {
    /// How this row is named in a report: `provider/model`.
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

/// A parsed rubric block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rubric {
    /// The token after `rubric` on the fence line, when there is one.
    pub version: Option<String>,
    /// The rows, in file order.
    pub rows: Vec<RubricRow>,
}

/// Why a rubric could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RubricParseError {
    /// No fenced block whose info string starts with `rubric`.
    NoRubricBlock,
    /// The block was found but held no data rows.
    NoRows,
    /// A row did not have the five columns.
    BadRow {
        /// 1-indexed line within the file.
        line: usize,
        /// The line, verbatim.
        text: String,
    },
}

impl std::fmt::Display for RubricParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRubricBlock => write!(
                formatter,
                "no ```rubric block found — the rubric is a fenced block whose info string starts with `rubric`, holding a | tier | role(s) | provider | model id | reason | table"
            ),
            Self::NoRows => write!(
                formatter,
                "the ```rubric block holds no rows below its header"
            ),
            Self::BadRow { line, text } => write!(
                formatter,
                "line {line} is not a five-column rubric row: {text:?}"
            ),
        }
    }
}

impl std::error::Error for RubricParseError {}

/// Parse the `rubric` block out of a skill document.
///
/// # Errors
///
/// [`RubricParseError`] names what was wrong, with the line when a line is to
/// blame.
pub fn parse_rubric(document: &str) -> Result<Rubric, RubricParseError> {
    let mut lines = document.lines().enumerate();
    let (version, fence) = loop {
        let Some((_, line)) = lines.next() else {
            return Err(RubricParseError::NoRubricBlock);
        };
        let trimmed = line.trim_start();
        let fence_char = match trimmed.chars().next() {
            Some('`') => '`',
            Some('~') => '~',
            _ => continue,
        };
        let run = trimmed.chars().take_while(|c| *c == fence_char).count();
        if run < 3 {
            continue;
        }
        let info = trimmed[run..].trim();
        let mut words = info.split_whitespace();
        if words.next() != Some("rubric") {
            continue;
        }
        break (
            words.next().map(str::to_owned),
            fence_char.to_string().repeat(run),
        );
    };

    let mut rows = Vec::new();
    let mut header_seen = false;
    for (index, line) in lines {
        let trimmed = line.trim();
        if trimmed.starts_with(&fence) {
            break;
        }
        if !trimmed.starts_with('|') {
            // Blank lines and prose inside the block are ignored; only table
            // rows carry rubric content.
            if trimmed.is_empty() {
                continue;
            }
            return Err(RubricParseError::BadRow {
                line: index + 1,
                text: line.to_owned(),
            });
        }
        let cells = split_row(trimmed);
        if is_separator(&cells) {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        if cells.len() != 5 {
            return Err(RubricParseError::BadRow {
                line: index + 1,
                text: line.to_owned(),
            });
        }
        let model = unwrap_code(&cells[3]);
        let provider = unwrap_code(&cells[2]);
        if model.is_empty() || provider.is_empty() {
            return Err(RubricParseError::BadRow {
                line: index + 1,
                text: line.to_owned(),
            });
        }
        rows.push(RubricRow {
            tier: cells[0].clone(),
            roles: cells[1]
                .split(',')
                .map(|role| unwrap_code(role.trim()))
                .filter(|role| !role.is_empty())
                .collect(),
            provider,
            model,
            reason: cells[4].clone(),
        });
    }
    if rows.is_empty() {
        return Err(RubricParseError::NoRows);
    }
    Ok(Rubric { version, rows })
}

/// Split `| a | b |` into its trimmed cells.
fn split_row(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// `| --- | :-: |` — a markdown alignment row, not data.
fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' '))
}

/// Strip one layer of backticks a markdown author wrapped a value in.
fn unwrap_code(cell: &str) -> String {
    cell.trim().trim_matches('`').trim().to_owned()
}

/// The result of comparing a rubric to a catalog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RubricCheck {
    /// `provider/model` labels the rubric names that the catalog does not
    /// offer, sorted and deduplicated.
    pub not_offered: Vec<String>,
    /// `provider/model` labels the catalog offers that no rubric row names,
    /// sorted and deduplicated.
    pub unassigned: Vec<String>,
}

impl RubricCheck {
    /// `true` when the rubric and the catalog agree exactly.
    pub fn is_fresh(&self) -> bool {
        self.not_offered.is_empty() && self.unassigned.is_empty()
    }
}

/// Compare a rubric's rows to the catalog's `(provider, model)` pairs.
///
/// A row whose provider is `*` matches the id on any provider, and covers it
/// on every provider that offers it — a rubric that says "haiku, wherever you
/// find it" has assigned haiku everywhere and is not stale for it.
pub fn check_rubric(rows: &[RubricRow], offered: &[(String, String)]) -> RubricCheck {
    let mut not_offered: BTreeSet<String> = BTreeSet::new();
    let mut assigned: BTreeSet<(String, String)> = BTreeSet::new();
    for row in rows {
        let matches: Vec<&(String, String)> = offered
            .iter()
            .filter(|(provider, model)| {
                *model == row.model && (row.provider == ANY_PROVIDER || *provider == row.provider)
            })
            .collect();
        if matches.is_empty() {
            not_offered.insert(row.label());
            continue;
        }
        for pair in matches {
            assigned.insert(pair.clone());
        }
    }
    let unassigned: BTreeSet<String> = offered
        .iter()
        .filter(|pair| !assigned.contains(*pair))
        .map(|(provider, model)| format!("{provider}/{model}"))
        .collect();
    RubricCheck {
        not_offered: not_offered.into_iter().collect(),
        unassigned: unassigned.into_iter().collect(),
    }
}

/// Find the rubric document: the explicit path, or the nearest ancestor of the
/// working directory that holds [`DEFAULT_RUBRIC_RELATIVE_PATH`].
///
/// Walking up is what lets a seat run this from a worktree subdirectory. The
/// error names every directory that was tried, because "rubric not found" with
/// no path is the kind of message that costs an hour.
pub fn resolve_rubric_path(explicit: Option<&str>, start: &Path) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(CliError::NotFound(format!(
                "no rubric at {}",
                path.display()
            )))
        };
    }
    let mut tried = Vec::new();
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(DEFAULT_RUBRIC_RELATIVE_PATH);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }
    Err(CliError::NotFound(format!(
        "no rubric found — pass --rubric <path>, or run from a checkout holding {DEFAULT_RUBRIC_RELATIVE_PATH}. Tried: {}",
        tried.join(", ")
    )))
}

/// `bee sessions rubric check --channel <uuid> [--rubric <path>]`.
pub async fn cmd_rubric_check(
    client: &BuzzClient,
    channel_id: &str,
    rubric_path: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = resolve_rubric_path(rubric_path, &cwd)?;
    let document = std::fs::read_to_string(&path)
        .map_err(|error| CliError::Other(format!("cannot read {}: {error}", path.display())))?;
    let rubric = parse_rubric(&document)
        .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))?;

    let snapshot = load_catalogs(client, channel_id).await?;
    let offered = snapshot.offered_pairs();
    let check = check_rubric(&rubric.rows, &offered);

    let catalogs: Vec<Value> = snapshot
        .records
        .iter()
        .map(|record| {
            json!({
                "signer": record.signer,
                "revision": record.catalog.revision,
                "eventId": record.event_id,
            })
        })
        .collect();
    // One revision only when one signer published: with two hosts there is no
    // shared clock and therefore no single number, and printing one anyway
    // would name a revision nothing has.
    let catalog_revision = match snapshot.records.as_slice() {
        [only] => json!(only.catalog.revision),
        _ => Value::Null,
    };

    let report = json!({
        "rubric": path.display().to_string(),
        "rubricVersion": rubric.version,
        "rows": rubric.rows.len(),
        "channel": channel_id,
        "catalogRevision": catalog_revision,
        "catalogs": catalogs,
        "offered": offered
            .iter()
            .map(|(provider, model)| format!("{provider}/{model}"))
            .collect::<Vec<String>>(),
        "notOffered": check.not_offered,
        "unassigned": check.unassigned,
        "stale": !check.is_fresh(),
        "malformedCatalogs": snapshot
            .malformed
            .iter()
            .map(|(event_id, reason)| json!({ "eventId": event_id, "reason": reason }))
            .collect::<Vec<Value>>(),
    });

    match format {
        crate::OutputFormat::Compact => println!(
            "{}",
            json!({
                "rubricVersion": rubric.version,
                "notOffered": check.not_offered,
                "unassigned": check.unassigned,
                "stale": !check.is_fresh(),
            })
        ),
        crate::OutputFormat::Json => println!("{report}"),
    }

    if check.is_fresh() {
        return Ok(());
    }
    Err(CliError::Other(format!(
        "rubric is stale: {} not offered, {} unassigned",
        check.not_offered.len(),
        check.unassigned.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"# Choose a model

Some prose.

```rubric v3
| tier | role(s) | provider | model id | reason |
| --- | --- | --- | --- | --- |
| deep | lead, architect | claude-primary | opus[1m] | long-context planning |
| fast | builder, runner | claude-primary | `sonnet` | throughput |
| any | poker | * | haiku | cheap adversarial passes |
```

More prose.
"#;

    fn offered(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(provider, model)| ((*provider).to_owned(), (*model).to_owned()))
            .collect();
        pairs.sort();
        pairs
    }

    #[test]
    fn the_block_is_parsed_with_its_version_and_rows() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        assert_eq!(rubric.version.as_deref(), Some("v3"));
        assert_eq!(rubric.rows.len(), 3);
        assert_eq!(rubric.rows[0].tier, "deep");
        assert_eq!(rubric.rows[0].roles, vec!["lead", "architect"]);
        assert_eq!(rubric.rows[0].provider, "claude-primary");
        assert_eq!(rubric.rows[0].model, "opus[1m]");
        assert_eq!(rubric.rows[0].reason, "long-context planning");
        // Backticks are markdown, not part of the id.
        assert_eq!(rubric.rows[1].model, "sonnet");
        assert_eq!(rubric.rows[2].provider, "*");
    }

    /// A rubric with no version is still a rubric; the check reports the
    /// missing version rather than refusing to run.
    #[test]
    fn a_block_without_a_version_parses_and_reports_none() {
        let document = "```rubric\n| tier | role(s) | provider | model id | reason |\n| - | - | - | - | - |\n| a | lead | p | m | r |\n```\n";
        let rubric = parse_rubric(document).expect("parse");
        assert_eq!(rubric.version, None);
        assert_eq!(rubric.rows.len(), 1);
    }

    #[test]
    fn a_document_with_no_rubric_block_says_so() {
        let document = "# No rubric here\n\n```json\n{}\n```\n";
        assert_eq!(parse_rubric(document), Err(RubricParseError::NoRubricBlock));
        // A fence whose info merely *contains* rubric is not the block.
        let near = "```not-rubric\n| a | b | c | d | e |\n```\n";
        assert_eq!(parse_rubric(near), Err(RubricParseError::NoRubricBlock));
    }

    #[test]
    fn a_block_with_only_a_header_has_no_rows() {
        let document =
            "```rubric v1\n| tier | role(s) | provider | model id | reason |\n| - | - | - | - | - |\n```\n";
        assert_eq!(parse_rubric(document), Err(RubricParseError::NoRows));
    }

    #[test]
    fn a_short_row_names_its_line() {
        let document = "```rubric v1\n| tier | role(s) | provider | model id | reason |\n| - | - | - | - | - |\n| a | lead | p |\n```\n";
        assert!(matches!(
            parse_rubric(document),
            Err(RubricParseError::BadRow { line: 4, .. })
        ));
    }

    /// The clean result, and the only one that exits 0.
    #[test]
    fn a_rubric_matching_the_catalog_is_fresh() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(
            &rubric.rows,
            &offered(&[
                ("claude-primary", "opus[1m]"),
                ("claude-primary", "sonnet"),
                ("claude-primary", "haiku"),
            ]),
        );
        assert_eq!(check, RubricCheck::default());
        assert!(check.is_fresh());
    }

    /// The two ways a rubric goes stale, both reported by name — never
    /// repaired, and never translated onto a neighbouring id.
    #[test]
    fn an_unoffered_row_and_an_unnamed_model_are_both_reported() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(
            &rubric.rows,
            &offered(&[
                ("claude-primary", "sonnet"),
                ("claude-primary", "haiku"),
                ("claude-primary", "claude-fable-5[1m]"),
            ]),
        );
        assert_eq!(check.not_offered, vec!["claude-primary/opus[1m]"]);
        assert_eq!(check.unassigned, vec!["claude-primary/claude-fable-5[1m]"]);
        assert!(!check.is_fresh());
    }

    /// `*` covers the id wherever it is offered — a rubric that says "haiku,
    /// whichever provider has it" has assigned haiku on both.
    #[test]
    fn a_wildcard_provider_covers_every_provider_offering_the_id() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(
            &rubric.rows,
            &offered(&[
                ("claude-primary", "opus[1m]"),
                ("claude-primary", "sonnet"),
                ("claude-primary", "haiku"),
                ("second-host", "haiku"),
            ]),
        );
        assert!(check.is_fresh(), "unexpected: {check:?}");
    }

    /// A row naming the right id on the wrong provider is not offered — the
    /// pair is the coordinate a create names, so half a match is no match.
    #[test]
    fn a_row_on_a_provider_that_does_not_offer_the_id_is_not_offered() {
        let rows = vec![RubricRow {
            tier: "deep".into(),
            roles: vec!["lead".into()],
            provider: "codex-primary".into(),
            model: "opus[1m]".into(),
            reason: "…".into(),
        }];
        let check = check_rubric(&rows, &offered(&[("claude-primary", "opus[1m]")]));
        assert_eq!(check.not_offered, vec!["codex-primary/opus[1m]"]);
        assert_eq!(check.unassigned, vec!["claude-primary/opus[1m]"]);
    }

    /// An empty catalog is not a fresh rubric: every row names something
    /// nothing is serving, and the check says so rather than passing quietly
    /// because there was nothing to compare against.
    #[test]
    fn an_empty_catalog_makes_every_row_not_offered() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(&rubric.rows, &[]);
        assert_eq!(check.not_offered.len(), 3);
        assert!(check.unassigned.is_empty());
        assert!(!check.is_fresh());
    }

    #[test]
    fn an_explicit_path_that_does_not_exist_is_named() {
        let error =
            resolve_rubric_path(Some("/nope/rubric.md"), Path::new("/tmp")).expect_err("must fail");
        assert!(
            error.to_string().contains("/nope/rubric.md"),
            "unexpected: {error}"
        );
    }

    #[test]
    fn the_default_path_is_found_by_walking_up_from_the_working_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let rubric = root.join(DEFAULT_RUBRIC_RELATIVE_PATH);
        std::fs::create_dir_all(rubric.parent().expect("parent")).expect("mkdir");
        std::fs::write(&rubric, DOCUMENT).expect("write");
        let nested = root.join("crates").join("buzz-cli");
        std::fs::create_dir_all(&nested).expect("mkdir");
        assert_eq!(resolve_rubric_path(None, &nested).expect("resolve"), rubric);
    }

    #[test]
    fn a_missing_default_rubric_names_the_paths_it_tried() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_rubric_path(None, dir.path()).expect_err("must fail");
        assert!(
            error.to_string().contains(DEFAULT_RUBRIC_RELATIVE_PATH),
            "unexpected: {error}"
        );
    }
}
