// One build stamp, in the shape NIP-11 already publishes.
//
// The relay answers "what am I running" over NIP-11: `software_commit` (a full
// 40-hex commit, or the literal `unknown`), `software_commit_count` (that same
// commit's `git rev-list --count`, or JSON `null`) and `build_time`. Both
// non-answers are *disclosed*, never errors and never guesses — that
// distinction is the whole point of the fields (finding 32).
//
// The client had no such answer. `bee --version` named a commit but not when it
// was built, and the desktop's stamp lived in its own build script with its own
// spelling of the same rules. This module is the shared one: the type, the
// validation, and the sentences for the two non-answers, so a client and the
// relay can be compared field by field without either side re-deciding what
// `unknown` means.
//
// Plain `//` comments, not `//!`: this file is `include!`d into
// `crates/beekeeper-core/build.rs` mid-stream, where an inner doc comment is not
// allowed. It is compiled into the crate proper as well, so the rules are
// covered by `cargo test -p beekeeper-core build_info` — a build script's own code
// is never run by `cargo test`, and this repository has already paid twice for
// resolution logic that lived only there (REVIEW-A1 F1, F12).

/// What a build discloses when it cannot name its commit.
///
/// The literal NIP-11 `software_commit` serves, and the literal `bee
/// --version` prints. Never `""`, never omitted, never a guess.
pub const UNKNOWN_COMMIT: &str = "unknown";

/// What a build discloses when it cannot name its commit's ordinal.
///
/// Rendered where a number would otherwise go; NIP-11 serializes the same
/// absence as JSON `null`.
pub const UNKNOWN_COUNT: &str = "null";

/// Hex digits of a commit shown to a person. Eight, matching `bee git check
/// --ref`'s own truncation and the desktop's About row, so the same build
/// reads the same everywhere.
pub const SHORT_COMMIT_HEX: usize = 8;

/// A full SHA-1 object name, in hex.
const FULL_SHA_HEX: usize = 40;

/// Whether `value` is a full, lowercase-hex commit object name.
///
/// The same predicate the relay applies before serving `software_commit`
/// (`crates/beekeeper-relay/src/build_provenance.rs`): a short SHA, mixed case or
/// non-hex value is refused rather than carried, because a reader with no
/// checkout cannot disambiguate an abbreviation and a value that cannot be
/// looked up is worse than a disclosed `unknown`.
pub fn is_full_sha(value: &str) -> bool {
    value.len() == FULL_SHA_HEX
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A `git rev-list --count` result, parsed.
///
/// Rejects the empty string, [`UNKNOWN_COMMIT`], signs, leading zeros (git
/// never pads), inner whitespace, anything non-decimal — and `0`. A real
/// commit's count is at minimum 1 (itself), so a `0` can only come from a
/// broken pipeline, and unlike a malformed string a `0` would take part in a
/// consumer's subtraction and read as agreement.
pub fn parse_commit_count(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value == UNKNOWN_COMMIT {
        return None;
    }
    if value.len() > 1 && value.starts_with('0') {
        return None;
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let count: u64 = value.parse().ok()?;
    (count >= 1).then_some(count)
}

/// The commit a binary was built from, that commit's ordinal, and when.
///
/// Every field is independently absent-able and every absence is disclosed
/// rather than filled in. The commit and the count are resolved **together**
/// by the build script or not at all: a count from this checkout beside a SHA
/// from `BUZZ_SOURCE_SHA` would describe two different histories in two
/// fields that look internally consistent, which no consumer could detect.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildInfo {
    /// Full 40-hex commit, or `None` — rendered [`UNKNOWN_COMMIT`].
    pub commit: Option<String>,
    /// `git rev-list --count` of [`Self::commit`], or `None` — rendered
    /// [`UNKNOWN_COUNT`]. Never present without a commit it describes.
    pub commit_count: Option<u64>,
    /// RFC 3339 UTC second-precision stamp, or [`UNKNOWN_COMMIT`]'s literal
    /// when the build environment supplied none.
    ///
    /// A `String` rather than an `Option`, matching NIP-11's `build_time`: a
    /// wall-clock read does not fail the way a git lookup does, so the absent
    /// case is a packaging choice (a reproducible build pinning the stamp),
    /// not a measurement failure.
    ///
    /// Precisely, this is **when the stamp was taken** — the last run of
    /// `build.rs` at which the resolved commit changed — not the instant the
    /// linker finished. Cargo re-runs a build script only when a watched path
    /// moves, so no build script in this repository can claim the latter. For
    /// a clean checkout the two coincide; for a working tree being edited the
    /// stamp already reads `<sha>-dirty`, which is the disclosure that
    /// matters.
    pub build_time: String,
    /// `Some(true)` only when the build script *observed* a modified working
    /// tree.
    ///
    /// Never `Some(false)`. Cargo cannot cheaply watch every untracked path,
    /// so a clean claim could survive an incremental rebuild that made it
    /// false; `None` therefore means "not observed dirty", which a consumer
    /// must not render as "clean". Same contract as the desktop's
    /// `AppBuildIdentity::source_dirty`.
    pub source_dirty: Option<bool>,
}

impl BuildInfo {
    /// Build an identity from the four raw compile-time stamps, applying the
    /// validation above. Pure, so the rules are testable without a build.
    pub fn from_stamps(
        commit: Option<&str>,
        commit_count: Option<&str>,
        build_time: Option<&str>,
        source_dirty: Option<&str>,
    ) -> Self {
        let commit = commit
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .filter(|sha| is_full_sha(sha));
        // A count without a commit describes nothing, so it is dropped rather
        // than carried — the pair is the unit of meaning.
        let commit_count = commit
            .as_ref()
            .and(commit_count)
            .and_then(parse_commit_count);
        let build_time = build_time
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(UNKNOWN_COMMIT)
            .to_owned();
        Self {
            commit,
            commit_count,
            build_time,
            source_dirty: match source_dirty.map(str::trim) {
                Some("1") => Some(true),
                _ => None,
            },
        }
    }

    /// The commit as a person reads it: eight hex, or [`UNKNOWN_COMMIT`].
    ///
    /// A `-dirty` suffix is appended when the tree was observed modified,
    /// because a bare commit name claims the binary contains exactly that
    /// commit and a build from a modified tree does not. `unknown` is already
    /// the absence of a commit and is never decorated — there is nothing for
    /// the suffix to qualify.
    pub fn short_commit(&self) -> String {
        match &self.commit {
            Some(sha) => {
                let short = &sha[..SHORT_COMMIT_HEX];
                if self.source_dirty == Some(true) {
                    format!("{short}-dirty")
                } else {
                    short.to_owned()
                }
            }
            None => UNKNOWN_COMMIT.to_owned(),
        }
    }

    /// The full commit, or [`UNKNOWN_COMMIT`] — what a tooltip shows.
    pub fn commit_display(&self) -> String {
        self.commit.clone().unwrap_or(UNKNOWN_COMMIT.to_owned())
    }

    /// The ordinal, or [`UNKNOWN_COUNT`] — NIP-11's `null`, spelled out.
    pub fn commit_count_display(&self) -> String {
        self.commit_count
            .map_or(UNKNOWN_COUNT.to_owned(), |count| count.to_string())
    }
}

/// `<commit|unknown> (count <n|null>, built <time>)` — every field present,
/// each absence disclosed in the spelling NIP-11 uses for it.
impl std::fmt::Display for BuildInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (count {}, built {})",
            self.short_commit(),
            self.commit_count_display(),
            self.build_time
        )
    }
}

/// This crate's own build stamp — and so the stamp of anything linking it.
///
/// The values come from `crates/beekeeper-core/build.rs`, which resolves them from
/// `BUZZ_SOURCE_SHA`/`BUZZ_SOURCE_COMMIT_COUNT` (what a packaging pipeline
/// states, including `scripts/app-from.sh`) when set, else from the checkout,
/// else nothing. `option_env!` rather than `env!` throughout: a build that
/// could answer nothing must still compile and then *say* it could answer
/// nothing.
pub fn build_info() -> BuildInfo {
    BuildInfo::from_stamps(
        option_env!("BUZZ_CORE_SOURCE_SHA"),
        option_env!("BUZZ_CORE_SOURCE_COMMIT_COUNT"),
        option_env!("BUZZ_CORE_BUILD_TIME"),
        option_env!("BUZZ_CORE_SOURCE_DIRTY"),
    )
}

/// RFC 3339 UTC timestamp for `epoch_secs`, second precision.
///
/// Hand-rolled rather than `chrono`, because `build.rs` calls it and a build
/// script cannot depend on its own crate's dependencies.
pub fn rfc3339_utc(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Now, as [`rfc3339_utc`] renders it.
pub fn rfc3339_utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339_utc(now.as_secs())
}

/// Days since the Unix epoch to a proleptic Gregorian `(year, month, day)`.
/// Howard Hinnant's `civil_from_days`, public domain:
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "f723824d68183a1a2c4c7925673853694825f973";

    #[test]
    fn a_full_lowercase_hex_commit_is_the_only_accepted_shape() {
        assert!(is_full_sha(SHA));
        assert!(!is_full_sha(&SHA[..39]), "too short");
        assert!(!is_full_sha(&format!("{SHA}a")), "too long");
        assert!(!is_full_sha(&SHA.to_ascii_uppercase()), "uppercase");
        assert!(!is_full_sha("f723824d"), "an abbreviation is not a commit");
        assert!(!is_full_sha(""));
    }

    #[test]
    fn a_count_is_git_s_own_output_or_nothing() {
        assert_eq!(parse_commit_count("17544"), Some(17_544));
        assert_eq!(parse_commit_count(" 17544\n"), Some(17_544));
        assert_eq!(parse_commit_count("1"), Some(1));
        assert_eq!(parse_commit_count("0"), None, "0 would subtract as data");
        assert_eq!(parse_commit_count("017"), None, "git never pads");
        assert_eq!(parse_commit_count("-3"), None);
        assert_eq!(parse_commit_count("unknown"), None);
        assert_eq!(parse_commit_count(""), None);
        assert_eq!(parse_commit_count("17 544"), None);
    }

    #[test]
    fn a_complete_stamp_survives_intact() {
        let info =
            BuildInfo::from_stamps(Some(SHA), Some("17544"), Some("2026-09-05T12:00:00Z"), None);
        assert_eq!(info.commit.as_deref(), Some(SHA));
        assert_eq!(info.commit_count, Some(17_544));
        assert_eq!(info.short_commit(), "f723824d");
        assert_eq!(
            info.to_string(),
            "f723824d (count 17544, built 2026-09-05T12:00:00Z)"
        );
    }

    /// The distinction the fields exist for: a build that could determine
    /// nothing says so, in the two spellings NIP-11 uses, and never as an
    /// error or an empty string.
    #[test]
    fn a_build_that_knows_nothing_discloses_it() {
        let info = BuildInfo::from_stamps(None, None, None, None);
        assert_eq!(info.commit, None);
        assert_eq!(info.commit_count, None);
        assert_eq!(info.short_commit(), "unknown");
        assert_eq!(info.commit_display(), "unknown");
        assert_eq!(info.commit_count_display(), "null");
        assert_eq!(info.build_time, "unknown");
        assert_eq!(info.to_string(), "unknown (count null, built unknown)");
    }

    /// A count belongs to the commit beside it. With no commit there is
    /// nothing for it to be the ordinal *of*, so it is dropped rather than
    /// served as a free-floating number.
    #[test]
    fn a_count_without_a_commit_is_dropped() {
        let info = BuildInfo::from_stamps(None, Some("17544"), None, None);
        assert_eq!(info.commit_count, None);
        assert_eq!(info.commit_count_display(), "null");
    }

    #[test]
    fn a_malformed_commit_reads_as_unknown_not_as_itself() {
        for bad in ["not-a-sha", "F723824D68183A1A2C4C7925673853694825F973x", ""] {
            let info = BuildInfo::from_stamps(Some(bad), Some("17544"), None, None);
            assert_eq!(info.commit, None, "{bad:?} must not be carried");
        }
        // Case is normalised, not refused: git itself prints lowercase, but a
        // pipeline that upcased the value still named this commit.
        let info = BuildInfo::from_stamps(Some(&SHA.to_ascii_uppercase()), None, None, None);
        assert_eq!(info.commit.as_deref(), Some(SHA));
    }

    /// A commit name alone claims the binary contains exactly that commit.
    /// A build from a modified tree does not, and says so.
    #[test]
    fn a_dirty_build_is_stamped_dirty_but_an_unknown_one_is_never_decorated() {
        let dirty = BuildInfo::from_stamps(Some(SHA), None, None, Some("1"));
        assert_eq!(dirty.short_commit(), "f723824d-dirty");
        let unknown = BuildInfo::from_stamps(None, None, None, Some("1"));
        assert_eq!(unknown.short_commit(), "unknown");
        // Only the literal `1` is an observation; anything else is not one.
        let quiet = BuildInfo::from_stamps(Some(SHA), None, None, Some("0"));
        assert_eq!(quiet.source_dirty, None, "never a clean claim");
        assert_eq!(quiet.short_commit(), "f723824d");
    }

    #[test]
    fn rfc3339_renders_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_757_073_600), "2025-09-05T12:00:00Z");
        let now = rfc3339_utc_now();
        assert_eq!(now.len(), "2026-09-05T12:00:00Z".len(), "{now}");
        assert!(now.ends_with('Z'), "{now}");
    }

    /// This binary's own stamp must parse as one, whatever the build
    /// environment could determine — the test that would have caught a build
    /// script emitting a value the type refuses.
    #[test]
    fn this_builds_own_stamp_is_well_formed() {
        let info = build_info();
        if let Some(commit) = &info.commit {
            assert!(is_full_sha(commit), "{commit}");
            assert_eq!(
                info.short_commit().len(),
                SHORT_COMMIT_HEX + usize::from(info.source_dirty == Some(true)) * 6
            );
        } else {
            assert_eq!(info.short_commit(), UNKNOWN_COMMIT);
        }
        assert!(!info.build_time.is_empty());
    }
}
