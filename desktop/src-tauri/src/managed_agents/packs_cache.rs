//! The packs a project's seats are staged from, fetched from a git repository.
//!
//! # Why a repository
//!
//! Role packs are trees of text — seven roles are roughly 200 KB — and until
//! this module existed they lived only in whatever folder an operator happened
//! to point the crew-role installer at. That made a role a property of one
//! computer: an evolved `builder` pack on this machine was invisible on Andy's,
//! and a person logging into Beekeeper from a second machine got their agents
//! with no packs behind them.
//!
//! So the packs live in a git repository on the relay, versioned and
//! reviewable, and the wire carries **the pointer and the proof**: a project's
//! kind:30624 record names the repository and either a branch or an exact
//! commit, and the seat's kind:44223 carries the `packRef` — repo, sha, role,
//! path — of what was actually staged. A reader can therefore answer "which
//! pack ran" from the wire alone rather than from a claim.
//!
//! # What this module does
//!
//! Given a decoded 30624 ([`ProjectPackSource`]) it clones or fetches the
//! repository into this host's packs cache, checks out the pinned commit (or
//! the named ref's tip, recording the commit it resolved to), and hands back
//! the directory holding one role's pack plus the [`PackRef`] describing it.
//!
//! # What it refuses to do
//!
//! - **It never guesses.** A repository that cannot be fetched, a commit that
//!   is not in it, or a role directory that is not there is
//!   [`HIRE_PACK_UNAVAILABLE`] — a refused hire naming the reason — never a
//!   silent bare persona. A project that says its seats come from a pack and
//!   then seats one without it is exactly the class of untruth the wire
//!   contract exists to remove.
//! - **It never lets the wire name a path on this machine.** The repository
//!   coordinate, the ref, the sha, the sub-path and the role are all
//!   validated here before they reach `git`, and the checkout is confined to
//!   this host's own cache directory.

use std::path::{Path, PathBuf};

use crate::commands::project_git_exec::{run_git, validate_clone_url, GitAuthConfig};

/// Where in a packs repository the role directories live when a 30624 names
/// no `path` tag — the wire's own default, re-exported so the host and the
/// relay cannot disagree about it.
pub use buzz_core_pkg::project_pack_source::DEFAULT_PACK_PATH;

/// The refusal a hire carries when a project's packs cannot be staged.
///
/// One sentence, and it says the two things an operator needs: that the packs
/// were promised by the project rather than by this computer, and that the
/// seat was **not** started without them. A seat quietly launched on a bare
/// persona would look like a working hire and behave like an agent that forgot
/// its craft.
pub const HIRE_PACK_UNAVAILABLE: &str =
    "This project stages its agents from a packs repository, and this computer could not \
     read the pack for that role — the seat was not started, because an agent seated \
     without its role pack is not the agent the project asked for.";

/// How much of the repository owner's key names its directory in the cache.
const OWNER_PREFIX: usize = 8;

/// Longest role slug, sub-path segment, and repository id this module accepts.
const MAX_SLUG_BYTES: usize = 64;

/// The pack that was staged for one seat, as it reaches the wire.
///
/// Field-for-field the kind:44223 `packRef` object, declared once in
/// `buzz-core` and re-exported here: the host writes it into the seat file and
/// the provider publishes it, so one declaration is the only way the two can
/// be guaranteed to be the same four keys.
pub use buzz_core_pkg::coding_session_payload::PackRef;

/// A project's kind:30624 pack source, in the shape the host stages from.
///
/// `buzz_core::project_pack_source::ProjectPackSource` is the decoder's own
/// type and keeps the pin as a `PackPin` enum; this one keeps the two halves
/// apart because that is how the renderer hands them across the Tauri
/// boundary (`ProjectPackSourceInput`, `actor_seats.rs`). [`Self::target`]
/// collapses them back to exactly one, refusing both-or-neither the way the
/// decoder does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectPackSource {
    /// `30617:<owner-hex>:<id>` — the packs repository announcement.
    pub repo: String,
    /// `refs/heads/main`, when the source follows a branch.
    pub git_ref: Option<String>,
    /// A pinned commit, when the source pins one. Exactly one of this and
    /// [`Self::git_ref`] is set; both or neither is a malformed source.
    pub sha: Option<String>,
    /// Sub-path holding the role directories. [`DEFAULT_PACK_PATH`] when the
    /// record names none.
    pub path: String,
}

impl ProjectPackSource {
    /// The commit-ish this source asks for, as a `(kind, value)` pair, or the
    /// reason it names none.
    fn target(&self) -> Result<PackTarget, String> {
        match (self.git_ref.as_deref(), self.sha.as_deref()) {
            (Some(_), Some(_)) => {
                Err("a pack source names both a ref and a sha; exactly one is allowed".to_string())
            }
            (None, None) => Err("a pack source names neither a ref nor a sha".to_string()),
            (Some(reference), None) => Ok(PackTarget::Ref(validate_branch_ref(reference)?)),
            (None, Some(sha)) => Ok(PackTarget::Sha(validate_sha(sha)?)),
        }
    }
}

/// What the checkout is asked to land on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PackTarget {
    /// A branch name with its `refs/heads/` prefix stripped.
    Ref(String),
    /// A lowercase 40-hex commit.
    Sha(String),
}

/// A staged role pack: where it is on this computer, and what it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedProjectPack {
    /// Directory of the role's pack inside the checkout — what the seat's
    /// `packDir` points at.
    pub dir: PathBuf,
    /// The persona inside [`Self::dir`] that declares the role — the seat's
    /// `personaId`.
    pub persona: String,
    /// The wire's account of it.
    pub pack_ref: PackRef,
}

/// Split `30617:<owner-hex>:<id>` into its owner and repository id.
///
/// Validated here rather than trusted: the coordinate arrives from a signed
/// event that this computer did not author, and both halves become path
/// segments in a URL and in this host's cache directory.
pub fn parse_repo_coordinate(coordinate: &str) -> Result<(String, String), String> {
    let mut parts = coordinate.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    let id = parts.next().unwrap_or_default();
    if kind != "30617" {
        return Err(format!(
            "a packs repository coordinate must be 30617:<owner>:<id>, not {coordinate:?}"
        ));
    }
    if !crate::managed_agents::is_lowercase_hex_pubkey(owner) {
        return Err("a packs repository owner must be 64-character lowercase hex".to_string());
    }
    if !is_repo_id(id) {
        return Err(format!(
            "a packs repository id is not a repository id: {id:?}"
        ));
    }
    Ok((owner.to_string(), id.to_string()))
}

/// Repository ids are the same conservative slug the relay's git routes serve.
fn is_repo_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SLUG_BYTES
        && !value.starts_with('-')
        && !value.starts_with('.')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !value.contains("..")
}

/// A role slug, as the wire's seat contract defines it.
pub fn is_role_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SLUG_BYTES
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_sha(value: &str) -> Result<String, String> {
    let sha = value.trim();
    if sha.len() == 40
        && sha
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    {
        return Ok(sha.to_string());
    }
    Err(format!(
        "a pinned pack commit must be 40-character lowercase hex, not {value:?}"
    ))
}

fn validate_branch_ref(value: &str) -> Result<String, String> {
    let branch = value.trim().strip_prefix("refs/heads/").unwrap_or(value);
    crate::commands::project_git_exec::clean_branch(Some(branch.to_string()))
        .ok_or_else(|| format!("a pack source ref is not a branch name: {value:?}"))
}

/// Validate the 30624 `path` tag into a relative sub-path of the repository.
///
/// Rejects absolute paths, traversal, and anything a shell or `git` could read
/// as an option, so the wire can name a directory *inside* the checkout and
/// nothing else.
pub fn validate_pack_path(value: &str) -> Result<String, String> {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        return Ok(DEFAULT_PACK_PATH.to_string());
    }
    let segments: Vec<&str> = path.split('/').collect();
    let acceptable = segments.iter().all(|segment| {
        !segment.is_empty()
            && *segment != "."
            && *segment != ".."
            && !segment.starts_with('-')
            && segment.len() <= MAX_SLUG_BYTES
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    });
    if !acceptable {
        return Err(format!(
            "a pack source path is not a repository path: {value:?}"
        ));
    }
    Ok(segments.join("/"))
}

/// This host's packs cache: `<app data>/packs`.
///
/// A sibling of `session-provider`, and for the same reason — it is host-local
/// execution state, created on demand so a fresh install needs no migration.
pub fn packs_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?
        .join("packs");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create the packs cache: {error}"))?;
    Ok(dir)
}

/// The packs bundled into the app, as a `PackRef.repo` value.
///
/// Deliberately not a coordinate: nothing on the relay announces these, and
/// writing a repository that does not exist into the field a reader uses to go
/// and look would be worse than saying plainly where they came from. The
/// matching `sha` is the app's own version, which is exactly what pins them.
pub use buzz_core_pkg::project_pack_source::PACK_REF_SHIPPED_REPO;

/// The seven role packs this build ships, or `None` when this build has none.
///
/// A packaged app carries `personas/roles` as a bundle resource; a development
/// build has no resource directory, so the checkout the binary was built from
/// answers instead. Either way the answer is a directory that really holds
/// packs — never a path guessed and handed on unchecked.
///
/// This is the **last** fallback, and it is why a person who installs
/// Beekeeper on a second machine and hires an architect gets an architect: no
/// project record, no folder to pick, no runbook.
pub fn shipped_packs_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    if let Ok(resource) = app
        .path()
        .resolve(DEFAULT_PACK_PATH, tauri::path::BaseDirectory::Resource)
    {
        if resource.is_dir() {
            return Some(resource);
        }
    }
    // Development: the app runs out of `target/debug` with no resource dir.
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join(DEFAULT_PACK_PATH);
    checkout.is_dir().then_some(checkout)
}

/// The `packRef` for an *installed* pack that turns out to be one of the packs
/// this build ships, or `None` when it came from somewhere else.
///
/// The crew-role installer points an agent's `persona_team_dir` at whatever
/// folder of role packs the operator chose. On a development build that folder
/// is very often the checkout's own `personas/roles` — the same directory
/// [`shipped_packs_dir`] answers with — and then "installed" and "shipped" name
/// the same bytes. `plan_seat_pack` reached the installed arm first and
/// published `packRef: null`, so a seat staged from the shipped defaults said
/// on the wire that nothing vouched for its pack (the `packRef` half of
/// finding 53, measured on run 5: nine role agents, every `persona_team_dir`
/// under the shipped directory, no `packRef` on any 44223).
///
/// This is a recognition, never a guess: the answer is `Some` only when
/// `pack_dir` is `<shipped>/<role>`, compared as canonical paths so a symlinked
/// or `..`-laden route to the same directory is the same directory. A pack from
/// anywhere else keeps `None`, because no repository — not even this app's own
/// version — can vouch for it.
pub fn shipped_pack_ref_for_dir(
    shipped_root: Option<&Path>,
    pack_dir: &Path,
    role: &str,
    version: &str,
) -> Option<PackRef> {
    let role = role.trim();
    if role.is_empty() {
        return None;
    }
    let expected = shipped_root?.join(role);
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if canonical(&expected) != canonical(pack_dir) {
        return None;
    }
    Some(PackRef {
        repo: PACK_REF_SHIPPED_REPO.to_string(),
        sha: version.to_string(),
        role: role.to_string(),
        path: format!("{DEFAULT_PACK_PATH}/{role}"),
    })
}

/// The `sha` a shipped pack is pinned by: this app's version.
pub fn shipped_packs_version(app: &tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The role directory inside a *session checkout*, when it holds one.
///
/// A project that keeps its packs in the repository it is being worked on gets
/// them without announcing anything: `<checkout>/personas/roles/<role>`. The
/// checkout is only ever read — it never becomes a git remote, a write target,
/// or a clone destination — so it is the same read-only use the role-pack
/// installer's own scan already makes of a chosen folder
/// (`crew_roles::scan_project_role_packs`).
pub fn checkout_role_pack(checkout: &Path, role: &str) -> Option<(PathBuf, String)> {
    role_pack_in_checkout(checkout, DEFAULT_PACK_PATH, role)
}

/// This host's checkout directory for one packs repository.
///
/// `<packs root>/<owner-prefix>-<id>`: short enough to read, keyed by both
/// halves of the coordinate so two projects' packs never share a checkout.
pub fn packs_checkout_dir(packs_root: &Path, owner: &str, id: &str) -> PathBuf {
    packs_root.join(pack_cache_dir_name(owner, id))
}

/// The directory name one packs repository occupies: `<owner8>-<id>`.
///
/// Named to match `buzz_core::project_pack_source::pack_cache_dir_name` so the
/// CLI's `bee packs status` and this host name the same directory; the CLI
/// takes the whole coordinate and this takes it already split, because the
/// caller here has always just validated both halves.
///
/// A test in this module asserts the two agree, because a disagreement would
/// be invisible: the CLI would report an empty cache while the host had one.
pub fn pack_cache_dir_name(owner: &str, id: &str) -> String {
    let owner_prefix: String = owner.chars().take(OWNER_PREFIX).collect();
    format!("{owner_prefix}-{id}")
}

/// The relay git URL a packs repository is cloned from.
///
/// The relay serves every repository at `<origin>/git/<owner>/<id>` — the same
/// URL `bee git setup` writes as `origin` — so a packs repository needs no
/// separate hosting and no separate credential path: the existing
/// `git-credential-nostr` helper signs for it like any other.
pub fn packs_clone_url(relay_http_base: &str, owner: &str, id: &str) -> String {
    format!("{}/git/{owner}/{id}", relay_http_base.trim_end_matches('/'))
}

/// Clone or update `checkout` from `clone_url` and land it on `source`'s
/// commit, returning the commit it landed on.
///
/// Idempotent: a checkout that already exists is fetched rather than
/// re-cloned, and a checkout already sitting on a pinned commit needs no
/// network at all — a pinned source is the common case for a project that
/// wants every machine on the same pack, and re-fetching per hire would make
/// every seat wait on the relay.
pub fn sync_packs_checkout(
    checkout: &Path,
    clone_url: &str,
    source: &ProjectPackSource,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    let target = source.target()?;
    let checkout_path = checkout
        .to_str()
        .ok_or_else(|| "the packs cache path is not valid UTF-8".to_string())?;

    if !checkout.join(".git").is_dir() {
        if checkout.exists() {
            std::fs::remove_dir_all(checkout)
                .map_err(|error| format!("failed to clear {}: {error}", checkout.display()))?;
        }
        if let Some(parent) = checkout.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create the packs cache: {error}"))?;
        }
        run_git(
            &["clone", "--quiet", "--", clone_url, checkout_path],
            None,
            auth,
        )?;
    }

    let wanted = match &target {
        // A pinned commit already in the object store needs no fetch: the
        // object cannot change, so the network can only confirm what is here.
        PackTarget::Sha(sha) => {
            if has_commit(checkout, sha, auth) {
                sha.clone()
            } else {
                fetch(checkout, clone_url, auth)?;
                if !has_commit(checkout, sha, auth) {
                    return Err(format!(
                        "the packs repository does not contain commit {sha}"
                    ));
                }
                sha.clone()
            }
        }
        PackTarget::Ref(branch) => {
            fetch(checkout, clone_url, auth)?;
            let remote_ref = format!("refs/remotes/origin/{branch}");
            run_git(
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{remote_ref}^{{commit}}"),
                ],
                Some(checkout),
                auth,
            )
            .map_err(|error| format!("the packs repository has no branch {branch}: {error}"))?
            .trim()
            .to_string()
        }
    };

    if wanted.is_empty() {
        return Err("the packs repository resolved to no commit".to_string());
    }
    run_git(
        &["checkout", "--detach", "--force", "--quiet", &wanted],
        Some(checkout),
        auth,
    )?;
    // `git checkout` leaves files a previous commit added but this one does
    // not have. A stale role directory would stage a pack the wire's sha does
    // not describe, which is worse than staging none.
    run_git(
        &["clean", "-x", "-d", "--force", "--quiet"],
        Some(checkout),
        auth,
    )?;
    Ok(wanted)
}

fn fetch(checkout: &Path, clone_url: &str, auth: &GitAuthConfig) -> Result<(), String> {
    run_git(
        &[
            "fetch",
            "--quiet",
            "--prune",
            "--",
            clone_url,
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        Some(checkout),
        auth,
    )
    .map(|_| ())
}

fn has_commit(checkout: &Path, sha: &str, auth: &GitAuthConfig) -> bool {
    run_git(
        &["cat-file", "-e", &format!("{sha}^{{commit}}")],
        Some(checkout),
        auth,
    )
    .is_ok()
}

/// The persona inside `pack_dir` that declares `role`, if any.
///
/// The one question the staging rule asks of a directory: *does this pack hold
/// the role this seat was created with?* Answered by resolving the pack and
/// reading the persona's own `role:` frontmatter — the same resolver the
/// provider will use to materialize the seat's skills — so a directory named
/// after a role but holding something else is not mistaken for that role's
/// pack. Never guessed from the directory name.
///
/// `None` for a directory that is not a pack, a pack that cannot be read, and
/// a pack whose personas declare some other role or none.
pub fn role_persona_in_pack(pack_dir: &Path, role: &str) -> Option<String> {
    let wanted = role.trim();
    if wanted.is_empty() || !pack_dir.join(".plugin").join("plugin.json").is_file() {
        return None;
    }
    let resolved = match buzz_persona_pkg::resolve::resolve_pack(pack_dir) {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::debug!(
                pack = %pack_dir.display(),
                %error,
                "a pack that cannot be read holds no role"
            );
            return None;
        }
    };
    resolved
        .personas
        .into_iter()
        .find(|persona| {
            persona
                .role
                .as_deref()
                .map(str::trim)
                .is_some_and(|declared| declared == wanted)
        })
        .map(|persona| persona.name)
}

/// The pack one role occupies inside a synced checkout, as `(dir, persona)`.
///
/// The role names a directory, but the *directory name is not the test*: the
/// pack there must hold a persona declaring that role, checked with the same
/// resolver the provider uses. A directory called `builder` whose persona
/// declares nothing is not a builder pack, and staging it would hand a seat
/// skills the wire says are a different role's.
///
/// `None` when the repository holds no such pack — a fact the caller turns
/// into [`HIRE_PACK_UNAVAILABLE`], because the project said its seats come
/// from this repository and this role is not in it.
pub fn role_pack_in_checkout(checkout: &Path, path: &str, role: &str) -> Option<(PathBuf, String)> {
    if !is_role_slug(role) {
        return None;
    }
    let mut dir = checkout.to_path_buf();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        dir.push(segment);
    }
    dir.push(role);
    if !dir.is_dir() {
        return None;
    }
    let persona = role_persona_in_pack(&dir, role)?;
    Some((dir, persona))
}

/// Sync a project's packs repository and stage one role out of it.
///
/// The whole wire-driven staging rule in one call: validate the source, put
/// the checkout on the exact commit, find the role's directory, and describe
/// what was staged in the shape the seat's 44223 carries.
pub fn stage_project_role_pack(
    packs_root: &Path,
    relay_http_base: &str,
    source: &ProjectPackSource,
    role: &str,
    auth: &GitAuthConfig,
) -> Result<StagedProjectPack, String> {
    if !is_role_slug(role) {
        return Err(format!("{role:?} is not a role slug"));
    }
    let (owner, id) = parse_repo_coordinate(&source.repo)?;
    let path = validate_pack_path(&source.path)?;
    let checkout = packs_checkout_dir(packs_root, &owner, &id);
    let clone_url = packs_clone_url(relay_http_base, &owner, &id);
    validate_clone_url(&clone_url)?;
    let sha = sync_packs_checkout(&checkout, &clone_url, source, auth)?;
    let (dir, persona) = role_pack_in_checkout(&checkout, &path, role).ok_or_else(|| {
        format!("the packs repository at {sha} holds no {path}/{role} pack for role {role}")
    })?;
    Ok(StagedProjectPack {
        pack_ref: PackRef {
            repo: source.repo.clone(),
            sha,
            role: role.to_string(),
            path: format!("{path}/{role}"),
        },
        dir,
        persona,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::project_git_exec::build_test_git_auth_config;

    /// A throwaway git repository under this worktree's scratch directory.
    ///
    /// Never a worktree of this repository: a test that runs `git` inside the
    /// checkout it was launched from will one day write to it.
    struct ScratchRepo {
        dir: PathBuf,
    }

    impl Drop for ScratchRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn scratch_root() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("l23b-scratch")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            ));
        std::fs::create_dir_all(&root).expect("scratch root");
        root
    }

    fn git(args: &[&str], cwd: &Path) -> String {
        let auth = build_test_git_auth_config().expect("git auth");
        run_git(args, Some(cwd), &auth).unwrap_or_else(|error| panic!("git {args:?}: {error}"))
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write");
    }

    /// Write one role pack — `.plugin/plugin.json` plus a persona declaring
    /// the role — under `<root>/personas/roles/<role>`.
    fn write_role_pack(root: &Path, role: &str, body: &str) {
        let pack = root.join(DEFAULT_PACK_PATH).join(role);
        write(
            &pack.join(".plugin/plugin.json"),
            &format!(
                r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
            ),
        );
        write(
            &pack.join(format!("personas/{role}.persona.md")),
            &format!(
                "---\nname: {role}\ndisplay_name: {role}\ndescription: The {role}.\nrole: {role}\n---\n{body}\n"
            ),
        );
    }

    /// A packs repository with two roles on `main`, plus a second commit that
    /// changes the builder and adds a runner.
    fn packs_repo(root: &Path) -> (ScratchRepo, String, String) {
        let dir = root.join("packs-origin");
        std::fs::create_dir_all(&dir).expect("origin dir");
        git(&["init", "--quiet", "--initial-branch", "main", "."], &dir);
        assert!(
            dir.join(".git").is_dir(),
            "the throwaway repository must own its own .git before anything is added"
        );
        git(&["config", "user.email", "l23b@test.invalid"], &dir);
        git(&["config", "user.name", "L23B"], &dir);
        write_role_pack(&dir, "builder", "You build. v1");
        write_role_pack(&dir, "architect", "You design. v1");
        git(&["add", "--all"], &dir);
        git(&["commit", "--quiet", "-m", "roles v1"], &dir);
        let first = git(&["rev-parse", "HEAD"], &dir).trim().to_string();
        write_role_pack(&dir, "builder", "You build. v2");
        write_role_pack(&dir, "runner", "You run. v2");
        git(&["add", "--all"], &dir);
        git(&["commit", "--quiet", "-m", "roles v2"], &dir);
        let second = git(&["rev-parse", "HEAD"], &dir).trim().to_string();
        (ScratchRepo { dir }, first, second)
    }

    fn persona_body(checkout: &Path, role: &str) -> String {
        std::fs::read_to_string(
            checkout
                .join(DEFAULT_PACK_PATH)
                .join(role)
                .join(format!("personas/{role}.persona.md")),
        )
        .expect("persona file")
    }

    fn source(repo: &str, git_ref: Option<&str>, sha: Option<&str>) -> ProjectPackSource {
        ProjectPackSource {
            repo: repo.to_string(),
            git_ref: git_ref.map(str::to_owned),
            sha: sha.map(str::to_owned),
            path: DEFAULT_PACK_PATH.to_string(),
        }
    }

    const REPO: &str =
        "30617:aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66:packs";

    #[test]
    fn a_ref_following_source_lands_on_the_branch_tip_and_records_its_sha() {
        let root = scratch_root();
        let (origin, _first, second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();

        let sha = sync_packs_checkout(
            &checkout,
            &url,
            &source(REPO, Some("refs/heads/main"), None),
            &auth,
        )
        .expect("sync");
        assert_eq!(sha, second, "a ref records the commit it resolved to");
        assert!(persona_body(&checkout, "builder").contains("You build. v2"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_pinned_sha_stages_that_commit_and_not_the_tip() {
        let root = scratch_root();
        let (origin, first, second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();

        let sha = sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth)
            .expect("sync");
        assert_eq!(sha, first);
        assert_ne!(first, second);
        assert!(
            persona_body(&checkout, "builder").contains("You build. v1"),
            "a pinned commit stages that commit's pack"
        );
        // A role added after the pin is not in the checkout — and no file from
        // a later checkout of the same cache is left behind either.
        assert!(!checkout.join("personas/roles/runner").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_second_sync_moves_an_existing_cache_between_commits() {
        let root = scratch_root();
        let (origin, first, second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();

        sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&second)), &auth)
            .expect("sync to tip");
        assert!(checkout.join("personas/roles/runner").is_dir());
        sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth)
            .expect("sync back");
        assert!(
            !checkout.join("personas/roles/runner").exists(),
            "a role the pinned commit does not have must not survive the move"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_commit_the_repository_does_not_have_refuses() {
        let root = scratch_root();
        let (origin, _first, _second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();
        let error = sync_packs_checkout(
            &checkout,
            &url,
            &source(REPO, None, Some(&"9".repeat(40))),
            &auth,
        )
        .expect_err("a commit that is not there is not a pack");
        assert!(error.contains("does not contain commit"), "{error}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_branch_the_repository_does_not_have_refuses() {
        let root = scratch_root();
        let (origin, _first, _second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();
        let error = sync_packs_checkout(
            &checkout,
            &url,
            &source(REPO, Some("refs/heads/nope"), None),
            &auth,
        )
        .expect_err("a branch that is not there is not a pack");
        assert!(error.contains("no branch nope"), "{error}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_role_the_repository_does_not_hold_is_not_a_pack() {
        let root = scratch_root();
        let (origin, first, _second) = packs_repo(&root);
        let auth = build_test_git_auth_config().expect("git auth");
        let checkout = root.join("cache/aa11bb22-packs");
        let url = origin.dir.to_string_lossy().to_string();
        sync_packs_checkout(&checkout, &url, &source(REPO, None, Some(&first)), &auth)
            .expect("sync");
        assert!(role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "builder").is_some());
        assert!(
            role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "runner").is_none(),
            "the role is not in this commit"
        );
        assert!(
            role_pack_in_checkout(&checkout, DEFAULT_PACK_PATH, "../../etc").is_none(),
            "a traversing role is not a role slug"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The addendum's last fallback, checked against the packs this build
    /// actually ships rather than against a fixture: every one of the seven
    /// role directories in `personas/roles` must resolve as that role's pack,
    /// or a fresh install hires an architect and gets a bare persona.
    #[test]
    fn every_shipped_role_pack_resolves_as_its_own_role() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the crate has a repository above it")
            .join(DEFAULT_PACK_PATH);
        assert!(
            shipped.is_dir(),
            "this build ships no {DEFAULT_PACK_PATH}: {}",
            shipped.display()
        );
        let mut found: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&shipped).expect("read shipped packs") {
            let entry = entry.expect("entry");
            if !entry.path().is_dir() {
                continue;
            }
            let role = entry.file_name().to_string_lossy().into_owned();
            assert!(
                role_pack_in_checkout(&shipped, "", &role).is_some(),
                "{role} is a directory in the shipped packs but not a pack declaring that role"
            );
            found.push(role);
        }
        found.sort();
        assert_eq!(
            found,
            vec![
                "architect",
                "builder",
                "designer",
                "lead",
                "poker",
                "runner",
                "verifier",
            ],
            "the seven roles the app ships"
        );
    }

    /// The middle fallback: a project that keeps its packs in the repository
    /// being worked on gets them without announcing anything.
    #[test]
    fn a_session_checkout_can_hold_the_role_pack_itself() {
        let root = scratch_root();
        let checkout = root.join("checkout");
        write_role_pack(&checkout, "builder", "You build, from the checkout.");
        assert_eq!(
            checkout_role_pack(&checkout, "builder"),
            Some((
                checkout.join(DEFAULT_PACK_PATH).join("builder"),
                "builder".to_owned()
            ))
        );
        // A role the checkout does not hold is not the checkout's answer.
        assert!(checkout_role_pack(&checkout, "runner").is_none());
        // And a directory that is not a pack is not one either.
        std::fs::create_dir_all(checkout.join(DEFAULT_PACK_PATH).join("runner"))
            .expect("empty role dir");
        assert!(checkout_role_pack(&checkout, "runner").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_source_names_exactly_one_of_a_ref_and_a_sha() {
        assert!(source(REPO, Some("refs/heads/main"), None).target().is_ok());
        assert!(source(REPO, None, Some(&"a".repeat(40))).target().is_ok());
        let both = source(REPO, Some("refs/heads/main"), Some(&"a".repeat(40)));
        assert!(both.target().unwrap_err().contains("exactly one"));
        assert!(source(REPO, None, None)
            .target()
            .unwrap_err()
            .contains("neither"));
        assert!(source(REPO, None, Some("nothex")).target().is_err());
        assert!(source(REPO, Some("refs/heads/../evil"), None)
            .target()
            .is_err());
    }

    #[test]
    fn a_repository_coordinate_is_validated_before_it_becomes_a_path() {
        let (owner, id) = parse_repo_coordinate(REPO).expect("coordinate");
        assert_eq!(id, "packs");
        assert_eq!(
            packs_checkout_dir(Path::new("/cache"), &owner, &id),
            PathBuf::from("/cache/aa11bb22-packs")
        );
        assert_eq!(
            packs_clone_url("https://hive.example/", &owner, &id),
            format!("https://hive.example/git/{owner}/packs")
        );
        for bad in [
            "30621:aa:packs",
            "30617:not-hex:packs",
            &format!("30617:{owner}:../escape"),
            &format!("30617:{owner}:"),
            &format!("30617:{owner}:-flag"),
        ] {
            assert!(
                parse_repo_coordinate(bad).is_err(),
                "{bad} must not become a path"
            );
        }
    }

    #[test]
    fn a_pack_path_may_only_name_a_place_inside_the_checkout() {
        assert_eq!(validate_pack_path("").expect("default"), DEFAULT_PACK_PATH);
        assert_eq!(
            validate_pack_path("/personas/roles/").expect("trimmed"),
            "personas/roles"
        );
        for bad in ["../etc", "personas/../../etc", "personas/-flag", "a//b"] {
            assert!(validate_pack_path(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn the_host_and_the_cli_name_the_same_cache_directory() {
        // `bee packs status` derives the directory with
        // `buzz_core::project_pack_source::pack_cache_dir_name` from the whole
        // coordinate; this host derives it from the two halves. If they ever
        // disagreed the CLI would report `cache_present: false` over a cache
        // the host had just filled — a wrong answer that looks like a fact.
        let owner = "a".repeat(64);
        for id in ["packs", "beekeeper-packs", &"z".repeat(64)] {
            let coordinate = format!("30617:{owner}:{id}");
            assert_eq!(
                Some(pack_cache_dir_name(&owner, id)),
                buzz_core_pkg::project_pack_source::pack_cache_dir_name(&coordinate),
                "{coordinate}"
            );
        }
    }
}

#[cfg(test)]
mod shipped_pack_ref_tests;
