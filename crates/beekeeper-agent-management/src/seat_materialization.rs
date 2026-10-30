//! Canonical role resolution and immutable pack composition shared by management clients.
//! Callers authenticate and pin the repository before supplying its checkout.

pub use beekeeper_persona::compose::{RoleSource, SourceProvenance};
pub use beekeeper_persona::team::AgentsRepoAccess;
pub use beekeeper_persona::template::TemplateCatalog;
use std::path::{Path, PathBuf};

/// Directory containing content-addressed composed role packs.
pub const STAGED_PACKS_DIR: &str = "staged";

/// Whether a role can be used as a single repository path segment.
pub fn is_role_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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
    let resolved = match beekeeper_persona::resolve::resolve_pack(pack_dir) {
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
/// into a named pack-unavailable refusal, because the project said its seats come
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
pub fn missing_role_pack_reason(sha: &str, path: &str, role: &str) -> String {
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
        .join(beekeeper_persona::compose::FLAT_ROLES_DIR)
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
    let path = if beekeeper_core::project_pack_source::is_root_pack_path(path) {
        ""
    } else {
        path
    };
    match source {
        RoleSource::Pack { role, .. } if path.is_empty() => role.clone(),
        RoleSource::Pack { role, .. } => format!("{path}/{role}"),
        RoleSource::Flat { role, .. } if path.is_empty() => {
            format!("{}/{role}", beekeeper_persona::compose::FLAT_ROLES_DIR)
        }
        RoleSource::Flat { role, .. } => {
            format!(
                "{path}/{}/{role}",
                beekeeper_persona::compose::FLAT_ROLES_DIR
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
    use beekeeper_persona::compose::{
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

#[cfg(test)]
#[path = "seat_materialization_tests.rs"]
mod tests;
