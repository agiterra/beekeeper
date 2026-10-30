use super::*;
use crate::management::local::{actor_custody, management_lock, save, stage_actor_with_role};
use std::path::Path;

fn text(path: &Path) -> Result<&str, Error> {
    path.to_str()
        .ok_or_else(|| fail("materialization path is not UTF-8"))
}
impl<T: RelayTransport> LocalHost<'_, T> {
    pub(super) async fn stage_seat(&self, scope: &Scope, plan: &SeatPlan) -> Result<(), Error> {
        self.revalidate(scope, plan).await?;
        let (snapshot, material) = self.material(scope).await?;
        let config = host(self.context, self.client)?;
        let p = decode_coding_session_lifecycle_command(&plan.create.content).map_err(fail)?;
        let Action::SessionCreate {
            actor: Some(actor),
            session_ref: Some(session_ref),
            ..
        } = p.action
        else {
            return Err(fail("not a seated create"));
        };
        if actor != self.actor {
            return Err(Error::Auth(
                "saved create selected a different actor".into(),
            ));
        }
        let branch = format!("beekeeper-hire/{}", p.command_id);
        let preparation =
            beekeeper_host_core::layout::host_dir(&self.context.home, self.context.instance)
                .join("management/hire-preparation");
        beekeeper_host_core::atomic_write::create_dir_all_restricted(&preparation).map_err(fail)?;
        let freshly_cut = crate::hire_preparation::prepare(
            &preparation.join(format!("{}-code.json", plan.create.id)),
            json!({"create":plan.create.id,"checkout":plan.checkout,"destination":plan.worktree,"revision":plan.checkout_revision,"branch":branch}),
            &plan.worktree,
            &plan.checkout,
            &[
                "worktree",
                "add",
                "--no-checkout",
                "-b",
                &branch,
                "--",
                text(&plan.worktree)?,
                &plan.checkout_revision,
            ],
            &["checkout", "--force", &branch],
            &plan.checkout_revision,
            |root, args| git(self.context, root, args),
            |run| {
                if run(&plan.worktree, &["rev-parse", "HEAD"])? != plan.checkout_revision
                    || run(&plan.worktree, &["rev-parse", "--abbrev-ref", "HEAD"])? != branch
                    || run(
                        &plan.worktree,
                        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    )? != run(
                        &plan.checkout,
                        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    )?
                {
                    return Err(Error::Conflict(
                        "reserved seat is not the originally cut worktree".into(),
                    ));
                }
                Ok(())
            },
        )?;
        if let Some(destination) = &plan.agents_checkout {
            let source =
                beekeeper_core::project_pack_source::decode_project_pack_source(&material.source)
                    .map_err(fail)?;
            let write_branch = if material.agents_access == "write" {
                let beekeeper_core::project_pack_source::PackPin::Ref(reference) = source.pin()
                else {
                    return Err(Error::Refused(
                        "a write-enabled agents role requires a source branch".into(),
                    ));
                };
                Some(
                    reference
                        .strip_prefix("refs/heads/")
                        .ok_or_else(|| fail("agents source branch malformed"))?
                        .to_owned(),
                )
            } else {
                None
            };
            let checkout = match write_branch.as_deref() {
                Some(branch) => vec!["checkout", "--force", "-B", branch, &material.pack_ref.sha],
                None => vec!["checkout", "--force", "--detach", &material.pack_ref.sha],
            };
            crate::hire_preparation::prepare(
                &preparation.join(format!("{}-agents.json", plan.create.id)),
                json!({"create":plan.create.id,"source":material.source_checkout,"destination":destination,"revision":material.pack_ref.sha,"branch":write_branch}),
                destination,
                &material.source_checkout,
                &[
                    "clone",
                    "--no-hardlinks",
                    "--no-checkout",
                    "--",
                    text(&material.source_checkout)?,
                    text(destination)?,
                ],
                &checkout,
                &material.pack_ref.sha,
                |root, args| git(self.context, root, args),
                |run| {
                    // Before the first checkout, clone HEAD may legitimately name
                    // another source branch. Prove the pinned commit is available.
                    if run(
                        destination,
                        &[
                            "rev-parse",
                            &format!("{}^{{commit}}", material.pack_ref.sha),
                        ],
                    )? != material.pack_ref.sha
                    {
                        return Err(Error::Conflict(
                            "agents clone lacks pinned source commit".into(),
                        ));
                    }
                    Ok(())
                },
            )?;
            if let Some(branch) = write_branch.as_deref() {
                if git(
                    self.context,
                    destination,
                    &["rev-parse", "--abbrev-ref", "HEAD"],
                )? != branch
                {
                    return Err(Error::Conflict(
                        "completed agents preparation changed its branch; refusing reset".into(),
                    ));
                }
            }
            // The clone's upstream is the authorized relay repository, never
            // a management-private source-cache directory.
            let repo = source.repo().splitn(3, ':').collect::<Vec<_>>();
            let url = format!(
                "{}/git/{}/{}",
                self.client.relay_url().trim_end_matches('/'),
                repo[1],
                repo[2]
            );
            git(
                self.context,
                destination,
                &["remote", "set-url", "origin", "--", &url],
            )?;
        }
        let seeding = if freshly_cut {
            let boundary = crate::hire_source::git_boundary(
                self.context,
                &plan.worktree,
                Some(&plan.checkout),
            )?;
            let pool = beekeeper_session_provider::execution_scope::project_scope_pool_dir(
                &config.provider_state_dir,
                Some(&scope.project),
                &plan.worktree,
            );
            match beekeeper_session_provider::sandbox_seed_host::seed_tree(
                &boundary,
                &plan.checkout,
                &plan.worktree,
                Some(&pool),
            ) {
                Ok(Some(receipt)) => Some(serde_json::to_value(receipt).map_err(fail)?),
                Ok(None) => None,
                Err(refusal) => Some(json!({"declares":true,"complete":false,"refusal":refusal})),
            }
        } else if plan.worktree.join("sandbox.yml").exists() {
            Some(
                json!({"declares":true,"complete":false,"detail":"existing tree; seeding outcome not re-executed on recovery"}),
            )
        } else {
            None
        };
        let definition: ManagedAgentEventContent =
            serde_json::from_str(&snapshot.definition.content).map_err(fail)?;
        // This lock is shared with ordinary management, held only for the
        // canonical local custody writes, never through a receipt/model wait.
        let _lock = management_lock(self.context)?;
        let credentials = actor_custody(&config.provider_state_dir, &actor, None)?;
        if beekeeper_sdk::nip_oa::verify_auth_tag(
            &credentials.1,
            &nostr::PublicKey::from_hex(&actor).map_err(fail)?,
        )
        .map_err(fail)?
        .to_hex()
            != owner(self.client)?
        {
            return Err(Error::Auth("actor custody owner mismatch".into()));
        }
        let mut role = json!({"packDir":material.pack_dir,"personaId":material.pack_ref.role,"packRef":material.pack_ref});
        if let Some(path) = &plan.agents_checkout {
            role["agentsCheckout"] = json!({"path":path,"access":material.agents_access});
        }
        stage_actor_with_role(
            &config,
            &p.command_id,
            &actor,
            &definition.name,
            (&credentials.0, &credentials.1),
            &plan.worktree,
            Some(&scope.project),
            Some(role),
        )?;
        let path = config
            .provider_state_dir
            .join("coding-session-workdirs.json");
        let mut records: serde_json::Value = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).map_err(fail)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                json!({"version":2,"worktrees":{}})
            }
            Err(e) => return Err(fail(e)),
        };
        let key = format!("{session_ref}/{}", plan.pack_ref.role);
        let entry = json!({"path":plan.worktree,"branch":branch,"repoRoot":plan.checkout,"createdAt":chrono::Utc::now().to_rfc3339(),"actorPubkey":actor,"agentsClone":plan.agents_checkout,"seeding":seeding});
        let worktrees = records["worktrees"]
            .as_object_mut()
            .ok_or_else(|| fail("worktree store is malformed"))?;
        // Multiple completed responsibilities for one role remain nameable.
        let key = if worktrees
            .get(&key)
            .is_some_and(|v| v["path"] != entry["path"])
        {
            format!("{session_ref}/{}-{}", plan.pack_ref.role, p.command_id)
        } else {
            key
        };
        worktrees.entry(key).or_insert(entry);
        save(&path, &records)
    }
}
