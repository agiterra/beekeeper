//! Read-only MCP projection of one launcher-selected coding-session package
//! directory.
//!
//! The package directory is accepted only at process startup. Tool calls
//! cannot redirect the server outside it, fetch relay data, sign events, or
//! write provider-native state. Within that directory the server serves the
//! newest package the launcher wrote, re-validated in full before it is
//! served.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{PoisonError, RwLock, RwLockReadGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_context::{
    coding_session_first_turn_brief, CodingSessionContextHistoryItem, CodingSessionContextPackage,
    CodingSessionContextProvenance, CodingSessionContextRole, MAX_CONTEXT_HISTORY_CONTENT_BYTES,
    MAX_CONTEXT_HISTORY_ITEMS, MAX_CONTEXT_INBOX_ITEMS, MAX_CONTEXT_PACKAGE_BYTES,
};
use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

/// Launcher-only path to one private, immutable context package.
pub const SESSION_CONTEXT_PACKAGE_ENV: &str = "BEEKEEPER_SESSION_CONTEXT_PACKAGE";
/// Launcher-only directory holding one execution's package generations.
///
/// Preferred over [`SESSION_CONTEXT_PACKAGE_ENV`]: the launching provider may
/// write a newer verified generation into this directory while the session
/// runs, and this server serves the newest generation it can fully validate.
pub const SESSION_CONTEXT_PACKAGE_DIR_ENV: &str = "BEEKEEPER_SESSION_CONTEXT_PACKAGE_DIR";
/// Launcher-only `cs-target` key of the execution this server serves.
///
/// A public wire identity, not a credential: every event that execution
/// publishes already carries it. Without it `session_inbox` cannot tell which
/// of the umbrella's commands were addressed to *this* seat, and says so
/// rather than paging a sibling's mail.
pub const SESSION_CONTEXT_SELF_TARGET_ENV: &str = "BEEKEEPER_SESSION_CONTEXT_SELF_TARGET";

const DEFAULT_HISTORY_LIMIT: usize = 200;
/// The package ceiling itself, imported rather than restated, so the page cap
/// and the package cap can never drift apart again.
const MAX_HISTORY_LIMIT: usize = MAX_CONTEXT_HISTORY_ITEMS;
const DEFAULT_INBOX_LIMIT: usize = 50;
/// The package's own inbox ceiling, imported rather than restated.
const MAX_INBOX_LIMIT: usize = MAX_CONTEXT_INBOX_ITEMS;
const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 200;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const MAX_INLINE_CONTENT_BYTES: usize = MAX_CONTEXT_HISTORY_CONTENT_BYTES;
const MAX_SEARCH_SNIPPET_BYTES: usize = 2 * 1024;
/// One shared response byte budget for every paged tool, measured against the
/// rendered (pretty-printed) bytes of the items a page carries.
const MAX_HISTORY_PAGE_BYTES: usize = 128 * 1024;
const MAX_SEARCH_PAGE_BYTES: usize = 128 * 1024;
const MAX_INDEX_PREVIEW_BYTES: usize = 64;
/// Age at which a served snapshot is labelled stale.
///
/// This is a disclosure trigger, not a correctness boundary: the raw ages are
/// reported beside it so a reader can disagree with the threshold.
const STALE_AFTER_MS: i64 = 15 * 60 * 1000;
/// Digits in a generation file name (`<seq:010>.json`).
const GENERATION_FILE_DIGITS: usize = 10;
/// Honest label for what a package refresh does to offsets.
const OFFSET_STABILITY: &str = "unstable_across_refresh";

/// Empty arguments for the session overview tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionOverviewParams {}

/// Item detail level for one `session_history` page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SessionHistoryView {
    /// Whole verified items, including structured content.
    #[default]
    Full,
    /// Metadata only, so many more items fit in one page.
    Index,
}

/// Pagination arguments for verified session history.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryParams {
    /// Cursor: the signed source event id of the last item already read. The
    /// page starts at the item after it. Stable across a package refresh,
    /// which offsets are not.
    #[serde(default)]
    pub since: Option<String>,
    /// Item detail level, "full" (default) or "index".
    #[serde(default)]
    pub view: Option<SessionHistoryView>,
    /// Zero-based history-item offset. Defaults to zero.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Number of history items to return. Defaults to 200; maximum 4096. A
    /// page also ends at the shared response byte budget, whichever comes
    /// first.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Pagination arguments for this execution's verified command inbox.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionInboxParams {
    /// Cursor: the signed source event id of the last command already read.
    /// The page starts at the command after it.
    #[serde(default)]
    pub since: Option<String>,
    /// Number of commands to return. Defaults to 50; maximum 256.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Search arguments for verified session history.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSessionParams {
    /// Non-empty text query, at most 256 UTF-8 bytes.
    pub query: String,
    /// Zero-based offset into matching items. Defaults to zero.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Number of matching items to return. Defaults to 50; maximum 200. A page
    /// also ends at the shared response byte budget, whichever comes first.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One fully validated package generation, plus its event-id address book.
struct LoadedPackage {
    seq: u64,
    package: CodingSessionContextPackage,
    /// Signed source event id to history offset. `validate()` already proved
    /// event ids are unique within a package.
    index: HashMap<String, usize>,
}

impl LoadedPackage {
    fn new(seq: u64, package: CodingSessionContextPackage) -> Self {
        let index = package
            .history
            .iter()
            .enumerate()
            .map(|(offset, item)| (item.event_id.clone(), offset))
            .collect();
        Self {
            seq,
            package,
            index,
        }
    }
}

/// What one served response says about the generation behind it.
///
/// The generation itself is deliberately **not** carried here: it is read off
/// the same guard that produced the response body, so an envelope can never
/// name a generation the payload did not come from.
struct RefreshOutcome {
    changed: bool,
    refused: bool,
}

pub(crate) struct SessionContextState {
    loaded: RwLock<LoadedPackage>,
    /// Launcher-chosen generation directory, absent in legacy single-file mode.
    dir: Option<PathBuf>,
    /// The `cs-target` key this server serves, when the launcher named one.
    self_target: Option<String>,
    /// Generation named by the previous response this server rendered.
    ///
    /// rmcp dispatches every inbound request on its own task, and agents issue
    /// parallel tool calls, so "changed since the previous call" cannot be
    /// derived from whether *this* task performed the reload — another task's
    /// reload changes provenance under this one just the same. Comparing the
    /// generation actually served against the last one reported keeps the
    /// disclosure true whichever task installed the new package.
    last_reported_seq: AtomicU64,
}

impl SessionContextState {
    pub(crate) fn load_from_env() -> io::Result<Option<Self>> {
        let dir = std::env::var_os(SESSION_CONTEXT_PACKAGE_DIR_ENV);
        let file = std::env::var_os(SESSION_CONTEXT_PACKAGE_ENV);
        if dir.is_none() && file.is_none() {
            return Ok(None);
        }
        let mut directory_error = None;
        if let Some(raw) = dir {
            match Self::load_from_dir(Path::new(&raw)) {
                Ok(state) => return Ok(Some(state)),
                Err(error) => directory_error = Some(error),
            }
        }
        match (file, directory_error) {
            (Some(raw), _) => Self::load(Path::new(&raw)).map(Some),
            (None, Some(error)) => Err(error),
            (None, None) => Ok(None),
        }
    }

    /// Serve the newest generation in `dir` that passes the whole load
    /// gauntlet.
    ///
    /// A newer generation that fails any check is refused and the next one
    /// down is tried, because at startup there is no last-good package to keep
    /// serving; refusing everything would leave the agent with no verified
    /// context at all.
    fn load_from_dir(dir: &Path) -> io::Result<Self> {
        if !dir.is_absolute() {
            return Err(invalid_data(format!(
                "{SESSION_CONTEXT_PACKAGE_DIR_ENV} must name an absolute path"
            )));
        }
        let mut refusal = None;
        for (seq, path) in package_generations(dir)? {
            match load_package(&path) {
                Ok(package) => {
                    return Ok(Self {
                        loaded: RwLock::new(LoadedPackage::new(seq, package)),
                        dir: Some(dir.to_path_buf()),
                        self_target: self_target_from_env(),
                        last_reported_seq: AtomicU64::new(seq),
                    })
                }
                Err(error) => {
                    tracing::warn!(
                        generation = seq,
                        %error,
                        "refused a session context package generation"
                    );
                    refusal = Some(error);
                }
            }
        }
        Err(refusal.unwrap_or_else(|| {
            invalid_data(format!(
                "{SESSION_CONTEXT_PACKAGE_DIR_ENV} holds no loadable package generation"
            ))
        }))
    }

    fn load(path: &Path) -> io::Result<Self> {
        Ok(Self {
            loaded: RwLock::new(LoadedPackage::new(0, load_package(path)?)),
            dir: None,
            self_target: self_target_from_env(),
            last_reported_seq: AtomicU64::new(0),
        })
    }

    fn read(&self) -> RwLockReadGuard<'_, LoadedPackage> {
        self.loaded.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn generation(&self) -> u64 {
        self.read().seq
    }

    /// Acquire the guard one response is rendered from, and the disclosure
    /// that describes it.
    ///
    /// Both halves of a response — the envelope's generation and the items,
    /// provenance, staleness and cursors — come from this single guard. The
    /// two used to be taken separately, with the generation captured before
    /// the reload dropped its write lock and the payload read back afterwards;
    /// a concurrent tool call landing in that window made the response name a
    /// generation it had not served.
    fn serve(&self) -> (RwLockReadGuard<'_, LoadedPackage>, RefreshOutcome) {
        let refused = self.reload_if_newer();
        let loaded = self.read();
        let served = loaded.seq;
        let previous = self.last_reported_seq.swap(served, Ordering::SeqCst);
        (
            loaded,
            RefreshOutcome {
                changed: served != previous,
                refused,
            },
        )
    }

    /// Pick up a newer generation the launcher wrote, or keep serving the last
    /// good one. Reports whether a newer candidate was **refused**.
    ///
    /// A candidate that fails any check in [`load_package`] is refused, the
    /// loaded generation does not advance past it, and the response says so.
    /// The next successful launcher write lands at a higher sequence and
    /// supersedes it, so no descent loop is needed here.
    ///
    /// Whether this call installed the newer package is not reported: it is
    /// not the fact the response owes the agent. What changed under the agent
    /// is read off the served guard in [`SessionContextState::serve`], so a
    /// generation another task installed is disclosed here too.
    fn reload_if_newer(&self) -> bool {
        let current = self.generation();
        let Some(dir) = self.dir.as_deref() else {
            return false;
        };
        let newest = match package_generations(dir) {
            Ok(generations) => generations.into_iter().next(),
            Err(error) => {
                tracing::warn!(%error, "cannot list session context package generations");
                return true;
            }
        };
        let Some((seq, path)) = newest else {
            return false;
        };
        if seq <= current {
            return false;
        }
        match load_package(&path) {
            Ok(package) => {
                let mut guard = self.loaded.write().unwrap_or_else(PoisonError::into_inner);
                if seq > guard.seq {
                    *guard = LoadedPackage::new(seq, package);
                }
                false
            }
            Err(error) => {
                tracing::warn!(
                    generation = seq,
                    %error,
                    "refused a newer session context package generation"
                );
                true
            }
        }
    }
}

/// Generation files in `dir`, newest sequence first.
fn package_generations(dir: &Path) -> io::Result<Vec<(u64, PathBuf)>> {
    let entries = std::fs::read_dir(dir).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read session context package directory: {error}"),
        )
    })?;
    let mut generations = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read session context package directory entry: {error}"),
            )
        })?;
        if let Some(seq) = generation_sequence(&entry.file_name()) {
            generations.push((seq, entry.path()));
        }
    }
    generations.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
    Ok(generations)
}

fn generation_sequence(name: &OsStr) -> Option<u64> {
    let stem = name.to_str()?.strip_suffix(".json")?;
    if stem.len() != GENERATION_FILE_DIGITS || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

/// The whole load gauntlet, run unchanged on every candidate generation.
fn load_package(path: &Path) -> io::Result<CodingSessionContextPackage> {
    if !path.is_absolute() {
        return Err(invalid_data(format!(
            "{SESSION_CONTEXT_PACKAGE_ENV} must name an absolute path"
        )));
    }
    let path_metadata = std::fs::symlink_metadata(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot inspect session context package: {error}"),
        )
    })?;
    if path_metadata.file_type().is_symlink() {
        return Err(invalid_data(
            "session context package must not be a symbolic link",
        ));
    }

    let file = File::open(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot open session context package: {error}"),
        )
    })?;
    let open_metadata = file.metadata().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot inspect open session context package: {error}"),
        )
    })?;
    if !open_metadata.is_file() {
        return Err(invalid_data(
            "session context package must be a regular file",
        ));
    }
    validate_private_permissions(&path_metadata, &open_metadata)?;
    if open_metadata.len() > MAX_CONTEXT_PACKAGE_BYTES as u64 {
        return Err(invalid_data(format!(
            "session context package exceeds {MAX_CONTEXT_PACKAGE_BYTES} bytes"
        )));
    }

    let mut bytes = Vec::with_capacity(open_metadata.len() as usize);
    file.take(MAX_CONTEXT_PACKAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read session context package: {error}"),
            )
        })?;
    if bytes.len() > MAX_CONTEXT_PACKAGE_BYTES {
        return Err(invalid_data(format!(
            "session context package grew past {MAX_CONTEXT_PACKAGE_BYTES} bytes while reading"
        )));
    }

    let raw: Value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid_data(format!(
            "session context package is not valid JSON: {error}"
        ))
    })?;
    reject_secret_material(&raw)?;
    // Decode the original bytes, not the intermediate Value: a Value map
    // has already collapsed duplicate JSON keys, while the strict shared
    // struct must reject duplicate fields rather than silently taking one.
    let package: CodingSessionContextPackage = serde_json::from_slice(&bytes).map_err(|error| {
        invalid_data(format!(
            "session context package has an invalid shape: {error}"
        ))
    })?;
    package.validate().map_err(|error| {
        invalid_data(format!(
            "session context package failed validation: {error}"
        ))
    })?;
    Ok(package)
}

impl SessionContextState {
    #[cfg(test)]
    fn from_package(package: CodingSessionContextPackage) -> Self {
        Self {
            loaded: RwLock::new(LoadedPackage::new(0, package)),
            dir: None,
            self_target: None,
            last_reported_seq: AtomicU64::new(0),
        }
    }

    /// The same, for a server the launcher told which execution it serves.
    ///
    /// The env variable itself is read once at construction and is not safe to
    /// set from a threaded test runner, so tests inject the resolved value.
    #[cfg(test)]
    fn from_package_serving(package: CodingSessionContextPackage, self_target: &str) -> Self {
        Self {
            loaded: RwLock::new(LoadedPackage::new(0, package)),
            dir: None,
            self_target: Some(self_target.to_owned()),
            last_reported_seq: AtomicU64::new(0),
        }
    }

    pub(crate) fn overview(&self, _params: SessionOverviewParams) -> Result<String, ErrorData> {
        let (loaded, refresh) = self.serve();
        let package = &loaded.package;
        let mut history_by_role = BTreeMap::<&'static str, usize>::new();
        for item in &package.history {
            *history_by_role.entry(role_label(item.role)).or_default() += 1;
        }
        let mut response = base_response(&loaded, &refresh);
        response.insert("session".into(), json!(package.session));
        response.insert(
            "firstTurnBrief".into(),
            coding_session_first_turn_brief(package),
        );
        response.insert("availableHistoryItems".into(), json!(package.history.len()));
        response.insert("historyByRole".into(), json!(history_by_role));
        response.insert("selfTarget".into(), json!(self.self_target));
        response.insert(
            "roster".into(),
            json!(roster_view(package, self.self_target.as_deref())),
        );
        response.insert("rosterSemantics".into(), roster_semantics());
        // Absent, not null, when the umbrella has no policy: a reader that saw
        // `"policy": null` beside `policySemantics` would have to guess whether
        // nobody set one or the projection could not read one.
        if let Some(policy) = &package.policy {
            response.insert("policy".into(), json!(policy));
            response.insert("policySemantics".into(), policy_semantics());
        }
        response.insert(
            "availableInboxItems".into(),
            json!(self.inbox_items(package).len()),
        );
        render(Value::Object(response))
    }

    /// The umbrella's addressed commands that this execution is the target of.
    ///
    /// An empty slice when the launcher named no self target: without it there
    /// is no honest way to tell this seat's mail from a sibling's, and showing
    /// a sibling's would be exactly the cross-execution leak the package
    /// bounds exist to prevent. The refusal is disclosed in the response, not
    /// silently rendered as "no mail".
    fn inbox_items<'a>(
        &self,
        package: &'a CodingSessionContextPackage,
    ) -> Vec<&'a beekeeper_core::coding_session_context::CodingSessionContextInboxItem> {
        let Some(self_target) = self.self_target.as_deref() else {
            return Vec::new();
        };
        package
            .inbox
            .iter()
            .filter(|item| coding_session_target_key(&item.target) == self_target)
            .collect()
    }

    pub(crate) fn inbox(&self, params: SessionInboxParams) -> Result<String, ErrorData> {
        let (loaded, refresh) = self.serve();
        let limit = bounded_limit(
            params.limit,
            DEFAULT_INBOX_LIMIT,
            MAX_INBOX_LIMIT,
            "session_inbox",
        )?;
        let items = self.inbox_items(&loaded.package);
        let total = items.len();

        let start = match params.since.as_deref() {
            Some(cursor) => match items.iter().position(|item| item.event_id == cursor) {
                Some(offset) => offset.saturating_add(1).min(total),
                None => {
                    let mut response = base_response(&loaded, &refresh);
                    response.insert("selfTarget".into(), json!(self.self_target));
                    response.insert(
                        "scope".into(),
                        json!(match self.self_target {
                            Some(_) => "commands_addressed_to_this_execution",
                            None => "unavailable_no_self_target",
                        }),
                    );
                    response.insert("limit".into(), json!(limit));
                    response.insert("returned".into(), json!(0));
                    response.insert("availableInboxItems".into(), json!(total));
                    response.insert("stoppedBy".into(), json!("cursorMiss"));
                    response.insert("cursorResolution".into(), json!("not_in_package"));
                    response.insert("nextCursor".into(), Value::Null);
                    response.insert("items".into(), json!([]));
                    return render(Value::Object(response));
                }
            },
            None => 0,
        };
        let page: Vec<Value> = items[start..]
            .iter()
            .take(limit)
            .map(|item| json!(item))
            .collect();
        let end = start.saturating_add(page.len());

        let mut response = base_response(&loaded, &refresh);
        response.insert("selfTarget".into(), json!(self.self_target));
        response.insert(
            "scope".into(),
            json!(match self.self_target {
                Some(_) => "commands_addressed_to_this_execution",
                None => "unavailable_no_self_target",
            }),
        );
        response.insert("offset".into(), json!(start));
        response.insert("limit".into(), json!(limit));
        response.insert("returned".into(), json!(page.len()));
        response.insert("availableInboxItems".into(), json!(total));
        response.insert(
            "stoppedBy".into(),
            json!(if end >= total { "end" } else { "limit" }),
        );
        response.insert(
            "cursorResolution".into(),
            match params.since {
                Some(_) => json!("resolved"),
                None => Value::Null,
            },
        );
        response.insert(
            "nextCursor".into(),
            json!((end < total)
                .then(|| items.get(end.saturating_sub(1)))
                .flatten()
                .map(|item| item.event_id.clone())),
        );
        response.insert("inboxSemantics".into(), inbox_semantics());
        response.insert("items".into(), json!(page));
        render(Value::Object(response))
    }

    pub(crate) fn history(&self, params: SessionHistoryParams) -> Result<String, ErrorData> {
        let (loaded, refresh) = self.serve();
        let limit = bounded_limit(
            params.limit,
            DEFAULT_HISTORY_LIMIT,
            MAX_HISTORY_LIMIT,
            "session_history",
        )?;
        let view = params.view.unwrap_or_default();
        let total = loaded.package.history.len();

        // A cursor that the served package no longer carries is disclosed,
        // never silently restarted from zero: the truncation boundary moved
        // past it and the agent has to know that to stay honest about gaps.
        let start = match params.since.as_deref() {
            Some(cursor) => match loaded.index.get(cursor) {
                Some(offset) => offset.saturating_add(1).min(total),
                None => {
                    let mut response = base_response(&loaded, &refresh);
                    response.insert(
                        "sessionRef".into(),
                        json!(loaded.package.session.session_ref),
                    );
                    response.insert("view".into(), json!(view_label(view)));
                    response.insert("limit".into(), json!(limit));
                    response.insert("returned".into(), json!(0));
                    response.insert("availableHistoryItems".into(), json!(total));
                    response.insert("stoppedBy".into(), json!("cursorMiss"));
                    response.insert("cursorResolution".into(), json!("not_in_package"));
                    response.insert("nextCursor".into(), Value::Null);
                    response.insert("nextOffset".into(), Value::Null);
                    response.insert("offsetStability".into(), json!(OFFSET_STABILITY));
                    response.insert("items".into(), json!([]));
                    return render(Value::Object(response));
                }
            },
            None => params.offset.unwrap_or(0).min(total),
        };

        let page = collect_history_page(
            &loaded.package.history[start..],
            limit,
            MAX_HISTORY_PAGE_BYTES,
            view,
        )?;
        let end = start.saturating_add(page.items.len());
        let stopped_by = if end >= total {
            "end"
        } else if page.items.len() >= limit {
            "limit"
        } else {
            "pageBytes"
        };
        let next_cursor = (end < total)
            .then(|| loaded.package.history.get(end.saturating_sub(1)))
            .flatten()
            .map(|item| item.event_id.clone());

        let mut response = base_response(&loaded, &refresh);
        response.insert(
            "sessionRef".into(),
            json!(loaded.package.session.session_ref),
        );
        response.insert("view".into(), json!(view_label(view)));
        response.insert("offset".into(), json!(start));
        response.insert("limit".into(), json!(limit));
        response.insert("returned".into(), json!(page.items.len()));
        response.insert("availableHistoryItems".into(), json!(total));
        response.insert("stoppedBy".into(), json!(stopped_by));
        response.insert(
            "cursorResolution".into(),
            match params.since {
                Some(_) => json!("resolved"),
                None => Value::Null,
            },
        );
        response.insert("nextCursor".into(), json!(next_cursor));
        response.insert("nextOffset".into(), json!((end < total).then_some(end)));
        response.insert("offsetStability".into(), json!(OFFSET_STABILITY));
        if view == SessionHistoryView::Index {
            response.insert("targets".into(), json!(page.targets));
        }
        response.insert("items".into(), json!(page.items));
        render(Value::Object(response))
    }

    pub(crate) fn search(&self, params: SearchSessionParams) -> Result<String, ErrorData> {
        let (loaded, refresh) = self.serve();
        let query = params.query.trim();
        if query.is_empty() || query.len() > MAX_SEARCH_QUERY_BYTES {
            return Err(ErrorData::invalid_params(
                format!(
                    "search_session query must contain text and be at most {MAX_SEARCH_QUERY_BYTES} UTF-8 bytes"
                ),
                None,
            ));
        }
        let limit = bounded_limit(
            params.limit,
            DEFAULT_SEARCH_LIMIT,
            MAX_SEARCH_LIMIT,
            "search_session",
        )?;
        let offset = params.offset.unwrap_or(0);
        let folded_query = query.to_ascii_lowercase();
        let mut matches = Vec::new();
        for (history_offset, item) in loaded.package.history.iter().enumerate() {
            let searchable = serde_json::to_string(item).map_err(internal_serialization)?;
            let match_offset = if query.is_ascii() {
                searchable.to_ascii_lowercase().find(&folded_query)
            } else {
                searchable.find(query)
            };
            if let Some(match_offset) = match_offset {
                matches.push((history_offset, item, searchable, match_offset));
            }
        }
        let total_matches = matches.len();
        let start = offset.min(total_matches);
        let mut results = Vec::new();
        let mut used = 0usize;
        for (history_offset, item, searchable, match_offset) in matches[start..].iter().take(limit)
        {
            let result = json!({
                "historyOffset": history_offset,
                "eventId": item.event_id,
                "cursor": item.event_id,
                "createdAt": item.created_at,
                "author": item.author,
                "target": item.target,
                "eventSeq": item.event_seq,
                "turnId": item.turn_id,
                "role": item.role,
                "itemKind": item.item_kind,
                "snippet": excerpt_around(searchable, *match_offset, MAX_SEARCH_SNIPPET_BYTES),
                "snippetTruncatedByTool": searchable.len() > MAX_SEARCH_SNIPPET_BYTES,
            });
            let size = rendered_bytes(&result)?;
            // Always emit at least one result, so a single oversized match is
            // never made unfetchable.
            if !results.is_empty() && used.saturating_add(size) > MAX_SEARCH_PAGE_BYTES {
                break;
            }
            used = used.saturating_add(size);
            results.push(result);
        }
        let end = start.saturating_add(results.len());
        let stopped_by = if end >= total_matches {
            "end"
        } else if results.len() >= limit {
            "limit"
        } else {
            "pageBytes"
        };

        let mut response = base_response(&loaded, &refresh);
        response.insert(
            "sessionRef".into(),
            json!(loaded.package.session.session_ref),
        );
        response.insert("query".into(), json!(query));
        response.insert("offset".into(), json!(start));
        response.insert("limit".into(), json!(limit));
        response.insert("returned".into(), json!(results.len()));
        response.insert("totalMatches".into(), json!(total_matches));
        response.insert("stoppedBy".into(), json!(stopped_by));
        // Search pages over recomputed matches, not over a durable sequence,
        // so it has no cursor of its own; each result names its item's cursor
        // for an exact session_history follow-up.
        response.insert("nextCursor".into(), Value::Null);
        response.insert("cursorResolution".into(), Value::Null);
        response.insert(
            "nextOffset".into(),
            json!((end < total_matches).then_some(end)),
        );
        response.insert("offsetStability".into(), json!(OFFSET_STABILITY));
        response.insert("results".into(), json!(results));
        render(Value::Object(response))
    }
}

/// Keys every context tool response carries, whatever it is answering.
fn base_response(
    loaded: &LoadedPackage,
    refresh: &RefreshOutcome,
) -> serde_json::Map<String, Value> {
    let mut response = serde_json::Map::new();
    response.insert("packageVersion".into(), json!(loaded.package.v));
    // Read off the guard that produced everything else in this response, so
    // the envelope can never name a generation the body did not come from.
    response.insert("packageGeneration".into(), json!(loaded.seq));
    response.insert(
        "generationChangedSincePreviousCall".into(),
        json!(refresh.changed),
    );
    response.insert("refreshRefused".into(), json!(refresh.refused));
    response.insert("provenance".into(), json!(loaded.package.provenance));
    response.insert("provenanceSemantics".into(), provenance_semantics());
    response.insert("staleness".into(), staleness(&loaded.package.provenance));
    response
}

/// The `cs-target` key the launcher named for this server, when it named one.
///
/// Read once at construction, like every other launcher-only input: a tool
/// call cannot redirect the server at another execution's mail.
fn self_target_from_env() -> Option<String> {
    std::env::var(SESSION_CONTEXT_SELF_TARGET_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The roster, with the seat this server serves marked.
///
/// `isSelf` is computed here rather than left to the reader: an agent that has
/// to guess which row is itself will sooner or later address a message to
/// itself and wait for an answer that cannot come.
fn roster_view(package: &CodingSessionContextPackage, self_target: Option<&str>) -> Vec<Value> {
    package
        .roster
        .iter()
        .map(|entry| {
            let target_key = coding_session_target_key(&entry.target);
            let is_self = self_target.is_some_and(|value| value == target_key);
            json!({
                "target": entry.target,
                "targetKey": target_key,
                "isSelf": is_self,
                "actor": entry.actor,
                "role": entry.role,
                "status": entry.status.as_str(),
                "lastSignedSeq": entry.last_signed_seq,
                "lastSignedAtMs": entry.last_signed_at_ms,
                "quietForMs": entry
                    .last_signed_at_ms
                    .map(|at| package.provenance.generated_at.saturating_sub(at)),
            })
        })
        .collect()
}

/// What the roster does and does not claim.
fn roster_semantics() -> Value {
    json!({
        "targetKey": "the cs-target key to address with `bee sessions send --to <targetKey>`",
        "status": "active | superseded | ended | unknown, derived from signed metadata and the resume chain only",
        "liveness": "no lease is read here: a seat that is `active` may still be a stopped process. quietForMs is measured from the projection time, not from now — add staleness.ageSinceProjectionMs.",
        "quietForMs": "milliseconds between this seat's newest signed transcript item and this package's projection",
        "role": "null for a human-created execution; a role slug only for a seated agent",
    })
}

/// What a published session policy is, and — the load-bearing half — what it
/// is not.
///
/// Exactly one field in a kind-44245 record is enforced anywhere in this
/// repository (`docs/design/portable-team-loop/POLICY.md` §4). A seat that
/// read `gates.redFirst: true` and assumed something was checking would be
/// wrong, and a surface that let it assume so would be the "control that lies
/// about what it enforces" bug this project treats as a crash.
fn policy_semantics() -> Value {
    json!({
        "authority": "authorIsFounder distinguishes the umbrella's founder from a granted operator; the relay validates a policy's structure and never adjudicates who was entitled to set one",
        "enforced": "budget.turns only, at this provider's turn gate: a turn beyond it is refused BUDGET_EXHAUSTED and the refusal says the policy bound it",
        "notEnforced": "every other field — posture, the remaining budgets, attention, gates, bench, irreversible, stop — is a stated intention that nothing in this repository checks. Read it, quote it, act on it yourself; do not report it as a limit something is holding you to.",
        "withdrawal": "a record that sets no field at all is a policy somebody withdrew, which is a decision — not 'policy unknown'",
        "freshness": "a snapshot like everything else here; a policy published after this package was projected is not in it",
    })
}

/// What one inbox item does and does not claim.
fn inbox_semantics() -> Value {
    json!({
        "items": "verified kind-44220 turn commands addressed to this execution, oldest first",
        "stage": "the newest turn receipt this package could verify for the command: turn_queued, turn_started, turn_degraded, turn_dropped, turn_refused or interrupt_delivered; null means no verifiable receipt was in the fact set, never that none exists",
        "stageCode": "the receipt's error code when it carried one; the code vocabulary is open",
        "content": "the command's text as signed, after the package's fail-closed redaction",
        "ordering": "createdAt is the signed event time in Unix seconds; a page cursor is an eventId. The package retains a bounded newest-first window, so a cursor can fall out of it between refreshes: a `since` that is no longer retained answers stoppedBy 'cursorMiss' with cursorResolution 'not_in_package' and returns no items, and the way forward is to page again from the start.",
    })
}

/// How old the served snapshot is at the moment this response is rendered.
///
/// `completeAsOfMs` and `ageSinceCompleteAsOfMs` are explicit `null` for a
/// package with no watermark; the projection time is never silently
/// substituted for the watermark.
fn staleness(provenance: &CodingSessionContextProvenance) -> Value {
    let read_at_ms = now_ms();
    let age_since_projection_ms = read_at_ms.saturating_sub(provenance.generated_at);
    let age_since_complete_as_of_ms = provenance
        .complete_as_of
        .map(|watermark| read_at_ms.saturating_sub(watermark));
    let stale = age_since_complete_as_of_ms.unwrap_or(age_since_projection_ms) >= STALE_AFTER_MS;
    json!({
        "readAtMs": read_at_ms,
        "projectedAtMs": provenance.generated_at,
        "completeAsOfMs": provenance.complete_as_of,
        "ageSinceProjectionMs": age_since_projection_ms,
        "ageSinceCompleteAsOfMs": age_since_complete_as_of_ms,
        "stale": stale,
        "staleAfterMs": STALE_AFTER_MS,
        "clock": "this server's wall clock; the projector ran on the same host",
    })
}

/// Wall-clock milliseconds since the Unix epoch.
///
/// A clock reading before the epoch, or past the `i64` millisecond range, is
/// reported as `0` rather than panicking; the raw `projectedAtMs` sits beside
/// it so such a reading is visibly wrong instead of quietly plausible.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

fn provenance_semantics() -> Value {
    json!({
        "complete": "Complete only for the source snapshot begun at provenance.completeAsOf; later concurrent activity may exist. A null completeAsOf is a legacy package with an unknown watermark",
        "sourceEventCount": "All signed facts retained in the verified proof graph. provenance.sourceEventBreakdown reconciles it exactly: genesisEvents + authorityLinkEvents + nameRevisionEvents + goalRevisionEvents + generationBookkeepingEvents + transcriptEvents == sourceEventCount. Only transcriptEvents become history items, which is why sourceEventCount exceeds totalHistoryItems",
        "totalHistoryItems": "Verified transcript items only",
        "readTime": "readAtMs is this server's wall clock when this response was rendered, on the same host that projected the package. ageSinceCompleteAsOfMs is how long ago the snapshot watermark was",
        "staleness": "This package is a snapshot, not a live view. Nothing published after completeAsOf is represented here. When stale is true, say so to the operator before relying on this package to describe what the session is doing now",
        "pagination": "A page ends at limit or at the response byte budget, whichever comes first; stoppedBy names which. Offsets are not stable across a package refresh; a cursor is",
        "packageGeneration": "The launcher may write a newer verified package for this execution while it runs. Each response names the generation it served; a rising packageGeneration with a rising completeAsOf means this server picked up refreshed evidence"
    })
}

fn view_label(view: SessionHistoryView) -> &'static str {
    match view {
        SessionHistoryView::Full => "full",
        SessionHistoryView::Index => "index",
    }
}

/// One assembled history page and the execution legend its items point into.
struct HistoryPage {
    items: Vec<Value>,
    targets: Vec<CodingSessionTarget>,
}

/// Assemble items until `limit` or `budget` stops the page.
///
/// At least one item is always emitted when `items` is non-empty, so a single
/// oversized item is never made unfetchable.
fn collect_history_page(
    items: &[CodingSessionContextHistoryItem],
    limit: usize,
    budget: usize,
    view: SessionHistoryView,
) -> Result<HistoryPage, ErrorData> {
    let mut page = HistoryPage {
        items: Vec::new(),
        targets: Vec::new(),
    };
    let mut target_indexes = HashMap::<String, usize>::new();
    let mut used = 0usize;
    for item in items.iter().take(limit) {
        let mut pending_target = None;
        let mut legend_bytes = 0usize;
        let value = match view {
            SessionHistoryView::Full => history_item_view(item)?,
            SessionHistoryView::Index => {
                let target_key = coding_session_target_key(&item.target);
                let target_index = match target_indexes.get(&target_key) {
                    Some(index) => *index,
                    None => {
                        legend_bytes = rendered_bytes(&json!(item.target))?;
                        pending_target = Some((target_key, page.targets.len()));
                        page.targets.len()
                    }
                };
                history_index_view(item, target_index)?
            }
        };
        let size = rendered_bytes(&value)?.saturating_add(legend_bytes);
        if !page.items.is_empty() && used.saturating_add(size) > budget {
            break;
        }
        if let Some((target_key, index)) = pending_target {
            target_indexes.insert(target_key, index);
            page.targets.push(item.target.clone());
        }
        used = used.saturating_add(size);
        page.items.push(value);
    }
    Ok(page)
}

/// Rendered size of one value, measured the way the response is rendered.
fn rendered_bytes(value: &Value) -> Result<usize, ErrorData> {
    serde_json::to_string_pretty(value)
        .map(|encoded| encoded.len())
        .map_err(internal_serialization)
}

fn history_item_view(item: &CodingSessionContextHistoryItem) -> Result<Value, ErrorData> {
    let content = serde_json::to_string(&item.content).map_err(internal_serialization)?;
    let (content_value, preview, content_truncated) = if content.len() <= MAX_INLINE_CONTENT_BYTES {
        (Some(item.content.clone()), None, false)
    } else {
        (
            None,
            Some(truncate_utf8(&content, MAX_INLINE_CONTENT_BYTES)),
            true,
        )
    };
    Ok(json!({
        "eventId": item.event_id,
        "cursor": item.event_id,
        "createdAt": item.created_at,
        "author": item.author,
        "sourceKind": item.source_kind,
        "target": item.target,
        "eventSeq": item.event_seq,
        "turnId": item.turn_id,
        "role": item.role,
        "itemKind": item.item_kind,
        "content": content_value,
        "contentPreviewJson": preview,
        "contentTruncatedByTool": content_truncated,
    }))
}

/// Metadata-only projection of one item.
///
/// `eventId` is the cursor, so no separate `cursor` key is emitted. `author`
/// is omitted (the same provider-authority pubkey on essentially every item,
/// and present in the full view) and so is `sourceKind`, which the package
/// contract pins to 44225 for every history item and which would therefore be
/// a constant column. `targetIndex` points into the response's `targets`
/// legend, so an index page still says which execution produced each item.
fn history_index_view(
    item: &CodingSessionContextHistoryItem,
    target_index: usize,
) -> Result<Value, ErrorData> {
    let content = serde_json::to_string(&item.content).map_err(internal_serialization)?;
    Ok(json!({
        "eventId": item.event_id,
        "createdAt": item.created_at,
        "eventSeq": item.event_seq,
        "turnId": item.turn_id,
        "targetIndex": target_index,
        "role": item.role,
        "itemKind": item.item_kind,
        "contentBytes": content.len(),
        "textPreview": index_preview(&item.content, &content),
    }))
}

fn index_preview(content: &Value, encoded: &str) -> String {
    let text = content
        .as_object()
        .and_then(|object| {
            ["text", "content", "message", "summary", "title"]
                .iter()
                .find_map(|key| object.get(*key).and_then(Value::as_str))
        })
        .unwrap_or(encoded);
    truncate_utf8(text, MAX_INDEX_PREVIEW_BYTES)
}

fn bounded_limit(
    requested: Option<usize>,
    default: usize,
    maximum: usize,
    tool: &str,
) -> Result<usize, ErrorData> {
    let limit = requested.unwrap_or(default);
    if limit == 0 || limit > maximum {
        return Err(ErrorData::invalid_params(
            format!("{tool} limit must be between 1 and {maximum}"),
            None,
        ));
    }
    Ok(limit)
}

fn role_label(role: CodingSessionContextRole) -> &'static str {
    match role {
        CodingSessionContextRole::User => "user",
        CodingSessionContextRole::Assistant => "assistant",
        CodingSessionContextRole::Tool => "tool",
        CodingSessionContextRole::Reasoning => "reasoning",
        CodingSessionContextRole::Lifecycle => "lifecycle",
        CodingSessionContextRole::System => "system",
    }
}

fn excerpt_around(text: &str, match_offset: usize, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut start = match_offset.saturating_sub(max_bytes / 3);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start.saturating_add(max_bytes).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < text.len() { "…" } else { "" };
    format!("{prefix}{}{suffix}", &text[start..end])
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8()).min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn render(value: Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(&value).map_err(internal_serialization)
}

fn internal_serialization(error: serde_json::Error) -> ErrorData {
    ErrorData::internal_error(
        format!("failed to serialize session context: {error}"),
        None,
    )
}

fn reject_secret_material(value: &Value) -> io::Result<()> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if matches!(
                    normalized.as_str(),
                    "privatekey"
                        | "secretkey"
                        | "signingkey"
                        | "nostrprivatekey"
                        | "buzzprivatekey"
                        | "buzzauthtag"
                        | "relayauthtoken"
                ) {
                    return Err(invalid_data(format!(
                        "session context package contains forbidden secret field {key:?}"
                    )));
                }
                reject_secret_material(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                reject_secret_material(nested)?;
            }
        }
        Value::String(text) if text.trim_start().starts_with("nsec1") => {
            return Err(invalid_data(
                "session context package contains forbidden Nostr secret material",
            ));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(unix)]
fn validate_private_permissions(
    path_metadata: &std::fs::Metadata,
    open_metadata: &std::fs::Metadata,
) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if path_metadata.dev() != open_metadata.dev() || path_metadata.ino() != open_metadata.ino() {
        return Err(invalid_data(
            "session context package changed while it was being opened",
        ));
    }
    if open_metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session context package must have exact Unix permissions 0600",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_permissions(
    _path_metadata: &std::fs::Metadata,
    _open_metadata: &std::fs::Metadata,
) -> io::Result<()> {
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn package_json() -> Value {
        json!({
            "v": 1,
            "session": {
                "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
                "genesisRef": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
                "channelId": "00000000-0000-0000-0000-000000000000",
                "name": "Rehydrated context",
                "goal": "Continue from durable facts",
                "projectRef": null
            },
            "provenance": {
                "generatedAt": 1,
                "completeAsOf": null,
                "complete": false,
                "truncated": true,
                "sourceEventCount": 4,
                "includedHistoryItems": 2,
                "omittedHistoryItems": 3,
                "totalHistoryItems": null,
                "notes": ["Source query did not prove a complete history window"]
            },
            "history": [
                {
                    "eventId": format!("{:064x}", 1),
                    "createdAt": 10,
                    "author": "ab".repeat(32),
                    "sourceKind": 44225,
                    "target": {
                        "driver": "codex-acp",
                        "instanceId": "primary",
                        "sessionId": "provider-session",
                        "generation": 1
                    },
                    "eventSeq": 1,
                    "turnId": "turn-1",
                    "role": "user",
                    "itemKind": "user_prompt",
                    "content": {"kind": "user_prompt", "text": "Need the durable close/reopen split"}
                },
                {
                    "eventId": format!("{:064x}", 2),
                    "createdAt": 11,
                    "author": "ab".repeat(32),
                    "sourceKind": 44225,
                    "target": {
                        "driver": "codex-acp",
                        "instanceId": "primary",
                        "sessionId": "provider-session",
                        "generation": 1
                    },
                    "eventSeq": 2,
                    "turnId": "turn-1",
                    "role": "assistant",
                    "itemKind": "assistant_text",
                    "content": {"kind": "assistant_text", "text": "Closure is separate from stopping execution"}
                }
            ]
        })
    }

    fn package() -> CodingSessionContextPackage {
        serde_json::from_value(package_json()).expect("valid package fixture")
    }

    fn write_package(path: &Path, value: &Value) {
        std::fs::write(path, serde_json::to_vec(value).expect("encode package"))
            .expect("write package");
        make_private(path);
    }

    fn make_private(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("chmod package");
        }
    }

    #[test]
    fn loads_one_private_absolute_package_and_keeps_provenance_honest() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());

        let state = SessionContextState::load(&path).expect("load package");
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        assert_eq!(overview["provenance"]["complete"], false);
        assert_eq!(overview["provenance"]["truncated"], true);
        assert_eq!(overview["provenance"]["omittedHistoryItems"], 3);
        assert_eq!(overview["availableHistoryItems"], 2);
    }

    #[test]
    fn history_is_bounded_paginated_and_repeats_provenance() {
        let state = SessionContextState::from_package(package());
        let first: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: Some(0),
                    limit: Some(1),
                    ..Default::default()
                })
                .expect("history"),
        )
        .expect("history JSON");
        assert_eq!(first["returned"], 1);
        assert_eq!(first["nextOffset"], 1);
        assert_eq!(first["items"][0]["itemKind"], "user_prompt");
        assert_eq!(first["provenance"]["complete"], false);
        assert_eq!(first["provenance"]["truncated"], true);
        assert!(first["provenanceSemantics"]["sourceEventCount"]
            .as_str()
            .is_some_and(|text| text.contains("proof graph")));
        assert!(state
            .history(SessionHistoryParams {
                offset: None,
                limit: Some(MAX_HISTORY_LIMIT + 1),
                ..Default::default()
            })
            .is_err());
    }

    #[test]
    fn one_history_call_can_retrieve_the_observed_101_item_session() {
        let mut package = package();
        let template = package.history[0].clone();
        package.history = (1..=101)
            .map(|seq| CodingSessionContextHistoryItem {
                event_id: format!("{seq:064x}"),
                created_at: seq,
                event_seq: seq,
                content: json!({
                    "kind": "user_prompt",
                    "content": format!("safe prompt {seq}"),
                    "steered": false
                }),
                ..template.clone()
            })
            .collect();
        package.provenance.included_history_items = 101;
        package.provenance.truncated = false;
        package.provenance.omitted_history_items = 0;
        package.provenance.total_history_items = None;
        package.validate().expect("101-item package");
        let state = SessionContextState::from_package(package);

        let page: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: Some(0),
                    limit: Some(101),
                    ..Default::default()
                })
                .expect("101-item page"),
        )
        .expect("history JSON");

        assert_eq!(page["returned"], 101);
        assert!(page["nextOffset"].is_null());
    }

    #[test]
    fn search_is_bounded_paginated_and_repeats_provenance() {
        let state = SessionContextState::from_package(package());
        let result: Value = serde_json::from_str(
            &state
                .search(SearchSessionParams {
                    query: "CLOSURE".into(),
                    offset: None,
                    limit: Some(1),
                })
                .expect("search"),
        )
        .expect("search JSON");
        assert_eq!(result["totalMatches"], 1);
        assert_eq!(result["results"][0]["historyOffset"], 1);
        assert_eq!(result["provenance"]["complete"], false);
        assert_eq!(result["provenance"]["truncated"], true);
        assert!(state
            .search(SearchSessionParams {
                query: "x".repeat(MAX_SEARCH_QUERY_BYTES + 1),
                offset: None,
                limit: None,
            })
            .is_err());
    }

    #[test]
    fn rejects_relative_non_private_symlink_and_malformed_packages() {
        assert!(SessionContextState::load(Path::new("relative.json")).is_err());
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("chmod public");
            assert!(SessionContextState::load(&path).is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("chmod private");
            let link = dir.path().join("context-link.json");
            symlink(&path, &link).expect("create symlink");
            assert!(SessionContextState::load(&link).is_err());
        }

        let malformed = dir.path().join("malformed.json");
        write_package(&malformed, &json!({"v": 1, "privateKey": "secret"}));
        assert!(SessionContextState::load(&malformed).is_err());

        let duplicate = dir.path().join("duplicate.json");
        let encoded = serde_json::to_string(&package_json()).expect("encode duplicate fixture");
        std::fs::write(
            &duplicate,
            encoded.replacen("\"v\":1", "\"v\":1,\"v\":1", 1),
        )
        .expect("write duplicate fixture");
        make_private(&duplicate);
        assert!(SessionContextState::load(&duplicate).is_err());

        let oversized = dir.path().join("oversized.json");
        let file = File::create(&oversized).expect("create oversized fixture");
        file.set_len(MAX_CONTEXT_PACKAGE_BYTES as u64 + 1)
            .expect("size oversized fixture");
        make_private(&oversized);
        assert!(SessionContextState::load(&oversized).is_err());
    }

    #[test]
    fn rejects_secret_fields_and_nsec_values_inside_structured_history() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        let mut secret_field = package_json();
        secret_field["history"][0]["content"]["private_key"] = json!("aa");
        write_package(&path, &secret_field);
        assert!(SessionContextState::load(&path).is_err());

        let mut nsec = package_json();
        nsec["history"][0]["content"]["text"] = json!("nsec1forbidden");
        write_package(&path, &nsec);
        assert!(SessionContextState::load(&path).is_err());

        let mut host_path = package_json();
        host_path["history"][0]["content"]["content"] = json!("read /Users/alice/private/repo");
        write_package(&path, &host_path);
        assert!(SessionContextState::load(&path).is_err());
    }

    #[test]
    fn a_large_valid_item_is_returned_in_full() {
        let mut raw = package_json();
        raw["provenance"]["complete"] = json!(true);
        raw["provenance"]["completeAsOf"] = json!(1);
        raw["provenance"]["truncated"] = json!(false);
        raw["provenance"]["omittedHistoryItems"] = json!(0);
        raw["provenance"]["totalHistoryItems"] = json!(2);
        raw["history"][0]["content"]["text"] = json!("x".repeat(30 * 1024));
        let package: CodingSessionContextPackage =
            serde_json::from_value(raw).expect("large package fixture");
        package.validate().expect("large package valid");
        let state = SessionContextState::from_package(package);
        let history: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: None,
                    limit: Some(1),
                    ..Default::default()
                })
                .expect("history"),
        )
        .expect("history JSON");
        assert_eq!(history["items"][0]["contentTruncatedByTool"], false);
        assert_eq!(
            history["items"][0]["content"]["text"]
                .as_str()
                .map(str::len),
            Some(30 * 1024)
        );
        assert!(history["items"][0]["contentPreviewJson"].is_null());
        assert_eq!(history["provenance"]["complete"], true);
        assert_eq!(history["provenance"]["truncated"], false);
    }

    // ---- pagination, staleness, and refresh ----------------------------

    /// A valid package of `count` items, each carrying `filler_bytes` of safe
    /// text, spread over `generations` sibling executions.
    fn synthetic_package(
        count: usize,
        filler_bytes: usize,
        generations: u64,
    ) -> CodingSessionContextPackage {
        let mut package = package();
        let template = package.history[0].clone();
        package.history = (1..=count)
            .map(|seq| CodingSessionContextHistoryItem {
                event_id: format!("{seq:064x}"),
                created_at: seq as u64,
                event_seq: seq as u64,
                target: CodingSessionTarget {
                    generation: (seq as u64 - 1) % generations + 1,
                    ..template.target.clone()
                },
                content: json!({
                    "kind": "user_prompt",
                    "text": format!("needle {}", "x".repeat(filler_bytes)),
                }),
                ..template.clone()
            })
            .collect();
        package.provenance.included_history_items = count as u64;
        package.provenance.truncated = false;
        package.provenance.omitted_history_items = 0;
        package.provenance.total_history_items = None;
        package.validate().expect("synthetic package fixture");
        package
    }

    /// The fixture package plus a two-seat roster and three addressed
    /// commands: two for this execution, one for a sibling.
    fn crew_package() -> (CodingSessionContextPackage, CodingSessionTarget) {
        use beekeeper_core::coding_session_context::{
            CodingSessionContextInboxItem, CodingSessionContextRosterEntry,
            CodingSessionContextSeatStatus,
        };
        use beekeeper_core::coding_session_payload::ReceiptStatus;

        let mut package = package();
        let mine = package.history[0].target.clone();
        let sibling = CodingSessionTarget {
            session_id: "sibling-session".into(),
            ..mine.clone()
        };
        package.roster = vec![
            CodingSessionContextRosterEntry {
                target: mine.clone(),
                actor: Some("ab".repeat(32)),
                role: Some("builder".into()),
                status: CodingSessionContextSeatStatus::Active,
                last_signed_seq: Some(2),
                last_signed_at_ms: Some(11_000),
            },
            CodingSessionContextRosterEntry {
                target: sibling.clone(),
                actor: Some("cd".repeat(32)),
                role: Some("lead".into()),
                status: CodingSessionContextSeatStatus::Superseded,
                last_signed_seq: None,
                last_signed_at_ms: None,
            },
        ];
        let item =
            |seq: u64, target: &CodingSessionTarget, text: &str| CodingSessionContextInboxItem {
                event_id: format!("{seq:064x}"),
                created_at: 100 + seq,
                command_id: format!("turn-{seq}"),
                sender: "cd".repeat(32),
                sender_role: Some("lead".into()),
                target: target.clone(),
                delivery: "boundary".into(),
                content: text.into(),
                stage: Some(ReceiptStatus::TurnQueued),
                stage_at: Some(200 + seq),
                stage_code: None,
            };
        package.inbox = vec![
            item(1, &mine, "first, for me"),
            item(2, &sibling, "for the sibling"),
            item(3, &mine, "second, for me"),
        ];
        package.validate().expect("crew fixture is a valid package");
        (package, mine)
    }

    fn inbox_page(state: &SessionContextState, params: SessionInboxParams) -> Value {
        serde_json::from_str(&state.inbox(params).expect("inbox page")).expect("inbox JSON")
    }

    /// The whole reason the sidecar is told its own target: a seat reads its
    /// own mail, and never a sibling's.
    #[test]
    fn session_inbox_returns_only_commands_addressed_to_this_execution() {
        let (package, mine) = crew_package();
        let state =
            SessionContextState::from_package_serving(package, &coding_session_target_key(&mine));

        let page = inbox_page(&state, SessionInboxParams::default());
        assert_eq!(page["scope"], "commands_addressed_to_this_execution");
        assert_eq!(page["availableInboxItems"], 2);
        assert_eq!(page["returned"], 2);
        assert_eq!(page["stoppedBy"], "end");
        let commands: Vec<&str> = page["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| item["commandId"].as_str().expect("commandId"))
            .collect();
        assert_eq!(commands, vec!["turn-1", "turn-3"]);
        assert_eq!(page["items"][0]["stage"], "turn_queued");
        assert_eq!(page["items"][0]["senderRole"], "lead");
    }

    /// Paging is by event id, and a page that ends short of the end says so
    /// and hands back the cursor that continues it.
    #[test]
    fn session_inbox_pages_by_cursor() {
        let (package, mine) = crew_package();
        let state =
            SessionContextState::from_package_serving(package, &coding_session_target_key(&mine));

        let first = inbox_page(
            &state,
            SessionInboxParams {
                since: None,
                limit: Some(1),
            },
        );
        assert_eq!(first["returned"], 1);
        assert_eq!(first["stoppedBy"], "limit");
        let cursor = first["nextCursor"].as_str().expect("cursor").to_owned();
        assert_eq!(cursor, first["items"][0]["eventId"]);

        let second = inbox_page(
            &state,
            SessionInboxParams {
                since: Some(cursor),
                limit: None,
            },
        );
        assert_eq!(second["returned"], 1);
        assert_eq!(second["items"][0]["commandId"], "turn-3");
        assert_eq!(second["nextCursor"], Value::Null);

        let missed = inbox_page(
            &state,
            SessionInboxParams {
                since: Some("ff".repeat(32)),
                limit: None,
            },
        );
        assert_eq!(missed["stoppedBy"], "cursorMiss");
        assert_eq!(missed["returned"], 0);
    }

    /// The inbox cursor is not stable across a refresh, and the semantics
    /// block must not tell an agent that it is.
    ///
    /// The projector trims the inbox oldest-first to its item and byte bounds,
    /// and the fetch path drops older kind-44220 candidates when that
    /// partition saturates. A busy hour therefore pushes a cursor a seat is
    /// holding out of the retained window. The response already reports that
    /// honestly — `stoppedBy: "cursorMiss"`, `cursorResolution:
    /// "not_in_package"` and the true `availableInboxItems` — but the
    /// semantics block promised the opposite, so an agent had no reason to
    /// treat the miss as anything but a bug.
    #[test]
    fn the_inbox_cursor_contract_does_not_promise_a_cursor_that_survives_a_refresh() {
        let (package, mine) = crew_package();
        let self_target = coding_session_target_key(&mine);
        let state = SessionContextState::from_package_serving(package.clone(), &self_target);
        let first = inbox_page(
            &state,
            SessionInboxParams {
                since: None,
                limit: Some(1),
            },
        );
        let cursor = first["nextCursor"].as_str().expect("cursor").to_owned();

        // The same package one refresh later, with the oldest retained item
        // trimmed exactly as the projector's bounds trim it.
        let mut trimmed = package;
        trimmed.inbox.retain(|item| item.event_id != cursor);
        trimmed
            .validate()
            .expect("a trimmed inbox is still a package");
        let refreshed = SessionContextState::from_package_serving(trimmed, &self_target);
        let missed = inbox_page(
            &refreshed,
            SessionInboxParams {
                since: Some(cursor),
                limit: None,
            },
        );
        assert_eq!(
            missed["stoppedBy"], "cursorMiss",
            "a cursor really can fall out of the retained window"
        );

        let ordering = inbox_semantics()["ordering"]
            .as_str()
            .expect("ordering semantics")
            .to_owned();
        assert!(
            !ordering.contains("stable across a package refresh"),
            "the semantics must not promise what the bounds cannot keep: {ordering}"
        );
        assert!(
            ordering.contains("cursorMiss"),
            "and must name the answer an agent will actually get: {ordering}"
        );
    }

    /// Without a self target the server cannot tell this seat's mail from a
    /// sibling's, and says so instead of guessing either way.
    #[test]
    fn session_inbox_without_a_self_target_returns_nothing_and_names_why() {
        let (package, _) = crew_package();
        let state = SessionContextState::from_package(package);
        let page = inbox_page(&state, SessionInboxParams::default());
        assert_eq!(page["scope"], "unavailable_no_self_target");
        assert_eq!(page["selfTarget"], Value::Null);
        assert_eq!(page["returned"], 0);
        assert_eq!(page["availableInboxItems"], 0);
    }

    /// The overview carries the roster, marks which row is this execution, and
    /// states plainly that it is not reporting liveness.
    #[test]
    fn session_overview_carries_the_roster_and_marks_this_seat() {
        let (package, mine) = crew_package();
        let state =
            SessionContextState::from_package_serving(package, &coding_session_target_key(&mine));
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");

        let roster = overview["roster"].as_array().expect("roster");
        assert_eq!(roster.len(), 2);
        assert_eq!(roster[0]["isSelf"], true);
        assert_eq!(roster[0]["role"], "builder");
        assert_eq!(roster[0]["status"], "active");
        assert_eq!(roster[0]["targetKey"], coding_session_target_key(&mine));
        assert_eq!(roster[1]["isSelf"], false);
        assert_eq!(roster[1]["status"], "superseded");
        assert_eq!(roster[1]["lastSignedSeq"], Value::Null);
        assert_eq!(overview["availableInboxItems"], 2);
        assert!(
            overview["rosterSemantics"]["liveness"]
                .as_str()
                .expect("liveness disclosure")
                .contains("no lease is read here"),
            "the roster must not be mistaken for a liveness answer"
        );
    }

    /// A published session policy reaches the seat, and reaches it with the
    /// sentence that says what is and is not enforced. Showing a budget with
    /// no such caveat is a budget bar nothing is counting.
    #[test]
    fn session_overview_carries_the_policy_and_says_what_it_does_not_enforce() {
        use beekeeper_core::coding_session_context::CodingSessionContextPolicy;
        use beekeeper_core::coding_session_policy::{
            CodingSessionPolicyBudget, CodingSessionPolicyPayload,
        };

        let (mut package, mine) = crew_package();
        package.policy = Some(CodingSessionContextPolicy {
            event_id: "ab".repeat(32),
            created_at: 900,
            author: "cd".repeat(32),
            author_is_founder: true,
            record: CodingSessionPolicyPayload {
                budget: Some(CodingSessionPolicyBudget {
                    turns: Some(240),
                    tokens_per_seat: None,
                    tokens_per_session: None,
                    cost_usd_per_session: None,
                    context_tier: None,
                }),
                ..CodingSessionPolicyPayload::empty(
                    package.session.session_ref.clone(),
                    package.session.genesis_ref.clone(),
                )
            },
        });
        let state =
            SessionContextState::from_package_serving(package, &coding_session_target_key(&mine));
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");

        assert_eq!(overview["policy"]["record"]["budget"]["turns"], 240);
        assert_eq!(overview["policy"]["authorIsFounder"], true);
        assert!(
            overview["policySemantics"]["notEnforced"]
                .as_str()
                .expect("notEnforced disclosure")
                .contains("stated intention"),
            "a policy field nothing checks must not be shown as a limit"
        );
    }

    /// No policy, no key, no semantics block — and no reader left guessing
    /// whether one was set.
    #[test]
    fn session_overview_omits_the_policy_when_there_is_none() {
        let (package, mine) = crew_package();
        let state =
            SessionContextState::from_package_serving(package, &coding_session_target_key(&mine));
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        assert!(overview.get("policy").is_none(), "{overview}");
        assert!(overview.get("policySemantics").is_none(), "{overview}");
    }

    /// A package written before the crew fields existed still loads, and
    /// simply has nothing to report.
    #[test]
    fn a_package_without_roster_or_inbox_still_loads() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());
        let state = SessionContextState::load(&path).expect("legacy package loads");
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        assert_eq!(overview["roster"], json!([]));
        assert_eq!(overview["availableInboxItems"], 0);
    }

    fn history_page(state: &SessionContextState, params: SessionHistoryParams) -> Value {
        serde_json::from_str(&state.history(params).expect("history page")).expect("history JSON")
    }

    fn search_page(state: &SessionContextState, params: SearchSessionParams) -> Value {
        serde_json::from_str(&state.search(params).expect("search page")).expect("search JSON")
    }

    fn returned(page: &Value) -> usize {
        page["items"]
            .as_array()
            .or_else(|| page["results"].as_array())
            .map(Vec::len)
            .unwrap_or_default()
    }

    fn generation_json(count: usize) -> Value {
        serde_json::to_value(synthetic_package(count, 16, 1)).expect("encode generation")
    }

    fn write_generation(dir: &Path, seq: u64, value: &Value) -> PathBuf {
        let path = dir.join(format!("{seq:010}.json"));
        write_package(&path, value);
        path
    }

    #[test]
    fn the_whole_package_ceiling_walks_by_cursor_with_no_error_and_a_bounded_call_count() {
        const CALL_CEILING: usize = 20;
        let state =
            SessionContextState::from_package(synthetic_package(MAX_CONTEXT_HISTORY_ITEMS, 16, 1));
        let mut cursor: Option<String> = None;
        let mut seen = 0usize;
        let mut calls = 0usize;
        loop {
            calls += 1;
            assert!(calls <= CALL_CEILING, "walk exceeded {CALL_CEILING} calls");
            let page = history_page(
                &state,
                SessionHistoryParams {
                    since: cursor.clone(),
                    view: Some(SessionHistoryView::Index),
                    offset: None,
                    limit: Some(MAX_HISTORY_LIMIT),
                },
            );
            assert!(
                page["stoppedBy"].as_str().is_some(),
                "every page names the bound that stopped it"
            );
            seen += returned(&page);
            match page["nextCursor"].as_str() {
                Some(next) => {
                    assert_eq!(page["stoppedBy"], "pageBytes");
                    cursor = Some(next.to_owned());
                }
                None => {
                    assert_eq!(page["stoppedBy"], "end");
                    break;
                }
            }
        }
        assert_eq!(seen, MAX_CONTEXT_HISTORY_ITEMS);
        assert!(calls > 1, "the ceiling is a bounded walk, not one call");
    }

    #[test]
    fn an_index_page_is_at_least_three_times_denser_than_a_full_page_for_the_same_items() {
        let state = SessionContextState::from_package(synthetic_package(400, 2 * 1024, 1));
        let full = history_page(
            &state,
            SessionHistoryParams {
                limit: Some(MAX_HISTORY_LIMIT),
                ..Default::default()
            },
        );
        let index = history_page(
            &state,
            SessionHistoryParams {
                view: Some(SessionHistoryView::Index),
                limit: Some(MAX_HISTORY_LIMIT),
                ..Default::default()
            },
        );
        assert!(
            returned(&index) >= 3 * returned(&full),
            "index returned {} against full {}",
            returned(&index),
            returned(&full)
        );
    }

    #[test]
    fn a_full_view_page_that_hits_the_byte_budget_sets_stopped_by_page_bytes_and_a_next_cursor() {
        let package = synthetic_package(400, 2 * 1024, 1);
        let state = SessionContextState::from_package(package.clone());
        let page = history_page(
            &state,
            SessionHistoryParams {
                limit: Some(MAX_HISTORY_LIMIT),
                ..Default::default()
            },
        );
        assert_eq!(page["stoppedBy"], "pageBytes");
        assert!(returned(&page) < 400);
        assert_eq!(
            page["nextCursor"].as_str(),
            Some(package.history[returned(&page) - 1].event_id.as_str())
        );
        assert_eq!(page["nextOffset"], json!(returned(&page)));
    }

    #[test]
    fn one_oversized_item_is_still_returned_alone_rather_than_made_unfetchable() {
        let package = synthetic_package(3, 1024, 1);
        for view in [SessionHistoryView::Full, SessionHistoryView::Index] {
            let page = collect_history_page(&package.history, MAX_HISTORY_LIMIT, 10, view)
                .expect("page under an impossible budget");
            assert_eq!(page.items.len(), 1, "a page never returns zero items");
        }
    }

    #[test]
    fn a_since_cursor_returns_only_the_items_after_it() {
        let package = synthetic_package(5, 16, 1);
        let cursor = package.history[1].event_id.clone();
        let expected = package.history[2].event_id.clone();
        let state = SessionContextState::from_package(package);
        let page = history_page(
            &state,
            SessionHistoryParams {
                since: Some(cursor),
                ..Default::default()
            },
        );
        assert_eq!(returned(&page), 3);
        assert_eq!(
            page["items"][0]["eventId"].as_str(),
            Some(expected.as_str())
        );
        assert_eq!(page["cursorResolution"], "resolved");
        assert_eq!(page["offset"], 2);
    }

    #[test]
    fn a_since_cursor_absent_from_the_package_reports_not_in_package_and_returns_no_items() {
        let state = SessionContextState::from_package(package());
        let page = history_page(
            &state,
            SessionHistoryParams {
                since: Some("ff".repeat(32)),
                ..Default::default()
            },
        );
        assert_eq!(page["cursorResolution"], "not_in_package");
        assert_eq!(returned(&page), 0);
        assert!(page["nextCursor"].is_null());
        assert!(page["nextOffset"].is_null());
        assert_eq!(page["provenance"]["omittedHistoryItems"], 3);
        assert!(page["staleness"]["completeAsOfMs"].is_null());
    }

    #[test]
    fn a_cursor_miss_is_stopped_by_cursor_miss_not_by_end() {
        let state = SessionContextState::from_package(package());
        let page = history_page(
            &state,
            SessionHistoryParams {
                since: Some("ff".repeat(32)),
                ..Default::default()
            },
        );
        assert_eq!(page["stoppedBy"], "cursorMiss");
    }

    #[test]
    fn an_index_item_names_which_execution_produced_it_through_the_targets_legend() {
        let package = synthetic_package(4, 16, 2);
        let state = SessionContextState::from_package(package);
        let page = history_page(
            &state,
            SessionHistoryParams {
                view: Some(SessionHistoryView::Index),
                ..Default::default()
            },
        );
        let targets = page["targets"].as_array().expect("targets legend");
        assert_eq!(targets.len(), 2);
        for (offset, item) in page["items"]
            .as_array()
            .expect("index items")
            .iter()
            .enumerate()
        {
            let index = item["targetIndex"].as_u64().expect("targetIndex") as usize;
            assert_eq!(targets[index]["generation"], json!((offset as u64) % 2 + 1));
            assert!(item.get("author").is_none());
            assert!(item.get("sourceKind").is_none());
            assert!(item.get("cursor").is_none(), "eventId is the cursor");
            assert!(item["textPreview"]
                .as_str()
                .is_some_and(|preview| preview.len() <= MAX_INDEX_PREVIEW_BYTES + '…'.len_utf8()));
        }
    }

    #[test]
    fn walking_a_package_by_cursor_yields_every_item_exactly_once() {
        let package = synthetic_package(300, 64, 2);
        let expected = package
            .history
            .iter()
            .map(|item| item.event_id.clone())
            .collect::<Vec<_>>();
        let state = SessionContextState::from_package(package);
        let mut walked = Vec::new();
        let mut cursor = None;
        loop {
            let page = history_page(
                &state,
                SessionHistoryParams {
                    since: cursor.clone(),
                    limit: Some(50),
                    ..Default::default()
                },
            );
            for item in page["items"].as_array().expect("items") {
                walked.push(item["eventId"].as_str().unwrap_or_default().to_owned());
                assert_eq!(item["cursor"], item["eventId"]);
            }
            match page["nextCursor"].as_str() {
                Some(next) => cursor = Some(next.to_owned()),
                None => break,
            }
        }
        assert_eq!(walked, expected);
    }

    #[test]
    fn an_offset_response_always_labels_offset_stability() {
        let state = SessionContextState::from_package(package());
        let offset_page = history_page(&state, SessionHistoryParams::default());
        let cursor_page = history_page(
            &state,
            SessionHistoryParams {
                since: Some(format!("{:064x}", 1)),
                ..Default::default()
            },
        );
        let miss_page = history_page(
            &state,
            SessionHistoryParams {
                since: Some("ff".repeat(32)),
                ..Default::default()
            },
        );
        let search = search_page(
            &state,
            SearchSessionParams {
                query: "closure".into(),
                offset: None,
                limit: None,
            },
        );
        for page in [&offset_page, &cursor_page, &miss_page, &search] {
            assert_eq!(page["offsetStability"], OFFSET_STABILITY);
        }
    }

    #[test]
    fn search_results_carry_a_cursor_and_share_the_byte_budget() {
        let package = synthetic_package(200, 2 * 1024, 1);
        let state = SessionContextState::from_package(package);
        let page = search_page(
            &state,
            SearchSessionParams {
                query: "needle".into(),
                offset: None,
                limit: Some(MAX_SEARCH_LIMIT),
            },
        );
        assert_eq!(page["totalMatches"], 200);
        assert!(returned(&page) < 200);
        assert_eq!(page["stoppedBy"], "pageBytes");
        assert_eq!(page["nextOffset"], json!(returned(&page)));
        assert!(page["nextCursor"].is_null());
        assert!(page["cursorResolution"].is_null());
        for result in page["results"].as_array().expect("results") {
            assert_eq!(result["cursor"], result["eventId"]);
        }
    }

    #[test]
    fn no_tool_can_produce_a_limit_error_naming_a_bound_it_does_not_enforce() {
        let state = SessionContextState::from_package(package());
        assert!(state
            .history(SessionHistoryParams {
                limit: Some(MAX_HISTORY_LIMIT),
                ..Default::default()
            })
            .is_ok());
        let history_error = state
            .history(SessionHistoryParams {
                limit: Some(MAX_HISTORY_LIMIT + 1),
                ..Default::default()
            })
            .expect_err("limit above the enforced maximum");
        assert!(history_error
            .message
            .contains(&format!("between 1 and {MAX_HISTORY_LIMIT}")));

        assert!(state
            .search(SearchSessionParams {
                query: "closure".into(),
                offset: None,
                limit: Some(MAX_SEARCH_LIMIT),
            })
            .is_ok());
        let search_error = state
            .search(SearchSessionParams {
                query: "closure".into(),
                offset: None,
                limit: Some(MAX_SEARCH_LIMIT + 1),
            })
            .expect_err("limit above the enforced maximum");
        assert!(search_error
            .message
            .contains(&format!("between 1 and {MAX_SEARCH_LIMIT}")));
    }

    #[test]
    fn every_response_reports_its_read_time_and_snapshot_age() {
        let state = SessionContextState::from_package(package());
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        let history = history_page(&state, SessionHistoryParams::default());
        let search = search_page(
            &state,
            SearchSessionParams {
                query: "closure".into(),
                offset: None,
                limit: None,
            },
        );
        for response in [&overview, &history, &search] {
            let staleness = &response["staleness"];
            assert!(staleness["readAtMs"].as_i64().is_some_and(|read| read > 0));
            assert_eq!(staleness["projectedAtMs"], 1);
            assert!(staleness["ageSinceProjectionMs"]
                .as_i64()
                .is_some_and(|age| age > 0));
            assert!(staleness["clock"].as_str().is_some());
            assert_eq!(response["packageGeneration"], 0);
            assert_eq!(response["generationChangedSincePreviousCall"], false);
            assert_eq!(response["refreshRefused"], false);
            assert_eq!(response["packageVersion"], 1);
        }
    }

    #[test]
    fn a_snapshot_older_than_the_stale_threshold_is_labelled_stale() {
        let mut fresh = synthetic_package(2, 16, 1);
        fresh.provenance.generated_at = now_ms();
        fresh.provenance.complete = true;
        fresh.provenance.complete_as_of = Some(fresh.provenance.generated_at);
        fresh.provenance.total_history_items = Some(2);
        fresh.validate().expect("fresh package");
        let mut old = fresh.clone();
        old.provenance.generated_at = now_ms() - STALE_AFTER_MS - 1;
        old.provenance.complete_as_of = Some(old.provenance.generated_at);
        old.validate().expect("stale package");

        let fresh_page = history_page(
            &SessionContextState::from_package(fresh),
            SessionHistoryParams::default(),
        );
        assert_eq!(fresh_page["staleness"]["stale"], false);

        let old_page = history_page(
            &SessionContextState::from_package(old),
            SessionHistoryParams::default(),
        );
        assert_eq!(old_page["staleness"]["stale"], true);
        assert!(old_page["staleness"]["ageSinceCompleteAsOfMs"]
            .as_i64()
            .is_some_and(|age| age > STALE_AFTER_MS));
    }

    #[test]
    fn a_package_with_no_complete_as_of_reports_a_null_watermark_and_a_null_age() {
        let state = SessionContextState::from_package(package());
        let page = history_page(&state, SessionHistoryParams::default());
        assert!(page["staleness"]["completeAsOfMs"].is_null());
        assert!(page["staleness"]["ageSinceCompleteAsOfMs"].is_null());
        assert!(page["staleness"]["ageSinceProjectionMs"].as_i64().is_some());
    }

    #[test]
    fn provenance_semantics_reconciles_source_event_count_against_total_history_items() {
        let semantics = provenance_semantics();
        let source = semantics["sourceEventCount"]
            .as_str()
            .expect("sourceEventCount semantics");
        assert!(source.contains("sourceEventBreakdown"));
        assert!(source.contains("== sourceEventCount"));
        assert!(source.contains("exceeds totalHistoryItems"));
        assert!(semantics["readTime"].as_str().is_some());
        assert!(semantics["staleness"].as_str().is_some());
        assert!(semantics["pagination"].as_str().is_some());
        assert!(semantics["packageGeneration"].as_str().is_some());
    }

    #[test]
    fn a_newer_generation_in_the_package_directory_is_picked_up_on_the_next_tool_call() {
        let dir = tempdir().expect("tempdir");
        write_generation(dir.path(), 0, &generation_json(2));
        let state = SessionContextState::load_from_dir(dir.path()).expect("load generation 0");
        let first = history_page(&state, SessionHistoryParams::default());
        assert_eq!(first["packageGeneration"], 0);
        assert_eq!(first["availableHistoryItems"], 2);

        write_generation(dir.path(), 1, &generation_json(3));
        let second = history_page(&state, SessionHistoryParams::default());
        assert_eq!(second["packageGeneration"], 1);
        assert_eq!(second["generationChangedSincePreviousCall"], true);
        assert_eq!(second["availableHistoryItems"], 3);

        let third = history_page(&state, SessionHistoryParams::default());
        assert_eq!(third["packageGeneration"], 1);
        assert_eq!(third["generationChangedSincePreviousCall"], false);
    }

    /// rmcp runs every inbound request on its own task, so two context tool
    /// calls are genuinely concurrent. A response's envelope and its body must
    /// still describe one generation: an agent that reads
    /// `packageGeneration: N` and then quotes the items beside it cannot be
    /// handed generation N+1's history under an N label.
    #[test]
    fn a_concurrent_reload_never_names_a_generation_it_did_not_serve() {
        use std::sync::Arc;

        const GENERATIONS: u64 = 12;
        const READERS: usize = 4;

        let dir = tempdir().expect("tempdir");
        // Generation `seq` holds exactly `seq + 1` history items, so every
        // response carries its own consistency check.
        write_generation(dir.path(), 0, &generation_json(1));
        let state =
            Arc::new(SessionContextState::load_from_dir(dir.path()).expect("load generation 0"));

        let writer = {
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                for seq in 1..=GENERATIONS {
                    write_generation(&path, seq, &generation_json(seq as usize + 1));
                    std::thread::yield_now();
                }
            })
        };
        let readers = (0..READERS)
            .map(|_| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    for _ in 0..GENERATIONS * 4 {
                        let page = history_page(&state, SessionHistoryParams::default());
                        let generation = page["packageGeneration"].as_u64().expect("generation");
                        assert_eq!(
                            page["availableHistoryItems"].as_u64(),
                            Some(generation + 1),
                            "response named generation {generation} but served another one's items"
                        );
                        assert_eq!(returned(&page) as u64, generation + 1);
                        std::thread::yield_now();
                    }
                })
            })
            .collect::<Vec<_>>();

        writer.join().expect("writer");
        for reader in readers {
            reader.join().expect("reader");
        }

        // Once the dust settles the newest generation is the one being served,
        // and it is named by both halves of the response.
        let settled = history_page(&state, SessionHistoryParams::default());
        assert_eq!(settled["packageGeneration"], GENERATIONS);
        assert_eq!(settled["availableHistoryItems"], GENERATIONS + 1);
    }

    #[test]
    fn a_corrupt_newer_generation_is_refused_and_the_last_good_package_keeps_serving() {
        let dir = tempdir().expect("tempdir");
        write_generation(dir.path(), 0, &generation_json(2));
        let state = SessionContextState::load_from_dir(dir.path()).expect("load generation 0");
        let corrupt = dir.path().join(format!("{:010}.json", 1));
        std::fs::write(&corrupt, b"{\"v\": 2, \"session\":").expect("write corrupt generation");
        make_private(&corrupt);

        let page = history_page(&state, SessionHistoryParams::default());
        assert_eq!(page["refreshRefused"], true);
        assert_eq!(page["packageGeneration"], 0);
        assert_eq!(page["availableHistoryItems"], 2);

        // Self-healing: a higher sequence supersedes the corpse.
        write_generation(dir.path(), 2, &generation_json(4));
        let healed = history_page(&state, SessionHistoryParams::default());
        assert_eq!(healed["refreshRefused"], false);
        assert_eq!(healed["packageGeneration"], 2);
        assert_eq!(healed["availableHistoryItems"], 4);
    }

    #[cfg(unix)]
    #[test]
    fn a_newer_generation_with_loose_permissions_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        write_generation(dir.path(), 0, &generation_json(2));
        let state = SessionContextState::load_from_dir(dir.path()).expect("load generation 0");
        let loose = write_generation(dir.path(), 1, &generation_json(3));
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644))
            .expect("chmod public");

        let page = history_page(&state, SessionHistoryParams::default());
        assert_eq!(page["refreshRefused"], true);
        assert_eq!(page["packageGeneration"], 0);
        assert_eq!(page["availableHistoryItems"], 2);
    }

    #[test]
    fn a_tool_argument_can_never_name_a_package_outside_the_launcher_directory() {
        let escape = json!({"path": "/etc/passwd"});
        assert!(serde_json::from_value::<SessionOverviewParams>(escape.clone()).is_err());
        assert!(serde_json::from_value::<SessionHistoryParams>(escape.clone()).is_err());
        assert!(serde_json::from_value::<SearchSessionParams>(json!({
            "query": "x",
            "path": "/etc/passwd"
        }))
        .is_err());
        assert!(serde_json::from_value::<SessionHistoryParams>(json!({
            "packageDir": "/tmp"
        }))
        .is_err());
    }

    #[test]
    fn legacy_single_file_mode_still_serves_all_three_tools() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());
        let state = SessionContextState::load(&path).expect("legacy load");

        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        assert_eq!(overview["packageGeneration"], 0);
        assert_eq!(overview["availableHistoryItems"], 2);

        let history = history_page(&state, SessionHistoryParams::default());
        assert_eq!(returned(&history), 2);
        assert_eq!(history["stoppedBy"], "end");
        assert_eq!(history["refreshRefused"], false);

        let search = search_page(
            &state,
            SearchSessionParams {
                query: "closure".into(),
                offset: None,
                limit: None,
            },
        );
        assert_eq!(search["totalMatches"], 1);
    }

    #[test]
    fn a_cursor_issued_before_a_generation_change_still_resolves_after_it() {
        let dir = tempdir().expect("tempdir");
        write_generation(dir.path(), 0, &generation_json(5));
        let state = SessionContextState::load_from_dir(dir.path()).expect("load generation 0");
        let first = history_page(
            &state,
            SessionHistoryParams {
                limit: Some(2),
                ..Default::default()
            },
        );
        let cursor = first["nextCursor"]
            .as_str()
            .expect("cursor mid-walk")
            .to_owned();

        write_generation(dir.path(), 1, &generation_json(8));
        let resumed = history_page(
            &state,
            SessionHistoryParams {
                since: Some(cursor),
                ..Default::default()
            },
        );
        assert_eq!(resumed["generationChangedSincePreviousCall"], true);
        assert_eq!(resumed["cursorResolution"], "resolved");
        assert_eq!(returned(&resumed), 6);
        assert_eq!(
            resumed["items"][0]["eventId"].as_str(),
            Some(format!("{:064x}", 3).as_str())
        );
    }
}
