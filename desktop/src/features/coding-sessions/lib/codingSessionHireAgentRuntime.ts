/**
 * Which runtime an agent is actually configured for, as a hire must read it.
 *
 * **This is the fix for a hire that ignored the dashboard.** On 2026-09-16 a
 * lead hired Kiln, whose Agents screen says Codex; the hire went to
 * `claude-primary` and was refused `HIRE_MODEL_NOT_OFFERED` for a model
 * (`gpt-5.6-terra`) that is not a Claude id, and the lead only recovered by
 * re-hiring with `--provider-instance codex-primary` (ledger 135(b)).
 *
 * The cause is an asymmetry in one record. `ManagedAgentSummary` resolves
 * `model` *effectively* — record, then the linked persona, then the global
 * default, with `model_source` naming the tier
 * (`desktop/src-tauri/src/managed_agents/runtime.rs:175`) — but publishes
 * `runtime` as the raw per-instance pin
 * (`runtime.rs:256`, `runtime: record.runtime.clone()`), which is `null`
 * exactly when the agent inherits its harness from its persona. Kiln inherits.
 * So the hire read a model from the persona tier and a runtime from a tier
 * that had none, concluded the identity pinned no runtime at all, and fell
 * through to this computer's first available one — Claude — carrying an
 * OpenAI model id.
 *
 * `agentCommand` on the same record is **not** raw: it is the effective
 * harness the native resolver produced (`resolve_effective_harness_descriptor`,
 * `runtime.rs:223`), which is what the Agents screen matches against the ACP
 * catalog to draw "Codex" (`AgentInstanceEditDialog.tsx:206-207`). Reading it
 * through the same catalog here is what makes the hire agree with the screen
 * a person just looked at.
 *
 * **Ledger 139: the native fix.** `ManagedAgentSummary` now publishes
 * `effective_runtime`/`runtime_source` alongside the raw `runtime` above —
 * the same record → definition resolution this file used to reconstruct by
 * hand through the ACP catalog (`desktop/src-tauri/src/managed_agents/
 * runtime/effective_summary.rs`, `summary_effective_runtime`). When a record
 * carries that field, it is taken as the answer directly and the catalog
 * reconstruction below never runs — one native resolution beats two client
 * ones re-deriving the same fact from `agentCommand`. The catalog fallback
 * chain stays for a record from a host that has not yet republished the new
 * field (an older bundled `bee`/backend a seat is still running against), so
 * a hire never regresses to "no runtime" merely because the field is absent.
 * The native `instance`/`instance_legacy` tier reports as `"record"` and
 * `definition` reports as `"harness"`, reusing the existing two sentences
 * (`describeCodingSessionHireRuntimeSource`) rather than adding a third,
 * because they already say the same two facts: "the record pinned it" or
 * "the persona's harness names it".
 */

/** Where an agent's runtime was read from. Named in every refusal. */
export type CodingSessionHireRuntimeSource =
  /** The per-instance pin a human set on this computer. */
  | "record"
  /** The effective harness command, i.e. the linked persona's runtime. */
  | "harness"
  /** The record's inference provider, the last and weakest fallback. */
  | "provider";

export type CodingSessionHireAgentRuntime = {
  /** The runtime slug, lower-cased, or null when the record pins none. */
  runtime: string | null;
  /** Which tier answered. Null exactly when `runtime` is null. */
  source: CodingSessionHireRuntimeSource | null;
  /** The raw value that was read, for a sentence that cites its evidence. */
  read: string | null;
};

/**
 * The native `runtime_source` tiers `ManagedAgentSummary` can report
 * (`ConfigSource`, snake-cased). No `"global"` case — there is no global
 * tier for runtime, see `EffectiveAgentConfig::runtime`'s doc.
 */
export type CodingSessionHireEffectiveRuntimeSource =
  | "instance"
  | "definition"
  | "instance_legacy";

export type ResolveCodingSessionHireAgentRuntimeInput = {
  /** `ManagedAgent.runtime` — the per-instance pin, `null` when inherited. */
  runtime?: string | null;
  /**
   * `ManagedAgent.effectiveRuntime` — the host's own record → definition
   * resolution (ledger 139). Preferred over every fallback below when
   * non-blank; leave absent/null for a host that has not republished this
   * field yet, and the catalog fallback chain applies unchanged.
   */
  effectiveRuntime?: string | null;
  /** `ManagedAgent.runtimeSource`, paired with `effectiveRuntime`. */
  effectiveRuntimeSource?: CodingSessionHireEffectiveRuntimeSource | null;
  /** `ManagedAgent.agentCommand` — the *effective* harness command. */
  agentCommand?: string | null;
  /** `ManagedAgent.provider`. */
  provider?: string | null;
  /**
   * This computer's ACP catalog as a command/id → runtime-id lookup.
   *
   * Absent means no catalog was read, and then the harness tier answers
   * nothing: a runtime guessed from an unmatched command string would be a
   * claim about a binary nobody probed.
   */
  runtimeIdForCommand?: (command: string) => string | null;
};

/** Resolve the runtime a hire seats this agent on. */
export function resolveCodingSessionHireAgentRuntime(
  input: ResolveCodingSessionHireAgentRuntimeInput,
): CodingSessionHireAgentRuntime {
  const native = (input.effectiveRuntime ?? "").trim();
  if (native.length > 0) {
    const source: CodingSessionHireRuntimeSource =
      input.effectiveRuntimeSource === "definition" ? "harness" : "record";
    return { runtime: native.toLowerCase(), source, read: native };
  }
  const pinned = (input.runtime ?? "").trim();
  if (pinned.length > 0) {
    return { runtime: pinned.toLowerCase(), source: "record", read: pinned };
  }
  const command = (input.agentCommand ?? "").trim();
  if (command.length > 0 && input.runtimeIdForCommand) {
    const matched = (input.runtimeIdForCommand(command) ?? "").trim();
    if (matched.length > 0) {
      return {
        runtime: matched.toLowerCase(),
        source: "harness",
        read: command,
      };
    }
  }
  const provider = (input.provider ?? "").trim();
  if (provider.length > 0) {
    return {
      runtime: provider.toLowerCase(),
      source: "provider",
      read: provider,
    };
  }
  return { runtime: null, source: null, read: null };
}

/**
 * A command/id → runtime-id lookup over this computer's ACP catalog.
 *
 * The same two-pass match the Agents screen uses: the entry's resolved
 * command first, then its id, so a record that pins `codex-acp` and one that
 * pins `codex` land on the same runtime.
 */
export function codingSessionHireRuntimeIdLookup(
  entries: readonly { id: string; command: string | null }[],
): (command: string) => string | null {
  const byCommand = new Map<string, string>();
  const byId = new Map<string, string>();
  for (const entry of entries) {
    const id = entry.id.trim();
    if (id.length === 0) continue;
    const command = entry.command?.trim() ?? "";
    if (command.length > 0 && !byCommand.has(command)) {
      byCommand.set(command, id);
    }
    if (!byId.has(id)) byId.set(id, id);
  }
  return (command: string) => {
    const asked = command.trim();
    if (asked.length === 0) return null;
    return byCommand.get(asked) ?? byId.get(asked) ?? null;
  };
}

/** How a runtime tier is named to a person. Never a bare token. */
export function describeCodingSessionHireRuntimeSource(
  source: CodingSessionHireRuntimeSource,
  read: string | null,
): string {
  const said = read?.trim();
  switch (source) {
    case "record":
      return "set on its record on this computer";
    case "harness":
      return said
        ? `from the harness its record resolves to, ${said}`
        : "from the harness its record resolves to";
    case "provider":
      return said
        ? `from its record's inference provider, ${said}`
        : "from its record's inference provider";
  }
}

/**
 * Which tier an agent's effective model came from — `ManagedAgent.modelSource`.
 *
 * Carried into refusals because the one the hire quoted on 2026-09-16 read as
 * a claim about "Kiln's record", while Kiln's record names no model of its
 * own: the id came from the linked persona. A message that names the wrong
 * file sends a person to edit a field that is already empty.
 */
export type CodingSessionHireModelSource =
  | "instance"
  | "definition"
  | "global"
  | "instance_legacy";

/** Where a model id was read from, as a sentence fragment. */
export function describeCodingSessionHireModelSource(
  source: CodingSessionHireModelSource | null | undefined,
): string {
  switch (source) {
    case "instance":
    case "instance_legacy":
      return "its own record on this computer names";
    case "definition":
      return "the persona its record is linked to names";
    case "global":
      return "this computer's default model is";
    default:
      // No tier was reported. Say what was read without claiming a file.
      return "the record this computer resolved for it says";
  }
}
