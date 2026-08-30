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
//! # A bracket suffix is a variant of its base
//!
//! `gpt-5.6-sol[high]`, `[low]`, `[max]`, `[ultra]` are one model at four
//! effort levels; `opus[1m]` and `opus` are one model at two context windows.
//! The bracket is a knob on a model, not a different model, so **a rubric
//! decides at the base**: a row naming `gpt-5.6-terra[high]` has assigned every
//! `gpt-5.6-terra` row in the catalog, and a row naming `opus[1m]` has assigned
//! bare `opus` too. Without that rule this check was permanently red on a
//! real host — 41 unassigned ids on 2026-08-30, every one of them an effort
//! level of a model the rubric had already ruled on — and a check that is
//! always red is a check nobody reads.
//!
//! Collapsing hides nothing: every offered id no row names literally is listed
//! under `variants`, which is informational and never sets `stale`. The
//! `default` alias is not a model and is never unassigned; `defaultResolvesTo`
//! says what it points at on each provider.
//!
//! Five columns, in that order. The token after `rubric` on the fence line is
//! the rubric's version and is reported back; a block with no version is
//! accepted and reports `null`, because a missing version is a fact about the
//! rubric rather than a reason to refuse it. `*` in the provider column means
//! "whichever provider offers it". A model id may be wrapped in backticks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;

use super::catalog::load_catalogs;

/// Where the lead pack keeps its rubric, relative to a repository root.
pub const DEFAULT_RUBRIC_RELATIVE_PATH: &str = "personas/roles/lead/skills/choose-model/SKILL.md";

/// The provider column value meaning "whichever provider offers it".
const ANY_PROVIDER: &str = "*";

/// The runtime alias every provider publishes for "whatever this host is set
/// to". It is never a model, so it is never something a rubric row can assign.
const DEFAULT_ALIAS: &str = "default";

/// The recorded live catalog both implementations are checked against.
#[cfg(test)]
const RUBRIC_FIXTURE_RELATIVE_PATH: &str = "testdata/rubric/live-catalog-665076ce.json";

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

/// A model id with its bracket suffix removed: the base model the variant is a
/// setting of.
///
/// `gpt-5.6-sol[high]` and `gpt-5.6-sol[max]` are one model at two effort
/// levels; `opus[1m]` and `opus` are one model at two context windows. The
/// bracket is a knob, not a different model, so the rubric decides at the base
/// and the check reads it that way.
fn base_id(model: &str) -> &str {
    model.split_once('[').map_or(model, |(base, _suffix)| base)
}

/// Does a rubric row's provider column apply to this provider?
fn provider_matches(row_provider: &str, provider: &str) -> bool {
    row_provider == ANY_PROVIDER || row_provider == provider
}

/// The result of comparing a rubric to a catalog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RubricCheck {
    /// `provider/model` labels the rubric names that the catalog does not
    /// offer **under that exact id**, sorted and deduplicated.
    pub not_offered: Vec<String>,
    /// Labels for the models the catalog offers that no rubric row covers,
    /// sorted and deduplicated — one entry per uncovered base, not one per
    /// variant, so a rubric that has not decided about `gpt-5.6-sol` shows one
    /// gap rather than six. Each entry names an id the catalog actually offers,
    /// so it can be pasted into a new rubric row as it stands.
    pub unassigned: Vec<String>,
    /// `provider/model` labels the base rule absorbed — offered ids no row
    /// names literally, that are neither the `default` alias nor themselves
    /// reported in [`Self::unassigned`]. Informational: this list never makes a
    /// rubric stale, and exists so collapsing to the base hides nothing.
    pub variants: Vec<String>,
}

impl RubricCheck {
    /// `true` when the rubric and the catalog agree.
    ///
    /// [`Self::variants`] is deliberately not consulted: a variant is a setting
    /// of a model the rubric already ruled on, and treating one as staleness is
    /// how a check that is always red trains its reader to ignore it.
    pub fn is_fresh(&self) -> bool {
        self.not_offered.is_empty() && self.unassigned.is_empty()
    }
}

/// Compare a rubric's rows to the catalog's `(provider, model)` pairs.
///
/// A row whose provider is `*` matches the id on any provider, and covers it
/// on every provider that offers it — a rubric that says "haiku, wherever you
/// find it" has assigned haiku everywhere and is not stale for it.
///
/// # The two directions are deliberately not symmetric
///
/// * **Not offered is exact.** A create names one id and the relay refuses
///   anything else, so a row naming `opus[1m]` when the catalog offers only
///   `opus[500k]` is a row that cannot be hired from. Exact, always.
/// * **Unassigned is by base.** A catalog id is covered when any row names its
///   base or any variant of its base: the rubric names `gpt-5.6-terra[high]`
///   and the catalog's six terra rows are all decisions the rubric has already
///   made. The gap is reported once, at the base.
///
/// The [`DEFAULT_ALIAS`] is never unassigned. It is the provider's own pointer
/// at whatever model the host is set to, not a model a rubric row could name;
/// see [`super::catalog::CatalogSnapshot::default_models`] for what it resolves
/// to on each provider.
pub fn check_rubric(rows: &[RubricRow], offered: &[(String, String)]) -> RubricCheck {
    let mut not_offered: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        let offered_here = offered.iter().any(|(provider, model)| {
            *model == row.model && provider_matches(&row.provider, provider)
        });
        if !offered_here {
            not_offered.insert(row.label());
        }
    }

    let mut uncovered: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut variants: BTreeSet<String> = BTreeSet::new();
    for (provider, model) in offered {
        if model.eq_ignore_ascii_case(DEFAULT_ALIAS) {
            continue;
        }
        let covered = rows.iter().any(|row| {
            provider_matches(&row.provider, provider) && base_id(&row.model) == base_id(model)
        });
        if !covered {
            uncovered
                .entry((provider.clone(), base_id(model).to_owned()))
                .or_default()
                .insert(model.clone());
        }
        let named_exactly = rows
            .iter()
            .any(|row| provider_matches(&row.provider, provider) && row.model == *model);
        if !named_exactly {
            variants.insert(format!("{provider}/{model}"));
        }
    }
    // One gap per base, named with an id the catalog really offers: the bare
    // base when it is on offer, otherwise the first variant of it. Reporting a
    // bare base nobody serves would send a reader to add a row that this same
    // check would then call not offered.
    let unassigned: BTreeSet<String> = uncovered
        .into_iter()
        .map(|((provider, base), ids)| {
            let representative = if ids.contains(&base) {
                base
            } else {
                ids.iter().next().cloned().unwrap_or(base)
            };
            format!("{provider}/{representative}")
        })
        .collect();
    // A base id already reported as unassigned is not additionally a variant
    // the collapse hid — it is right there in the other list.
    for label in &unassigned {
        variants.remove(label);
    }

    RubricCheck {
        not_offered: not_offered.into_iter().collect(),
        unassigned: unassigned.into_iter().collect(),
        variants: variants.into_iter().collect(),
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
    // What each provider's `default` alias points at. The check never counts
    // the alias as unassigned, so this is where a reader sees which id it is —
    // including the case where a provider's own default is the alias itself.
    let default_resolves_to: Value = snapshot
        .default_models()
        .into_iter()
        .map(|(provider, model)| (provider, model.map_or(Value::Null, Value::String)))
        .collect::<serde_json::Map<String, Value>>()
        .into();
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
        "defaultResolvesTo": default_resolves_to,
        "notOffered": check.not_offered,
        "unassigned": check.unassigned,
        "variants": check.variants,
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
                "defaultResolvesTo": default_resolves_to,
                "notOffered": check.not_offered,
                "unassigned": check.unassigned,
                "variants": check.variants,
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

    /// The rule the whole fix turns on: a row naming one effort level has
    /// decided about the model, so its siblings are not gaps.
    #[test]
    fn a_row_naming_one_variant_covers_every_variant_of_that_base() {
        let rows = vec![RubricRow {
            tier: "tier-2".into(),
            roles: vec!["builder".into()],
            provider: "codex-primary".into(),
            model: "gpt-5.6-terra[high]".into(),
            reason: "…".into(),
        }];
        let check = check_rubric(
            &rows,
            &offered(&[
                ("codex-primary", "gpt-5.6-terra"),
                ("codex-primary", "gpt-5.6-terra[high]"),
                ("codex-primary", "gpt-5.6-terra[low]"),
                ("codex-primary", "gpt-5.6-terra[ultra]"),
            ]),
        );
        assert!(check.is_fresh(), "unexpected: {check:?}");
        // Nothing is hidden by the collapse: the three ids no row names
        // literally are listed, and they do not make the rubric stale.
        assert_eq!(
            check.variants,
            vec![
                "codex-primary/gpt-5.6-terra",
                "codex-primary/gpt-5.6-terra[low]",
                "codex-primary/gpt-5.6-terra[ultra]",
            ]
        );
    }

    /// And the other direction: a row naming `opus[1m]` has decided about bare
    /// `opus`, because the context window is a setting, not a second model.
    #[test]
    fn a_row_naming_a_context_variant_covers_the_bare_base() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(
            &rubric.rows,
            &offered(&[
                ("claude-primary", "opus"),
                ("claude-primary", "opus[1m]"),
                ("claude-primary", "sonnet"),
                ("claude-primary", "haiku"),
            ]),
        );
        assert!(check.is_fresh(), "unexpected: {check:?}");
        assert_eq!(check.variants, vec!["claude-primary/opus"]);
    }

    /// A gap is reported once, at an id the catalog really offers — paste it
    /// into a row and the same check must not then call it not offered.
    #[test]
    fn an_uncovered_base_is_reported_once_with_an_id_the_catalog_offers() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let mut catalog = vec![
            ("claude-primary", "opus[1m]"),
            ("claude-primary", "sonnet"),
            ("claude-primary", "haiku"),
        ];
        catalog.extend([
            ("codex-primary", "gpt-5.6-sol"),
            ("codex-primary", "gpt-5.6-sol[high]"),
            ("codex-primary", "gpt-5.6-sol[max]"),
        ]);
        let check = check_rubric(&rubric.rows, &offered(&catalog));
        assert_eq!(check.unassigned, vec!["codex-primary/gpt-5.6-sol"]);
        assert!(!check.is_fresh());

        // The founder adds exactly the row the check named; the gap closes and
        // the new row is not reported as unoffered.
        let mut rows = rubric.rows.clone();
        rows.push(RubricRow {
            tier: "tier-2".into(),
            roles: vec!["builder".into()],
            provider: "codex-primary".into(),
            model: "gpt-5.6-sol".into(),
            reason: "…".into(),
        });
        let after = check_rubric(&rows, &offered(&catalog));
        assert!(after.is_fresh(), "unexpected: {after:?}");
    }

    /// When only a variant is on offer, the gap is named with that variant —
    /// never with a bare base id nothing serves.
    #[test]
    fn a_base_the_catalog_never_offers_bare_is_reported_as_its_variant() {
        let check = check_rubric(&[], &offered(&[("claude-primary", "claude-fable-5[1m]")]));
        assert_eq!(check.unassigned, vec!["claude-primary/claude-fable-5[1m]"]);
        assert!(check.variants.is_empty());
    }

    /// `default` is the provider's pointer at whatever the host is set to. It
    /// is not a model, so no rubric row can assign it and it is never a gap.
    #[test]
    fn the_default_alias_is_never_unassigned() {
        let rubric = parse_rubric(DOCUMENT).expect("parse");
        let check = check_rubric(
            &rubric.rows,
            &offered(&[
                ("claude-primary", "default"),
                ("claude-primary", "opus[1m]"),
                ("claude-primary", "sonnet"),
                ("claude-primary", "haiku"),
                ("goose-primary", "default"),
            ]),
        );
        assert!(check.is_fresh(), "unexpected: {check:?}");
        assert!(check.variants.is_empty());
    }

    /// The cross-implementation contract, on the catalog this relay really
    /// served: the Rust check and the desktop mirror must produce the same two
    /// lists for one recorded input.
    #[test]
    fn the_live_catalog_fixture_produces_the_recorded_lists() {
        let fixture = load_fixture();
        let rubric =
            parse_rubric(fixture["rubricBlock"].as_str().expect("rubricBlock")).expect("parse");
        let offered: Vec<(String, String)> = fixture["offered"]
            .as_array()
            .expect("offered")
            .iter()
            .map(|pair| {
                (
                    pair["providerInstanceRef"]
                        .as_str()
                        .expect("provider")
                        .to_owned(),
                    pair["model"].as_str().expect("model").to_owned(),
                )
            })
            .collect();
        let expected = &fixture["expected"];
        let check = check_rubric(&rubric.rows, &offered);
        assert_eq!(check.not_offered, strings(&expected["notOffered"]));
        assert_eq!(check.unassigned, strings(&expected["unassigned"]));
        assert_eq!(check.variants, strings(&expected["variants"]));
        // Every offered id lands in exactly one bucket: the `default` alias,
        // a row's literal id, an unassigned base, or a variant. Nothing is
        // dropped on the way to a shorter list.
        let aliases = offered
            .iter()
            .filter(|(_, model)| model.eq_ignore_ascii_case(DEFAULT_ALIAS))
            .count();
        let named = offered
            .iter()
            .filter(|(provider, model)| {
                !model.eq_ignore_ascii_case(DEFAULT_ALIAS)
                    && rubric
                        .rows
                        .iter()
                        .any(|row| provider_matches(&row.provider, provider) && row.model == *model)
            })
            .count();
        assert_eq!(
            aliases + named + check.unassigned.len() + check.variants.len(),
            offered.len(),
            "the buckets must partition the catalog"
        );
    }

    fn load_fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(RUBRIC_FIXTURE_RELATIVE_PATH);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        serde_json::from_str(&text).expect("fixture is JSON")
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|entry| entry.as_str().expect("string").to_owned())
            .collect()
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

    /// The cross-lane contract: the rubric this repository actually ships must
    /// parse with the parser this command actually runs, and must carry the
    /// version its own prose claims. A rubric whose fence forgot its version
    /// reports `rubricVersion: null` while the page above it says "Version 3",
    /// and a reader then cannot tell which of the two is stale.
    #[test]
    fn the_shipped_rubric_parses_and_carries_its_version() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(DEFAULT_RUBRIC_RELATIVE_PATH);
        let document = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let rubric = parse_rubric(&document)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()));
        assert!(
            rubric.version.is_some(),
            "the shipped rubric has no version on its fence line, so `rubric check` reports null"
        );
        assert!(!rubric.rows.is_empty(), "the shipped rubric has no rows");
        // Every row names a concrete id: no aliases, no "default", no blanks.
        for row in &rubric.rows {
            assert!(
                !row.model.eq_ignore_ascii_case("default"),
                "row {:?} names \"default\" instead of a catalog id",
                row.tier
            );
            assert!(!row.roles.is_empty(), "row {:?} names no role", row.tier);
        }
    }
}
