/**
 * Pure model behind "Add a provider to this session" (design §B).
 *
 * Joining an umbrella is an ordinary 44221 create that happens to carry the
 * umbrella's existing `sessionRef`; everything else — provider pick, model,
 * working directory, durable create, receipt wait — is the founding flow
 * unchanged. What this module owns is the two things joining adds: the channel
 * is pinned (a joining execution must live in the same host channel as the
 * session it joins) and the runtime picker says which runtimes the session
 * already has, without hiding them.
 */
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  isNewCodingSessionTargetReady,
  type NewCodingSessionTarget,
} from "@/features/coding-sessions/lib/newCodingSessionModel";

/** One pickable provider for the join, annotated with umbrella membership. */
export type AddCodingSessionProviderOption = {
  target: NewCodingSessionTarget;
  /**
   * This umbrella already runs an execution on this (signer, runtime). Shown
   * and still selectable — a second Claude execution is a legitimate thing to
   * want — but never the default, so "add a provider" never silently means
   * "add another of the one you already have".
   */
  alreadyInSession: boolean;
};

/**
 * The join picker's options, in the create screen's own target order.
 *
 * Targets are resolved for the umbrella's channel only, so the channel is
 * pinned structurally: there is no option here that would put the joining
 * execution somewhere else.
 */
export function listAddCodingSessionProviderOptions(input: {
  targets: readonly NewCodingSessionTarget[];
  umbrella: Pick<CodingSessionUmbrellaRecord, "executions">;
}): AddCodingSessionProviderOption[] {
  const present = new Set<string>();
  for (const execution of input.umbrella.executions) {
    const record = execution.activeGeneration;
    const signer = execution.signerPubkey;
    if (record.commandTarget) {
      present.add(runtimeIdentity(signer, record.commandTarget.driver));
    }
    if (record.runtime) present.add(runtimeIdentity(signer, record.runtime));
  }
  return input.targets.map((target) => ({
    target,
    alreadyInSession:
      present.has(
        runtimeIdentity(target.signerPubkey, target.provider.driver),
      ) ||
      present.has(
        runtimeIdentity(target.signerPubkey, target.provider.runtime),
      ),
  }));
}

/**
 * The option a freshly opened join dialog starts on: the first ready provider
 * the session does not already have. When every ready provider is already in
 * the session, nothing is preselected — the person picks deliberately.
 */
export function defaultAddCodingSessionProviderKey(
  options: readonly AddCodingSessionProviderOption[],
): string | null {
  const fresh = options.find(
    (option) =>
      !option.alreadyInSession && isNewCodingSessionTargetReady(option.target),
  );
  return fresh?.target.selectionKey ?? null;
}

/**
 * Why a join must not be built right now, or null when it may proceed.
 *
 * A join create that omits `genesisRef` is a 9-key *ungoverned* create — the
 * provider accepts it without any authority chain. That is only correct for a
 * genuinely legacy (pre-genesis) session. When the umbrella's genesis is
 * merely *unresolved* (its receipt-joined creates have not been observed) or
 * *conflicted*, omitting the field would route around the provider's own
 * fail-closed genesis handling and attach an ungoverned execution inside a
 * governed session — tonight's `genesisRef: None` forensic. Fail the join
 * visibly instead.
 */
export function addCodingSessionProviderGenesisGateMessage(
  umbrella: Pick<CodingSessionUmbrellaRecord, "genesisResolution">,
): string | null {
  switch (umbrella.genesisResolution) {
    case "governed":
    case "legacy":
      return null;
    case "unresolved":
      return (
        "This session's founding record hasn't been resolved yet, so a new " +
        "provider can't be attached under its authority. Wait a moment for " +
        "the session's records to load and try again."
      );
    case "conflict":
      return (
        "This session's founding records conflict, so a new provider can't " +
        "be attached under its authority."
      );
  }
}

/**
 * The exact create the join publishes, or null when this session cannot take
 * one.
 *
 * Two things make it a join rather than a second session: `sessionRef` is the
 * umbrella's own (never freshly minted), and the target's channel is the
 * umbrella's channel — every execution of one session lives in one host
 * channel, which is also what makes the conversation lane addressable. The
 * title is inherited so the joining execution does not advertise a competing
 * name for the same session.
 *
 * A governed umbrella's `genesisRef` is always carried. An umbrella whose
 * genesis is unresolved or conflicted refuses to build a join at all — see
 * {@link addCodingSessionProviderGenesisGateMessage} — rather than falling
 * back to an ungoverned 9-key create.
 */
export function buildAddCodingSessionProviderSubmit(input: {
  umbrella: Pick<
    CodingSessionUmbrellaRecord,
    "sessionRef" | "genesisRef" | "genesisResolution" | "title" | "executions"
  >;
  channelId: string;
  target: NewCodingSessionTarget | null;
  model: string | null;
  initialTurn: string;
  workdir: string;
}): {
  target: NewCodingSessionTarget;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
  workdir: string | null;
  sessionRef: string;
  genesisRef?: string;
  projectRef: string | null;
  repoRef: string | null;
} | null {
  const sessionRef = input.umbrella.sessionRef;
  const target = input.target;
  if (sessionRef === null || target === null) return null;
  if (target.channelId !== input.channelId) return null;
  if (addCodingSessionProviderGenesisGateMessage(input.umbrella) !== null) {
    return null;
  }
  const title = input.umbrella.title.trim();
  return {
    target,
    model: input.model && input.model.length > 0 ? input.model : null,
    title: title.length > 0 ? title : null,
    initialTurn: input.initialTurn.trim().length > 0 ? input.initialTurn : null,
    workdir: input.workdir.trim().length > 0 ? input.workdir.trim() : null,
    sessionRef,
    ...(input.umbrella.genesisRef
      ? { genesisRef: input.umbrella.genesisRef }
      : {}),
    projectRef: inheritedUmbrellaRef(input.umbrella, "projectRef"),
    repoRef: inheritedUmbrellaRef(input.umbrella, "repoRef"),
  };
}

/**
 * The umbrella's existing claim for a ref field: the first execution (attach
 * order, so the founding execution wins) whose active generation carries a
 * non-null value. A joined execution that published `projectRef: null` would
 * re-file the whole session as standalone whenever its metadata is freshest.
 */
function inheritedUmbrellaRef(
  umbrella: Pick<CodingSessionUmbrellaRecord, "executions">,
  field: "projectRef" | "repoRef",
): string | null {
  for (const execution of umbrella.executions) {
    const value = execution.activeGeneration[field];
    if (value !== null) return value;
  }
  return null;
}

/** The picker note for an option, or null when there is nothing to say. */
export function addCodingSessionProviderOptionNote(
  option: AddCodingSessionProviderOption,
): string | null {
  return option.alreadyInSession ? "already in this session" : null;
}

function runtimeIdentity(signerPubkey: string, runtime: string): string {
  return `${signerPubkey}\u0000${runtime}`;
}
