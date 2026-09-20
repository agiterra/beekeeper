/**
 * Host-local custody of an agent seat's key material, from the webview side.
 *
 * The secret half of a seat's identity never travels on the relay: it is
 * written into the provider's owner-only pending-seat file, keyed by the
 * create's exact `commandId`, and consumed by the provider when it spawns the
 * seat. These two calls are the whole webview surface of that channel — the
 * key itself is read from the OS keyring in Rust and never crosses into
 * JavaScript.
 *
 * Mirrors `desktop/src-tauri/src/managed_agents/actor_seats.rs`.
 */
import { invokeTauri } from "@/shared/api/tauri";

/**
 * The wire's account of the pack a seat was staged with: kind:44223 `packRef`.
 *
 * Present only when the pack came from a project's packs repository. A pack
 * installed on this computer has no repository that can vouch for it, so the
 * screen has nothing to name and must not invent one.
 */
export type CodingSessionSeatPackRef = {
  /** Repository announcement coordinate, `30617:<owner-hex>:<id>`. */
  repo: string;
  /** The exact commit staged, lowercase 40-hex. */
  sha: string;
  /** The role whose pack was staged. */
  role: string;
  /** Path of the staged role directory inside the repository. */
  path: string;
};

/**
 * A project's kind:30624 pack source, as this computer is asked to stage from.
 *
 * The renderer reads the signed record off the relay and passes its decoded
 * tags; every field is validated in Rust before it can reach `git`, and the
 * checkout is confined to the host's own packs cache.
 */
export type CodingSessionProjectPackSource = {
  /** `30617:<owner-hex>:<id>` — the packs repository announcement. */
  repo: string;
  /** `refs/heads/main`, when the source follows a branch. */
  gitRef?: string | null;
  /** A pinned commit, when the source pins one. Exactly one of the two. */
  sha?: string | null;
  /** Sub-path holding the role directories; `personas/roles` when absent. */
  path?: string | null;
};

/** What one staging call actually put on disk for the provider to consume. */
export type StagedCodingSessionActorSeat = {
  /**
   * Whether the seat was staged with a role pack. `false` means the seat runs
   * on its prompt alone — no `.agents/skills` will exist in its working
   * directory — and the screen must say so rather than imply craft it lacks.
   */
  packStaged: boolean;
  /**
   * The pack's `packRef`, when it came from the project's packs repository.
   * `null` for a pack installed on this computer and for a packless seat.
   */
  packRef: CodingSessionSeatPackRef | null;
};

/**
 * Where a seat's pack would come from, in the order the host tries them:
 * the project's packs repository, the session's own checkout, a pack
 * installed on this computer, then the packs this build ships.
 */
export type CodingSessionSeatPackOrigin =
  | "project"
  | "checkout"
  | "installed"
  | "shipped"
  | "none";

/** What this computer would stage for a seat, without staging it. */
export type CodingSessionSeatPackPreview = {
  /** Whether a pack would be staged at all. */
  packStaged: boolean;
  /** Where it would come from. */
  origin: CodingSessionSeatPackOrigin;
  /** The role the preview was computed for, or null when none was named. */
  role: string | null;
  /** Absolute directory that would be staged, or null. */
  packDir: string | null;
  /** The persona inside it, or null. */
  personaId: string | null;
  /** The wire's `packRef`, or null when no repository can vouch for it. */
  packRef: CodingSessionSeatPackRef | null;
  /**
   * The sentence a hire would be refused with, or null when it would go
   * ahead. Present exactly when the project names a packs source this
   * computer could not stage the seat's role out of.
   */
  refusal: string | null;
  /** The underlying reason behind `refusal`, or null. */
  reason: string | null;
  /**
   * `team.yml` `workspace.roles_visible` for this role (spec § 4.10): a
   * seat in this role keeps `beekeeper/` in its worktree. Absent from a host
   * too old to say, which a caller reads as `false` — hidden.
   */
  /** `team.yml` `workspace.agents_repo` for the seat's role (spec § 4.11). */
  agentsRepo?: "none" | "read" | "write";
};

/**
 * Stage a managed agent's identity for one exact coding-session create.
 *
 * Call this BEFORE the 44221 is published: the provider refuses a create
 * naming an actor with no staged seat (`ACTOR_UNAVAILABLE`). Rejects when the
 * agent is unknown to this computer or its key is unavailable (a keyring
 * outage), so a create the provider could never honour is never signed.
 *
 * The seat's role pack is resolved on the Rust side from the agent's own
 * provenance — this call names an agent and never a path, so nothing here can
 * choose the directory the provider materializes skills from.
 */
export async function stageCodingSessionActorSeat(input: {
  commandId: string;
  agentPubkey: string;
  /**
   * The role this seat is being created with. **The seat's role picks the
   * pack** — the actor's home role is never consulted — so a create that omits
   * it gets the actor's own pack, which is only right for a seat at its own
   * role.
   */
  role?: string | null;
  /** The project's 30624 pack source, when it has one. */
  packSource?: CodingSessionProjectPackSource | null;
  /**
   * The session's working directory. Read only — the host looks for
   * `<checkout>/personas/roles/<role>` and never writes there, clones there,
   * or uses it as a git remote.
   */
  checkout?: string | null;
  /**
   * When set, the host refuses (`SEAT_NOT_PROJECT_AGENT`) unless the agent's
   * recorded `project_ref` equals it. New selections pass the project they
   * create into; a resume of an existing execution passes nothing.
   */
  requireProjectRef?: string | null;
  /**
   * A new selection: the host also requires the role to be the agent's
   * primary role, and a projectless create to take an agent of no project.
   */
  newSelection?: boolean;
  /**
   * The seat's own worktree, when the create runs in one. The host reads
   * its `HEAD` and, when that branch has a committed change to what this
   * role's composition reads, seats the agent on the branch's definition
   * instead of `main`'s (spec § 4.9). Read only; never written to.
   */
  worktree?: string | null;
  /**
   * The provider session id of the execution being staged, when there is one.
   *
   * A re-stage (reconnect, restart, a generation after a relaunch) knows the
   * execution but not the directory the create cut for it. The host resolves
   * the seat's worktree from its own record by this id, so a role granted the
   * agents repository can be staged again without the renderer having to
   * carry — or invent — a path (ledger 187).
   */
  sessionId?: string | null;
}): Promise<StagedCodingSessionActorSeat> {
  const staged = await invokeTauri<StagedCodingSessionActorSeat | null>(
    "stage_coding_session_actor_seat",
    {
      commandId: input.commandId,
      agentPubkey: input.agentPubkey,
      role: input.role ?? null,
      packSource: input.packSource ?? null,
      checkout: input.checkout ?? null,
      requireProjectRef: input.requireProjectRef ?? null,
      newSelection: input.newSelection === true,
      worktree: input.worktree ?? null,
      sessionId: input.sessionId ?? null,
    },
  );
  return {
    packStaged: staged?.packStaged === true,
    packRef: staged?.packRef ?? null,
  };
}

/**
 * What this computer would stage for `agentPubkey` at `role`, without staging.
 *
 * Read-only: it syncs the project's packs cache so the answer is the answer,
 * but writes no seat and publishes nothing. The hire and launch dialogs call
 * it to show the pack a seat will run with — and, when the project's packs
 * cannot be read, the sentence the hire will be refused with — *before* the
 * operator commits to it.
 */
export async function previewCodingSessionSeatPack(input: {
  agentPubkey: string;
  role?: string | null;
  packSource?: CodingSessionProjectPackSource | null;
  checkout?: string | null;
  /** Same check as {@link stageCodingSessionActorSeat}'s `requireProjectRef`. */
  requireProjectRef?: string | null;
  /** Same as {@link stageCodingSessionActorSeat}'s `newSelection`. */
  newSelection?: boolean;
}): Promise<CodingSessionSeatPackPreview | null> {
  const preview = await invokeTauri<CodingSessionSeatPackPreview | null>(
    "preview_coding_session_seat_pack",
    {
      agentPubkey: input.agentPubkey,
      role: input.role ?? null,
      packSource: input.packSource ?? null,
      checkout: input.checkout ?? null,
      requireProjectRef: input.requireProjectRef ?? null,
      newSelection: input.newSelection === true,
    },
  );
  return preview ?? null;
}

/** What `project_packs_init` actually produced. */
export type ProjectPacksInit = {
  /** The repository coordinate, `30617:<viewer-hex>:<id>`. */
  repoRef: string;
  /**
   * Event id of the kind:30624 pack source, or null when it was withheld —
   * which happens exactly when the push did not land, because a source
   * pointing at an empty repository would refuse every later hire.
   */
  sourceEventId: string | null;
  /** The seed commit, lowercase 40-hex. */
  seedCommitSha: string;
  /**
   * Event id of the relay-signed kind:30618 ref state recording the push,
   * read back from the relay. Null when the push did not land, and also when
   * it did but the record was not published yet — never fabricated.
   */
  pushRecordEventId: string | null;
  /** The repository's `d` tag. */
  repoId: string;
  /** The relay git URL the repository is served at. */
  cloneUrl: string;
  /** Event id of the kind:30617 announcement. */
  announcementEventId: string;
  /** The branch the seed commit is on. */
  branch: string;
  /** The role directories seeded. */
  roles: string[];
  /** Whether the push reached the relay. */
  pushed: boolean;
  /** The push's own words when it did not. */
  pushError: string | null;
  /** The relay's refusal of a published event, when one was refused. */
  publicationError: string | null;
};

/**
 * Give a project its own packs repository, seeded from the shipped defaults.
 *
 * Announces a kind:30617 under the viewer's key, seeds and pushes one commit
 * over the existing credential helper, then publishes the kind:30624 — and
 * only if the push landed. The screen must read `pushed` before it says
 * "created": a repository that holds nothing is not a packs repository.
 */
export async function projectPacksInit(input: {
  /** `30621:<owner-hex>:<slug>`. */
  projectRef: string;
  /** Defaults to `<project-slug>-packs`. */
  repoId?: string | null;
}): Promise<ProjectPacksInit> {
  return await invokeTauri<ProjectPacksInit>("project_packs_init", {
    projectRef: input.projectRef,
    repoId: input.repoId ?? null,
  });
}

/** Drop a staged seat. Succeeds when the provider already consumed it. */
export async function clearCodingSessionActorSeat(
  commandId: string,
): Promise<void> {
  await invokeTauri("clear_coding_session_actor_seat", { commandId });
}
