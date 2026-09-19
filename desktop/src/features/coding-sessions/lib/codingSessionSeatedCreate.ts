/**
 * The order a seated create — and a seated reconnect — must happen in, as one
 * testable step.
 *
 * Two things must be true before an actor create is published, and both can
 * fail: the actor has to be a member of the session channel (the relay takes
 * a 44220 from nobody else), and its key material has to be staged
 * host-locally under this exact `commandId` (the provider refuses a create
 * whose actor it cannot resolve, `ACTOR_UNAVAILABLE`). If either fails the
 * create is NOT published and the reason is named — a half-seated session
 * that looks created and answers nothing is the failure this prevents.
 *
 * A resume needs only the second of those: the actor is already a member from
 * the create, but its custody entry was consumed when the first adapter
 * spawned, so the reconnect stages its own under its own `commandId`.
 *
 * The publish itself is injected so the guarantee is a property of these
 * functions rather than a comment in the create hook.
 *
 * **The project's pack source rides the staging call** (finding 84). A
 * project that publishes a kind:30624 names the packs repository its seats
 * stage their role packs from. The launch dialog's preview read it; the real
 * staging call on launch and hire did not, so every seat was staged from the
 * copy installed or shipped on this computer and the repository the project
 * pointed at was never staged from. The create now resolves that record the
 * same way the preview does ({@link SeatedCodingSessionCreateDeps.fetchPackSource})
 * and hands it to the host with the seat.
 */
import type { CodingSessionActorSeat } from "./codingSessionActorSeat";
import type {
  CodingSessionProjectPackSource,
  CodingSessionSeatPackRef,
} from "./codingSessionActorSeatCustody";
import type { ProjectPackSource } from "@/features/projects-container/lib/projectPackSource";
import { normalizeProjectCoordinate } from "@/shared/lib/projectAgentAssociation";

/**
 * Read the project's newest kind:30624, or `null` when it publishes none.
 *
 * The same reader the launch dialog's pack preview uses
 * (`codingSessionPackStatus.ts`), so the pack a seat is staged from is the
 * pack the dialog said it would be. Injected so this module stays free of
 * relay imports, and so a test can watch which project was asked about.
 */
export type CodingSessionSeatPackSourceReader = (
  projectRef: string,
) => Promise<CodingSessionProjectPackSource | null>;

/**
 * What one staging call actually put on disk, as this module reads it back.
 *
 * `packRef` is read-optional: a backend from before it was reported answers
 * `packStaged` alone, and that is reported as "unknown provenance", never
 * turned into a claim about a repository.
 */
export type CodingSessionSeatStaged = {
  packStaged: boolean;
  packRef?: CodingSessionSeatPackRef | null;
};

export type CodingSessionSeatCustody = {
  /**
   * Write the host-local custody entry for this exact command.
   *
   * May report what it staged (a seat with a role pack, or one without, and
   * the repository commit the pack came from when one can vouch for it); this
   * step does not act on that, but a crew launch has to be able to say which
   * of the two happened.
   */
  stageSeat: (input: {
    commandId: string;
    agentPubkey: string;
    /**
     * The role the seat holds on this execution. The host stages **that
     * role's** pack, never the actor's home role's — a `builder` identity
     * seated as `architect` is an architect here, and was being handed the
     * builder's skills before this was passed.
     */
    role?: string | null;
    /**
     * The project's kind:30624 pack source, decoded, or `null` when the
     * project publishes none (the host then stages its local copy, exactly
     * as before). Present on every staging call so a host that is handed
     * nothing was handed nothing on purpose.
     */
    packSource?: CodingSessionProjectPackSource | null;
    /**
     * The project the agent must belong to, native defense in depth: when set,
     * the host refuses to stage an agent whose recorded `project_ref` differs
     * (`SEAT_NOT_PROJECT_AGENT`). Present only on a **new selection** — a
     * create (hire, team launch, seat picker) into a project. A resume of an
     * existing execution never sends it, so historical executions stay
     * resumable and attributed to whoever ran them.
     */
    requireProjectRef?: string | null;
    /**
     * True on a **new selection** (hire, team launch, seat picker). The host
     * then also requires the seat's role to be the agent's primary role, and
     * a projectless create to take only an agent of no project. Absent on a
     * resume, which continues an execution that already exists.
     */
    newSelection?: boolean;
  }) => Promise<CodingSessionSeatStaged | undefined>;
  /** Drop the custody entry again. Best effort; never fails the publish. */
  clearSeat: (commandId: string) => Promise<void>;
  /**
   * Resolve the project's pack source for a staging call. Optional here
   * because a resume has no project coordinate at hand; required on the
   * create path ({@link SeatedCodingSessionCreateDeps}).
   */
  fetchPackSource?: CodingSessionSeatPackSourceReader;
};

/**
 * Told what staging actually put on disk, before the publish.
 *
 * Only called when a seat was staged. A create with no seat, and one refused
 * before staging, report nothing at all — and neither does a backend too old
 * to answer, because "unknown" and "no role pack" are different facts and
 * only one of them is worth putting on a screen.
 *
 * `packRef` names the repository commit the pack was staged from, when one
 * can vouch for it, so a screen can say "staged from 30617:…@<sha>" rather
 * than "a pack". `null` is a pack this computer alone vouches for, a packless
 * seat, or a backend that did not say.
 */
export type CodingSessionSeatStagedReporter = (staged: {
  packStaged: boolean;
  packRef: CodingSessionSeatPackRef | null;
}) => void;

export type SeatedCodingSessionCreateDeps = CodingSessionSeatCustody & {
  /** Add the actor to the channel; throws with actionable copy on failure. */
  ensureMembership: (input: {
    channelId: string;
    actorPubkey: string;
    actorLabel: string | null;
  }) => Promise<void>;
  /**
   * Read the project's newest kind:30624. Required on a create: a seat
   * launched or hired into a project that names a packs repository must be
   * staged from it, and a create that cannot ask is a create that would stage
   * the wrong pack quietly.
   */
  fetchPackSource: CodingSessionSeatPackSourceReader;
};

/**
 * The staging call's view of a decoded 30624 — the wire's `ref` under the
 * host's `gitRef` name, everything else verbatim. The one mapping the preview
 * and the create share, so the two cannot disagree about which repository
 * the project named.
 */
export function codingSessionSeatPackSource(
  source: ProjectPackSource | null,
): CodingSessionProjectPackSource | null {
  if (source === null) return null;
  return {
    repo: source.repo,
    gitRef: source.ref,
    sha: source.sha,
    path: source.path,
  };
}

/**
 * The `requireProjectRef` a create into `projectRef` stages with: the
 * normalized coordinate (what the agent record stores), the trimmed text when
 * it does not normalize (so the host refuses rather than skipping the check),
 * or null for a projectless create.
 */
export function codingSessionSeatRequiredProjectRef(
  projectRef: string | null | undefined,
): string | null {
  const trimmed = projectRef?.trim();
  if (!trimmed) return null;
  return normalizeProjectCoordinate(trimmed) ?? trimmed;
}

/**
 * Stage a seat's key material, publish, and take the entry back if nothing
 * went out.
 *
 * Custody is keyed by `commandId`, and the provider consumes the entry on the
 * command that names it — so **every** command that spawns a process for a
 * seat needs its own entry, not just the create. A resume is exactly that: it
 * mints a fresh `commandId` and starts a new adapter process, which needs the
 * same identity the create gave the first one.
 */
async function publishWithStagedSeat<T>(input: {
  commandId: string;
  actorPubkey: string;
  actorRole?: string | null;
  /**
   * The project the seat is created into, `30621:<owner>:<slug>`, or `null`
   * for a standalone session. With a project and a reader, the project's
   * 30624 is resolved and staged from; otherwise the host stages its local
   * copy, as it always did.
   */
  projectRef?: string | null;
  /** See `CodingSessionSeatCustody.stageSeat`; omitted when null. */
  requireProjectRef?: string | null;
  /** See `CodingSessionSeatCustody.stageSeat`; omitted when not true. */
  newSelection?: boolean;
  /** The seat's own worktree, for the § 4.9 branch override; omitted when unknown. */
  worktree?: string | null;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
  onSeatStaged?: CodingSessionSeatStagedReporter;
}): Promise<T> {
  const packSource = await resolveSeatPackSource(
    input.projectRef ?? null,
    input.deps.fetchPackSource,
  );
  let staged: CodingSessionSeatStaged | undefined;
  try {
    staged = await input.deps.stageSeat({
      commandId: input.commandId,
      agentPubkey: input.actorPubkey,
      role: input.actorRole ?? null,
      packSource,
      ...(input.requireProjectRef
        ? { requireProjectRef: input.requireProjectRef }
        : {}),
      ...(input.newSelection === true ? { newSelection: true } : {}),
      ...(input.worktree ? { worktree: input.worktree } : {}),
    });
  } catch (error) {
    // Staging can fail after it has written part of what it was asked to —
    // the § 4.11 agents clone is cut before the custody entry is filed — so
    // the entry is dropped rather than left under a command id no create will
    // ever name. Best effort, and a no-op when nothing was written; the
    // caller still gets the host's own error (ledger 169).
    await input.deps.clearSeat(input.commandId).catch(() => {});
    throw error;
  }
  if (staged) {
    input.onSeatStaged?.({
      packStaged: staged.packStaged,
      packRef: staged.packRef ?? null,
    });
  }
  try {
    return await input.publish();
  } catch (error) {
    // Nothing went out, so the staged secret has no command to belong to.
    await input.deps.clearSeat(input.commandId).catch(() => {});
    throw error;
  }
}

/**
 * The project's pack source for one staging call, or `null` when there is no
 * project or it publishes no 30624.
 *
 * A reader that fails does not degrade to `null`: that would stage this
 * computer's copy in place of the repository the project named — the exact
 * quiet substitution the source exists to end — so the create is refused
 * with the reader's own words instead.
 */
export async function resolveSeatPackSource(
  projectRef: string | null,
  fetchPackSource: CodingSessionSeatPackSourceReader | undefined,
): Promise<CodingSessionProjectPackSource | null> {
  const project = projectRef?.trim();
  if (!project || fetchPackSource === undefined) return null;
  try {
    return await fetchPackSource(project);
  } catch (error) {
    const reason =
      error instanceof Error && error.message.trim()
        ? error.message
        : "the relay did not answer";
    throw new Error(
      `Could not read the project's pack source (kind 30624 for ${project}): ${reason}`,
    );
  }
}

/**
 * Reconnect a seated execution, staging its identity for the new generation.
 *
 * The create's custody entry was consumed when the first adapter spawned, so
 * a resume that stages nothing is refused by the provider with
 * `ACTOR_UNAVAILABLE` — permanently, on the very host that holds the key.
 * With no actor this is exactly `publish()`.
 */
export async function publishSeatedCodingSessionResume<T>(input: {
  commandId: string;
  actorPubkey: string | null;
  /**
   * The role this execution's seat holds, from its own 44223. A resume stages
   * a fresh custody entry, so it must name the same role the create did or the
   * new generation would run another role's pack.
   */
  actorRole?: string | null;
  /**
   * The project the execution belongs to, from its 44223, when the caller
   * knows it. With it — and a custody seam that can read 30624s — the new
   * generation is staged from the same repository the create was; without it
   * the host stages its local copy.
   */
  projectRef?: string | null;
  /** The seat's own worktree, for the § 4.9 branch override; omitted when unknown. */
  worktree?: string | null;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
  /** Called with what staging actually put on disk, before the publish. */
  onSeatStaged?: CodingSessionSeatStagedReporter;
}): Promise<T> {
  if (!input.actorPubkey) return input.publish();
  return publishWithStagedSeat({
    commandId: input.commandId,
    actorPubkey: input.actorPubkey,
    actorRole: input.actorRole ?? null,
    projectRef: input.projectRef ?? null,
    ...(input.worktree ? { worktree: input.worktree } : {}),
    publish: input.publish,
    deps: input.deps,
    ...(input.onSeatStaged ? { onSeatStaged: input.onSeatStaged } : {}),
  });
}

/**
 * Publish a create, seating an agent first when one was chosen.
 *
 * With no seat this is exactly `publish()` — an unseated create keeps today's
 * behaviour, including doing no channel or custody work at all.
 */
export async function publishSeatedCodingSessionCreate<T>(input: {
  channelId: string;
  commandId: string;
  seat: CodingSessionActorSeat | null;
  seatLabel?: string | null;
  /**
   * The project coordinate the create is signed with, or `null` for a
   * standalone session. This is what the seat's pack source is looked up by:
   * the same coordinate the 44221 carries, so the pack a seat runs with is the
   * pack of the project it is filed under.
   */
  projectRef: string | null;
  publish: () => Promise<T>;
  deps: SeatedCodingSessionCreateDeps;
  /** Called with what staging actually put on disk, before the publish. */
  onSeatStaged?: CodingSessionSeatStagedReporter;
  /**
   * The directory the seat runs in when it is a worktree the launch cut.
   * The host decides whether that branch overrides the role (spec § 4.9);
   * a checkout that is not a worktree of the packs repository changes
   * nothing.
   */
  worktree?: string | null;
}): Promise<T> {
  if (!input.seat) return input.publish();

  await input.deps.ensureMembership({
    channelId: input.channelId,
    actorPubkey: input.seat.actor,
    actorLabel: input.seatLabel ?? null,
  });
  return publishWithStagedSeat({
    commandId: input.commandId,
    actorPubkey: input.seat.actor,
    // The seat's own role, not the actor's home role: this is the whole fix.
    actorRole: input.seat.role,
    projectRef: input.projectRef,
    // A create is a new selection, so the host re-checks that the chosen
    // agent belongs to the project it is filed under. Every new-selection
    // path (hire, team launch, seat picker) comes through here; a resume
    // does not (`publishSeatedCodingSessionResume`).
    requireProjectRef: codingSessionSeatRequiredProjectRef(input.projectRef),
    newSelection: true,
    ...(input.worktree ? { worktree: input.worktree } : {}),
    publish: input.publish,
    deps: input.deps,
    ...(input.onSeatStaged ? { onSeatStaged: input.onSeatStaged } : {}),
  });
}
