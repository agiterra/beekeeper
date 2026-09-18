//! The packs a project's seats are staged from, fetched from a git repository.
//!
//! # Why a repository
//!
//! Role packs are small trees of text, and until
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

/// The composer's source and provenance types, re-exported so the planner,
/// the Roles view and this module name one vocabulary.
pub use buzz_persona_pkg::compose::{RoleSource, SourceProvenance};
pub use buzz_persona_pkg::team::AgentsRepoAccess;
pub use buzz_persona_pkg::template::TemplateCatalog;

/// Where this build ships its role templates, relative to the resource root
/// and to a development checkout: `personas/templates`.
pub const DEFAULT_TEMPLATES_PATH: &str = "personas/templates";

/// The directory under the packs cache holding composed, staged packs:
/// `<packs root>/staged/<source key>/<digest>/`.
pub const STAGED_PACKS_DIR: &str = "staged";

/// The refusal a seat carries when this computer found a pack for its role
/// but could not compose it — a broken pack, an include it cannot resolve,
/// a template this build does not ship. The seat is **not** started on a
/// bare persona; the reason travels beside this sentence.
pub const SEAT_PACK_UNCOMPOSABLE: &str =
    "This computer found a pack for that role but could not compose it, so the seat was not \
     started — an agent seated without its role instructions is not the agent asked for.";

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
    /// The composed, staged pack directory under the packs cache — what the
    /// seat's `packDir` points at. Never a directory inside the shared
    /// checkout, so a later sync cannot change a running seat's instructions
    /// (spec § 4.5).
    pub dir: PathBuf,
    /// The persona inside [`Self::dir`] that declares the role — the seat's
    /// `personaId`.
    pub persona: String,
    /// The wire's account of it.
    pub pack_ref: PackRef,
    /// The composition's content digest (`sha256:…`), from its `compose.json`.
    pub digest: String,
    /// What the composer wanted said: a deprecated template, a mid-line
    /// `![[`. Never a refusal — those are `Err`.
    pub warnings: Vec<String>,
    /// `team.yml` `workspace.agents_repo` for this role (spec § 4.11): whether
    /// a seat gets the agents repository beside its worktree, and how.
    pub agents_repo: AgentsRepoAccess,
}

/// A composed pack staged under the packs cache from any rung.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedComposedPack {
    /// `<packs root>/staged/<source key>/<digest12>/`.
    pub dir: PathBuf,
    /// The persona inside it: always the role slug.
    pub persona: String,
    /// `sha256:…` over the staged bytes.
    pub digest: String,
    /// The composer's warnings, verbatim.
    pub warnings: Vec<String>,
    /// `team.yml` `workspace.agents_repo` for this role; `None` without a
    /// manifest or for a pack source.
    pub agents_repo: AgentsRepoAccess,
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
    // The repository root: the agents repository's layout (spec § 4.11).
    if path == buzz_core_pkg::project_pack_source::PACK_PATH_ROOT {
        return Ok(path.to_string());
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

/// The role packs this build ships, or `None` when this build has none.
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
/// folder of role packs the operator chose — on a development machine, the
/// checkout's own `personas/roles`. `plan_seat_pack` reaches the installed arm
/// first, and before L33 that arm hard-coded `packRef: null`, so a seat staged
/// from the shipped defaults said on the wire that nothing vouched for its
/// pack (finding 53, run 5).
///
/// L33 recognised the shipped pack **by path**: `pack_dir` canonicalises to
/// `<shipped>/<role>`. That was measured true only at the helper. On the
/// running dev app it is false: `tauri-build` copies the bundle resources
/// into the desktop crate's own target directory
/// (`desktop/src-tauri/target/debug/personas/roles`), [`shipped_packs_dir`]
/// answers with that copy, and the installed directory is the checkout the
/// copy was made from — the same bytes at another path. So every installed
/// role agent on a dev build, the lead included, kept publishing no `packRef`
/// (finding 72, runs 6 and 7, both machines).
///
/// This is therefore a recognition on two facts and a guess on none: the
/// answer is `Some` when `pack_dir` **is** `<shipped>/<role>` (canonical
/// paths, so a symlinked or `..`-laden route is the same directory), or when
/// it holds **byte-for-byte the same files** as `<shipped>/<role>`
/// ([`pack_bytes::same_pack_bytes`]). A copy the operator has edited since the
/// build differs in bytes and keeps `None`, because no version of this app can
/// vouch for what is in it.
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
    if canonical(&expected) != canonical(pack_dir)
        && !pack_bytes::same_pack_bytes(&expected, pack_dir)
    {
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

/// The role templates this build ships, or `None` when it ships none —
/// resolved exactly as [`shipped_packs_dir`] resolves the packs: the bundle
/// resource first, the development checkout second, a guess never.
pub fn shipped_templates_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    if let Ok(resource) = app
        .path()
        .resolve(DEFAULT_TEMPLATES_PATH, tauri::path::BaseDirectory::Resource)
    {
        if resource.is_dir() {
            return Some(resource);
        }
    }
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join(DEFAULT_TEMPLATES_PATH);
    checkout.is_dir().then_some(checkout)
}

/// The template catalog every composition on this host resolves against:
/// this build's shipped templates, identified by the app version.
///
/// A build with no templates directory, or one whose catalog does not load,
/// yields an **empty** catalog rather than an error: a role with no
/// `![[beekeeper/…]]` include composes fine without one, and a role with one
/// is refused at that include, naming the template it could not find. The
/// load failure is logged so it is not silent.
pub fn template_catalog(app: &tauri::AppHandle) -> TemplateCatalog {
    let version = shipped_packs_version(app);
    let Some(dir) = shipped_templates_dir(app) else {
        return TemplateCatalog::empty(&version);
    };
    match TemplateCatalog::load(&dir, &version) {
        Ok(catalog) => catalog,
        Err(error) => {
            tracing::warn!(
                templates = %dir.display(),
                %error,
                "the shipped template catalog could not be loaded; composing with none"
            );
            TemplateCatalog::empty(&version)
        }
    }
}

/// The role directory inside a *session checkout*, when it holds one.
///
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

/// The reason a hire is refused when a synced packs repository holds no pack
/// for `role`: one sentence, written here so the staging path and the Roles
/// view cannot spell it two ways.
pub(crate) fn missing_role_pack_reason(sha: &str, path: &str, role: &str) -> String {
    format!(
        "the packs repository at {sha} holds no {path}/{role} pack and no {path}/roles/{role}.md for role {role}"
    )
}

/// Find the source of `role` under `<checkout>/<path>`, in the two layouts
/// the composer reads (spec § 4.8): a pack directory
/// `<path>/<role>/.plugin/plugin.json` whose persona declares the role
/// first, then a flat file `<path>/roles/<role>.md`. `None` when neither is
/// there, which the caller turns into the same refusal it always did.
///
/// A pack directory whose persona declares some other role is not this role
/// ([`role_persona_in_pack`]), and a flat file that is not a file is nothing.
pub fn locate_role_source(checkout: &Path, path: &str, role: &str) -> Option<RoleSource> {
    if !is_role_slug(role) {
        return None;
    }
    let mut root = checkout.to_path_buf();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        root.push(segment);
    }
    let pack_dir = root.join(role);
    if let Some(persona) = role_persona_in_pack(&pack_dir, role) {
        return Some(RoleSource::Pack {
            dir: pack_dir,
            role: role.to_string(),
            persona: Some(persona),
        });
    }
    let flat = root
        .join(buzz_persona_pkg::compose::FLAT_ROLES_DIR)
        .join(format!("{role}.md"));
    if flat.is_file() {
        return Some(RoleSource::Flat {
            root,
            role: role.to_string(),
        });
    }
    None
}

/// The repository-relative `packRef.path` for a role found at `path`: the
/// pack directory for a pack source, `<path>/roles/<role>` for a flat one.
/// Both end in `/<role>`, which the closed 44223 validator requires; the
/// flat form's `.md` is implied (spec § 4.6).
pub fn pack_ref_path(source: &RoleSource, path: &str) -> String {
    let path = path.trim_matches('/');
    let path = if buzz_core_pkg::project_pack_source::is_root_pack_path(path) {
        ""
    } else {
        path
    };
    match source {
        RoleSource::Pack { role, .. } if path.is_empty() => role.clone(),
        RoleSource::Pack { role, .. } => format!("{path}/{role}"),
        RoleSource::Flat { role, .. } if path.is_empty() => {
            format!("{}/{role}", buzz_persona_pkg::compose::FLAT_ROLES_DIR)
        }
        RoleSource::Flat { role, .. } => {
            format!(
                "{path}/{}/{role}",
                buzz_persona_pkg::compose::FLAT_ROLES_DIR
            )
        }
    }
}

/// Where composed packs are staged: `<packs root>/staged`.
pub fn staged_packs_root(packs_root: &Path) -> PathBuf {
    packs_root.join(STAGED_PACKS_DIR)
}

/// A source key for a directory nobody on the wire can name: twelve hex of
/// the SHA-256 of its path, prefixed `local-`. Two hosts never share a
/// staged directory, so the key only has to be stable on this one.
pub fn local_source_key(dir: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(dir.to_string_lossy().as_bytes());
    format!("local-{}", &hex::encode(digest)[..12])
}

/// Compose `source` against `catalog` and stage the result as an ordinary
/// pack under `<packs root>/staged/<source_key>/<digest12>/`.
///
/// The directory is keyed by the composition's content digest, so it is
/// immutable for as long as any seat points at it: composing the same
/// inputs again finds the same directory and writes nothing; composing
/// changed inputs writes a sibling. A running seat's `packDir` therefore
/// never changes underneath it — the hazard the shared checkout had
/// (`sync_packs_checkout` runs `git checkout --force` on it).
///
/// # Errors
/// The composer's refusal, verbatim — a missing role, a cycle, a template
/// this build does not ship, a skill provided twice — or a filesystem error
/// under the staging root.
pub fn stage_composed_pack(
    packs_root: &Path,
    source_key: &str,
    source: &RoleSource,
    catalog: &TemplateCatalog,
    provenance: SourceProvenance,
) -> Result<StagedComposedPack, String> {
    use buzz_persona_pkg::compose::{
        compose_role, write_staged_pack, ComposeOptions, COMPOSE_JSON,
    };
    let options = ComposeOptions {
        pack_id: None,
        pack_version: None,
        source: provenance,
    };
    let composed = compose_role(source, catalog, &options).map_err(|error| error.to_string())?;
    let digest_hex = composed
        .provenance
        .digest
        .strip_prefix("sha256:")
        .unwrap_or(&composed.provenance.digest);
    let short: String = digest_hex.chars().take(12).collect();
    let dir = staged_packs_root(packs_root).join(source_key).join(short);
    let already = std::fs::read_to_string(dir.join(COMPOSE_JSON))
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value.get("digest")?.as_str().map(str::to_owned))
        .is_some_and(|digest| digest == composed.provenance.digest);
    if !already {
        write_staged_pack(&composed, &dir).map_err(|error| {
            format!(
                "failed to stage the composed pack at {}: {error}",
                dir.display()
            )
        })?;
    }
    Ok(StagedComposedPack {
        dir,
        persona: composed.role().to_string(),
        digest: composed.provenance.digest,
        warnings: composed.provenance.warnings,
        agents_repo: composed.agents_repo,
    })
}

/// Sync a project's packs repository and stage one role out of it.
///
/// The whole wire-driven staging rule in one call: validate the source, put
/// the checkout on the exact commit, find the role's source in it (pack or
/// flat, [`locate_role_source`]), compose it against `catalog`, stage the
/// result under the packs cache, and describe what was staged in the shape
/// the seat's 44223 carries.
pub fn stage_project_role_pack(
    packs_root: &Path,
    relay_http_base: &str,
    source: &ProjectPackSource,
    role: &str,
    auth: &GitAuthConfig,
    catalog: &TemplateCatalog,
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
    let role_source = locate_role_source(&checkout, &path, role)
        .ok_or_else(|| missing_role_pack_reason(&sha, &path, role))?;
    let ref_path = pack_ref_path(&role_source, &path);
    let staged = stage_composed_pack(
        packs_root,
        &format!("{}-{sha}", pack_cache_dir_name(&owner, &id)),
        &role_source,
        catalog,
        SourceProvenance {
            kind: "repository".to_string(),
            repo: Some(source.repo.clone()),
            sha: Some(sha.clone()),
            path: ref_path.clone(),
        },
    )?;
    Ok(StagedProjectPack {
        pack_ref: PackRef {
            repo: source.repo.clone(),
            sha,
            role: role.to_string(),
            path: ref_path,
        },
        dir: staged.dir,
        persona: staged.persona,
        digest: staged.digest,
        warnings: staged.warnings,
        agents_repo: staged.agents_repo,
    })
}

#[cfg(test)]
mod tests;

mod definition_drift;
mod pack_bytes;
pub use definition_drift::{definition_drift, DefinitionDrift, DefinitionDriftState};

#[cfg(test)]
mod shipped_pack_ref_tests;
