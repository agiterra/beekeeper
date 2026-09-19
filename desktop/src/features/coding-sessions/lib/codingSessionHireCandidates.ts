/**
 * The managed agents a hire host may seat, and the facts the decision reads
 * off each one.
 *
 * Split from `useCodingSessionHire.ts` (size) so the mapping from this
 * computer's agent records to hire candidates is one tested function rather
 * than an inline object literal in the hook — and so the one project fact a
 * hire is scoped by, `projectRef`, cannot be dropped on the way through.
 */
import type { ManagedAgent } from "@/shared/api/types";
import {
  resolveCodingSessionHireAgentRuntime,
  type CodingSessionHireModelSource,
  type CodingSessionHireRuntimeSource,
} from "./codingSessionHireAgentRuntime";
import type { CodingSessionHireCandidate } from "./codingSessionHirePolicy";

/** A managed agent, narrowed to what seating one needs. */
export type CodingSessionHireAgent = {
  pubkey: string;
  name: string;
  homeRole: string | null;
  /**
   * The project this agent durably belongs to, or null for none. Hiring
   * seats only agents of the umbrella's own project — see
   * `codingSessionHirePolicy.decideCodingSessionHire`.
   */
  projectRef: string | null;
  /**
   * The record's persona id. Read only to recognise a project team setup
   * actor, which is never hired (`isCodingSessionHireSetupActor`).
   */
  personaId?: string | null;
  hasRolePack?: boolean;
  model: string | null;
  /**
   * The runtime this agent is **effectively** configured for (`claude`,
   * `codex`, `goose`), or null when nothing on its record names one. This
   * decides the seat's runtime — see `codingSessionHirePolicy.chooseProvider`.
   *
   * Effective, not the raw per-instance pin: an agent that inherits its
   * harness from its persona pins nothing, and reading the pin alone is what
   * sent a Codex identity to `claude-primary` (ledger 135(b)). See
   * {@link resolveCodingSessionHireAgentRuntime}.
   */
  runtime?: string | null;
  /** Which tier {@link runtime} was read from, for the refusal sentence. */
  runtimeSource?: CodingSessionHireRuntimeSource | null;
  /** The raw value that tier held, so a refusal cites what it read. */
  runtimeRead?: string | null;
  /** The record's inference provider, read only as a fallback for `runtime`. */
  provider?: string | null;
  /** Which tier the effective {@link CodingSessionHireAgent.model} came from. */
  modelSource?: CodingSessionHireModelSource | null;
};

/**
 * This computer's managed agents as hire agents.
 *
 * Setup actors are **kept** here on purpose: the list is also how the host
 * names the seat that asked for a hire, and a setup session's lead may be the
 * requester. The decision excludes them from candidates instead.
 */
export function codingSessionHireAgentsFromManaged(
  agents: readonly ManagedAgent[],
  options?: {
    /**
     * This computer's ACP catalog as a command → runtime-id lookup. Absent
     * means none was read, and an agent that inherits its harness then names
     * no runtime rather than being assigned a guessed one.
     */
    runtimeIdForCommand?: (command: string) => string | null;
  },
): CodingSessionHireAgent[] {
  return agents.map((agent) => {
    const runtime = resolveCodingSessionHireAgentRuntime({
      runtime: agent.runtime,
      // The host's own record → definition resolution (ledger 139) answers
      // before the catalog chain; until item 165 it was never handed over.
      effectiveRuntime: agent.effectiveRuntime ?? null,
      effectiveRuntimeSource: agent.runtimeSource ?? null,
      agentCommand: agent.agentCommand,
      provider: agent.provider,
      ...(options?.runtimeIdForCommand
        ? { runtimeIdForCommand: options.runtimeIdForCommand }
        : {}),
    });
    return {
      pubkey: agent.pubkey,
      name: agent.name,
      homeRole: agent.homeRole,
      // `undefined` is an older backend that records no association: no
      // project, which a project session will never seat.
      projectRef: agent.projectRef ?? null,
      personaId: agent.personaId,
      ...(agent.hasRolePack === undefined
        ? {}
        : { hasRolePack: agent.hasRolePack }),
      model: agent.model,
      runtime: runtime.runtime,
      runtimeSource: runtime.source,
      runtimeRead: runtime.read,
      provider: agent.provider,
      modelSource: agent.modelSource,
    };
  });
}

/** Hire agents as the decision's candidates. */
export function codingSessionHireCandidatesOf(
  agents: readonly CodingSessionHireAgent[],
): CodingSessionHireCandidate[] {
  return agents.map((agent) => ({
    pubkey: agent.pubkey,
    name: agent.name,
    homeRole: agent.homeRole,
    projectRef: agent.projectRef ?? null,
    ...(agent.personaId ? { personaId: agent.personaId } : {}),
    ...(agent.hasRolePack === undefined
      ? {}
      : { hasRolePack: agent.hasRolePack }),
    model: agent.model,
    runtime: agent.runtime ?? null,
    runtimeSource: agent.runtimeSource ?? null,
    runtimeRead: agent.runtimeRead ?? null,
    provider: agent.provider ?? null,
    modelSource: agent.modelSource ?? null,
  }));
}
