//! The one place the seat staging rule lives: which pack a seat runs with,
//! project source first, and how every rung is composed and staged.
//!
//! Split out of `actor_seats.rs` to keep both files under the repository's
//! 1000-line ceiling. `actor_seats` re-exports [`plan_seat_pack`], so the
//! preview command, the staging command and the restage path all still name
//! it there.

use std::path::Path;

use tauri::AppHandle;

use crate::app_state::AppState;
use crate::managed_agents::actor_seats::{
    installed_seat_pack_ref, resolve_local_seat_pack, SeatPackOrigin, SeatPackPreview,
};
use crate::managed_agents::packs_cache;
use crate::relay::relay_ws_url_with_override;

/// Resolve the seat pack for `agent_pubkey` at `role`, project source first.
///
/// The one place the staging rule lives, shared by the preview command, the
/// staging command, and [`crate::managed_agents::actor_seats_restage`] (a
/// provider restart re-resolves the pack rather than trusting a stale one)
/// so no caller can promise or restage a pack the create-time rule would not
/// pick.
#[allow(clippy::too_many_arguments)] // Each argument is a fact the rule reads; the restage path passes them all.
pub(crate) fn plan_seat_pack(
    app: &AppHandle,
    state: &AppState,
    records: &[crate::managed_agents::types::ManagedAgentRecord],
    record: &crate::managed_agents::types::ManagedAgentRecord,
    role: Option<&str>,
    pack_source: Option<packs_cache::ProjectPackSource>,
    checkout: Option<&Path>,
) -> SeatPackPreview {
    let role = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_owned);
    let catalog = packs_cache::template_catalog(app);
    if let Some(source) = pack_source {
        let Some(role) = role.clone() else {
            return refused_plan(
                None,
                packs_cache::HIRE_PACK_UNAVAILABLE,
                "this project stages packs by role, and the seat was given none".to_string(),
            );
        };
        let staged = packs_cache::packs_root(app).and_then(|root| {
            let auth = crate::commands::project_git_exec::build_git_auth_config(state)?;
            let relay_http = crate::relay::relay_http_base_url(&relay_ws_url_with_override(state));
            packs_cache::stage_project_role_pack(
                &root,
                &relay_http,
                &source,
                &role,
                &auth,
                &catalog,
            )
        });
        return match staged {
            Ok(mut pack) => {
                // The provider reads `actions.yml` from the same clone the seat
                // was staged from (spec § 4.11). Best-effort; the seat stages
                // either way.
                if let (true, Some(ref_name)) = (
                    buzz_core_pkg::project_pack_source::is_root_pack_path(&source.path),
                    source.git_ref.as_deref(),
                ) {
                    if let Ok((owner, id)) = packs_cache::parse_repo_coordinate(&source.repo) {
                        if let Ok(root) = packs_cache::packs_root(app) {
                            if let Err(error) =
                                crate::coding_sessions::workdir_store::record_agents_repo(
                                    app,
                                    state,
                                    &record.project_ref.clone().unwrap_or_default(),
                                    packs_cache::packs_checkout_dir(&root, &owner, &id),
                                    ref_name,
                                )
                            {
                                pack.warnings.push(format!(
                                    "this host could not record the agents repository for the provider: {error}"
                                ));
                            }
                        }
                    }
                }
                let source_kind = "repository".to_string();
                SeatPackPreview {
                    pack_staged: true,
                    origin: SeatPackOrigin::Project,
                    role: Some(role),
                    pack_dir: Some(pack.dir.to_string_lossy().into_owned()),
                    persona_id: Some(pack.persona),
                    pack_ref: Some(pack.pack_ref),
                    refusal: None,
                    reason: None,
                    warnings: pack.warnings,
                    compose_digest: Some(pack.digest),
                    source_kind: Some(source_kind),
                    agents_repo: pack.agents_repo,
                }
            }
            Err(reason) => refused_plan(Some(role), packs_cache::HIRE_PACK_UNAVAILABLE, reason),
        };
    }

    // No project record. Three fallbacks, in the order the addendum fixes:
    // the session's own checkout, then a pack installed on this computer, then
    // the packs this build ships. Each is a fact; none is a guess. Whatever
    // rung answers, the seat runs a *composed* copy staged under the packs
    // cache (spec § 4.5), never the rung's own directory.
    if let Some((role, checkout)) = role.as_deref().zip(checkout) {
        // Only the pack layout: since 2026-09-18 a code checkout holds no
        // flat team (spec § 4.11).
        let located =
            packs_cache::locate_role_source(checkout, packs_cache::DEFAULT_PACK_PATH, role)
                .map(|source| (source, packs_cache::DEFAULT_PACK_PATH));
        if let Some((source, path)) = located {
            let provenance =
                packs_cache::SourceProvenance::local(packs_cache::pack_ref_path(&source, path));
            return stage_local_plan(
                app,
                &catalog,
                role,
                SeatPackOrigin::Checkout,
                None,
                &source,
                &packs_cache::local_source_key(&checkout.join(path)),
                provenance,
            );
        }
    }
    let teams = crate::managed_agents::teams::load_teams(app).unwrap_or_default();
    match resolve_local_seat_pack(record, records, &teams, role.as_deref()).map_or_else(
        || {
            // The shipped packs, named on the wire as what they are: no
            // repository announces them, and the app's own version is what
            // pins them.
            let role = role.as_deref()?;
            let dir = packs_cache::shipped_packs_dir(app)?;
            let (dir, persona) = packs_cache::role_pack_in_checkout(&dir, "", role)?;
            Some((
                dir,
                persona,
                SeatPackOrigin::Shipped,
                Some(packs_cache::PackRef {
                    repo: packs_cache::PACK_REF_SHIPPED_REPO.to_string(),
                    sha: packs_cache::shipped_packs_version(app),
                    role: role.to_string(),
                    path: format!("{}/{role}", packs_cache::DEFAULT_PACK_PATH),
                }),
            ))
        },
        |(dir, persona)| {
            let (origin, pack_ref) = installed_seat_pack_ref(
                packs_cache::shipped_packs_dir(app).as_deref(),
                &packs_cache::shipped_packs_version(app),
                &dir,
                role.as_deref(),
            );
            Some((dir, persona, origin, pack_ref))
        },
    ) {
        Some((dir, persona, origin, pack_ref)) => {
            let Some(role) = role.as_deref() else {
                // An unseated-by-role create keeps the behaviour it has always
                // had: the actor's own pack, read where it is. There is no
                // role slug to compose under, and no role instructions to
                // compose.
                return SeatPackPreview {
                    pack_staged: true,
                    origin,
                    role: None,
                    pack_dir: Some(dir.to_string_lossy().into_owned()),
                    persona_id: Some(persona),
                    pack_ref,
                    refusal: None,
                    reason: None,
                    warnings: Vec::new(),
                    compose_digest: None,
                    source_kind: Some("local".to_string()),
                    agents_repo: packs_cache::AgentsRepoAccess::None,
                };
            };
            let (key, provenance) = match (&origin, &pack_ref) {
                (SeatPackOrigin::Shipped, Some(pack_ref)) => (
                    format!("app-{}", pack_ref.sha),
                    packs_cache::SourceProvenance {
                        kind: "shipped".to_string(),
                        repo: Some(pack_ref.repo.clone()),
                        sha: Some(pack_ref.sha.clone()),
                        path: pack_ref.path.clone(),
                    },
                ),
                _ => (
                    packs_cache::local_source_key(&dir),
                    packs_cache::SourceProvenance::local(dir.to_string_lossy().into_owned()),
                ),
            };
            let source = packs_cache::RoleSource::Pack {
                dir,
                role: role.to_string(),
                persona: Some(persona),
            };
            stage_local_plan(
                app, &catalog, role, origin, pack_ref, &source, &key, provenance,
            )
        }
        None => SeatPackPreview {
            pack_staged: false,
            origin: SeatPackOrigin::None,
            role,
            pack_dir: None,
            persona_id: None,
            pack_ref: None,
            refusal: None,
            reason: None,
            warnings: Vec::new(),
            compose_digest: None,
            source_kind: None,
            agents_repo: packs_cache::AgentsRepoAccess::None,
        },
    }
}

/// A plan that refuses: nothing staged, the sentence and its reason set.
fn refused_plan(role: Option<String>, refusal: &str, reason: String) -> SeatPackPreview {
    SeatPackPreview {
        pack_staged: false,
        origin: SeatPackOrigin::None,
        role,
        pack_dir: None,
        persona_id: None,
        pack_ref: None,
        refusal: Some(refusal.to_string()),
        reason: Some(reason),
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        agents_repo: packs_cache::AgentsRepoAccess::None,
    }
}

/// Compose and stage a local rung's source, and describe the result.
///
/// A rung that found a pack it cannot compose refuses with
/// [`packs_cache::SEAT_PACK_UNCOMPOSABLE`] rather than seating the agent on
/// the uncomposed directory: the include the composer could not resolve is
/// part of the role's instructions, and a seat without it is not the role.
#[allow(clippy::too_many_arguments)] // One call site per rung; the facts are the facts.
fn stage_local_plan(
    app: &AppHandle,
    catalog: &packs_cache::TemplateCatalog,
    role: &str,
    origin: SeatPackOrigin,
    pack_ref: Option<packs_cache::PackRef>,
    source: &packs_cache::RoleSource,
    source_key: &str,
    provenance: packs_cache::SourceProvenance,
) -> SeatPackPreview {
    let provenance_kind = provenance.kind.clone();
    let staged = packs_cache::packs_root(app).and_then(|root| {
        packs_cache::stage_composed_pack(&root, source_key, source, catalog, provenance)
    });
    match staged {
        Ok(staged) => SeatPackPreview {
            pack_staged: true,
            origin,
            role: Some(role.to_string()),
            pack_dir: Some(staged.dir.to_string_lossy().into_owned()),
            persona_id: Some(staged.persona),
            pack_ref,
            refusal: None,
            reason: None,
            warnings: staged.warnings,
            compose_digest: Some(staged.digest),
            source_kind: Some(provenance_kind.to_string()),
            agents_repo: staged.agents_repo,
        },
        Err(reason) => refused_plan(
            Some(role.to_string()),
            packs_cache::SEAT_PACK_UNCOMPOSABLE,
            reason,
        ),
    }
}
