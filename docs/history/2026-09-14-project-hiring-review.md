# Project-agent hiring candidate review

Reviewed `42929e209` against base `2cce920db`, `docs/PROJECT_AGENT_HIRING_IMPL.md`, and the 2026-09-14 implementation report. The local selection path is correctly scoped: `decideCodingSessionHire` filters by exact `projectRef` and `homeRole`, and native `new_seat_refusal` repeats both checks for new selections. I found no path through the reviewed hire, picker, or native stage code that silently substitutes another project's agent or a different primary role.

## Findings

### 1. Major — association authority is enforced only in React, while native accepts any project coordinate

`desktop/src/features/project-agents/lib/publishedProjectAgents.ts:133-192` computes whether the current user is the creator, owner, or collaborator and disables the button. But the authoritative command at `desktop/src-tauri/src/commands/agent_project_association.rs:31-61` receives only `{pubkey, project_ref}`. It checks coordinate syntax and the local agent record, then persists the association and queues its 30177 publication. It never reads the project head/roster, proves the project exists, or checks that the active signer has project authority.

Trigger: call the exposed Tauri command directly (or reach it through any renderer bug/stale client state) as a viewer/nonmember, passing a valid `30621:<owner>:<slug>`. Native records the agent as belonging to that project. Hiring uses this local `project_ref` as its authoritative membership fact, so the same computer will thereafter accept the agent for a new seat in that project's session even though the associating identity was not authorized. Other readers may reject the wire claim, producing a split-brain roster, but that does not protect the local hire.

Fix before landing: make the native command prove the active identity is the project creator or a current owner/collaborator from signed project state, and fail closed when the project/roster cannot be read. The UI check remains useful presentation but cannot be the authority boundary.

### 2. Major — private-project fallback loses its unverified marker in compact output

When a private project is unreadable, `crates/buzz-cli/src/commands/project_agents.rs:357-382` accepts every matching-digest claim from the seat's NIP-OA attesting owner without verifying that owner has authority over the project. Full JSON distinguishes these rows with `owner_role: "attesting-owner"` (`:320-330`), but compact rendering omits `owner_role` (`:331-336`). The warning is only stderr (`:365-370`). A caller that captures stdout, uses `--format compact`, or suppresses stderr receives ordinary `{pubkey,name,role,owner}` rows indistinguishable from verified project agents.

Trigger: the owner that attested a seat publishes a 30177 for any agent with the target digest, while the seat cannot read that private project's roster; `bee --format compact projects agents` lists it as a project agent. This cannot force the host to seat a wrong local agent because local hire enforcement is sound, but it can pollute the lead's authoritative discovery input, cause futile hires, and misstate project membership. Preserve an explicit verification field in every output mode, or do not return fallback rows on stdout as project agents.

### 3. Major design/continuity gap — another host can erase the durable shared association

The published association is part of the content projection (`desktop/src-tauri/src/managed_agents/agent_events.rs:117-126`) at the replaceable address `(owner, agent pubkey)`. Boot reconcile republishes each local record whenever its projected content differs (`desktop/src-tauri/src/managed_agents/reconcile.rs:98-109,125-164`). Therefore a second computer holding the same agent with a legacy/unassociated local record publishes a newer event with no `project_digest`, superseding the associated host's event. The CLI and desktop deliberately honor the newest event and remove the agent from discovery.

Trigger: associate an agent on host A, then start host B where the same agent record has no `project_ref`. Host B's reconcile publishes the unassociated projection and the project roster loses that agent for every remote reader. A later boot/edit on either machine can flip the shared answer again. This contradicts the stated durable identity/second-computer discovery purpose and makes association depend on whichever host wrote last. The report calls it a minor known limit, but for project-owned agents it is a deterministic loss of shared membership, not an edge-only display issue. Give association its own project-scoped address or reconcile an authoritative association onto every host before allowing an unassociated publication to withdraw it.

### 4. Moderate — the digest does not provide the documented private-project secrecy

`crates/buzz-core/src/project_agent_association.rs:8-12` says someone who does not know the coordinate learns nothing beyond “some project.” The implementation is deterministic SHA-256 over a public domain separator plus `30621:<owner>:<dtag>` (`:23-51`). For common or otherwise guessable private project slugs, a relay member can enumerate candidate owner/slug coordinates offline, compare the result to public 30177 content, identify the project, and correlate agents sharing it. This is not a SHA-256 break; it is candidate checking against a low-entropy identifier.

Do not claim coordinate confidentiality from this digest. If hiding private project identity is required, use a project secret/salted opaque identifier unavailable to nonmembers; otherwise document that the digest hides plaintext only and permits guessing and correlation.

## Migration note

Existing unassociated agents are intentionally blocked from project hires. The shipped migration is reasonably safe for setup-created teams: boot/event sync calls the journal backfill, it matches both recorded pubkey and primary role, never overwrites another association, saves under the store lock, and queues republish. The Agents tab provides the explicit remedy for records outside setup journals. The material rollout risk is the native authority gap above: that remedy currently presents an owner/collaborator gate but does not enforce it below the UI.
