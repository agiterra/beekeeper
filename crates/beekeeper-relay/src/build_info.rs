//! Build-time identity compiled into the relay binary.

/// Full 40-hex source commit SHA, or `unknown`.
///
/// Set by `build.rs` (`BUZZ_RELAY_SOURCE_SHA`): `git rev-parse HEAD` when
/// this crate's own `.git` is present (a native build), else the
/// `BUZZ_SOURCE_SHA` build-arg the Dockerfile threads through from CI/deploy
/// (`deploy/autodeploy/autodeploy`) for the
/// case `.git` is not present — the relay's Docker build context is a `git
/// archive` export with no `.git` at all (see `.dockerignore`). Always set:
/// `build.rs` falls back to the literal `unknown` when neither source
/// answers, so this is never missing at compile time.
///
/// Advertised as NIP-11 `software_commit` (`nip11.rs`) and `GET /health`'s
/// second token (`router.rs`). Finding 32
/// (`review-2026-09-01/LIVE-RUN-TeamRolesV1.md`): without this, whether a
/// push had actually redeployed hive could not be observed by mechanism.
pub(crate) fn source_sha() -> &'static str {
    env!("BUZZ_RELAY_SOURCE_SHA")
}

/// `git rev-list --count` of [`source_sha`] — the number of commits
/// reachable from it, inclusive — or `None`.
///
/// Set by `build.rs` alongside [`source_sha`] and always describing the
/// **same** commit: `resolve_stamp` resolves the pair from one source or
/// yields no count at all, so this can never be an ordinal from a different
/// history than the SHA beside it.
///
/// `None` whenever the count could not be established honestly: the
/// `BUZZ_SOURCE_COMMIT_COUNT` build-arg absent or malformed on a Docker
/// build (whose context has no `.git`), a shallow checkout — where
/// `rev-list --count` returns the graft's size rather than the true ordinal
/// — or no git at all. Advertised as NIP-11 `software_commit_count`, where
/// it serializes as `null`: the same disclosed non-answer `software_commit`'s
/// literal `unknown` is, never omitted and never guessed.
pub(crate) fn source_commit_count() -> Option<u32> {
    // `build.rs` writes either a `parse_commit_count`-validated decimal or
    // the empty string, so this only distinguishes the two. The `>= 1` guard
    // is kept rather than assumed: `0` is the value a shallow or broken
    // pipeline would produce, and it is the one number that would subtract
    // as data instead of reading as absent.
    match env!("BUZZ_RELAY_SOURCE_COMMIT_COUNT") {
        "" => None,
        value => value.parse().ok().filter(|count| *count >= 1),
    }
}

/// RFC 3339 UTC timestamp this binary was compiled, second precision.
///
/// Set by `build.rs` (`BUZZ_RELAY_BUILD_TIME`) from the build machine's
/// clock at `cargo build` time — always set, never `unknown`: a wall-clock
/// read cannot fail the way a git lookup can. Advertised as NIP-11
/// `build_time`.
pub(crate) fn build_time() -> &'static str {
    env!("BUZZ_RELAY_BUILD_TIME")
}

/// Stable build identifier, or `local` outside CI.
pub(crate) fn build_id() -> &'static str {
    option_env!("BUZZ_BUILD_ID").unwrap_or("local")
}

/// Build details URL, or `unknown` outside CI.
pub(crate) fn build_url() -> &'static str {
    option_env!("BUZZ_BUILD_URL").unwrap_or("unknown")
}
