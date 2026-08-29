/**
 * The `model` / `provider` fields an instance edit puts on the wire.
 *
 * Both are HOST-owned identity facts — what this computer runs the identity on
 * — so they are submitted whether or not the instance is linked to a definition
 * (`docs/CREW_SESSIONS_PLAN.md` §3.1 D11-D13). Gating them on "not linked" is
 * what made the model control render, accept a pick, save, and change nothing
 * for every team identity; `linked` is kept in the signature only to document
 * that it deliberately does not gate.
 *
 * `undefined` means "omit from the patch" (unchanged, or a state we must not
 * write); `null` means "clear back to inheritance".
 *
 * Provider is additionally keyed on the effective post-submit runtime's
 * capability: `capable` persists it, `locked` clears a set one, and `unknown`
 * always omits so a not-yet-loaded catalog never becomes a destructive write.
 */
export function hostOwnedModelProviderSubmission({
  model,
  provider,
  agentModel,
  agentProvider,
  providerRuntimeCapability,
}: {
  /** Unused by design — see the note above. */
  linked: boolean;
  model: string | null;
  provider: string | null;
  agentModel: string | null;
  agentProvider: string | null;
  providerRuntimeCapability: "capable" | "locked" | "unknown";
}): { model: string | null | undefined; provider: string | null | undefined } {
  const nextModel = model !== agentModel ? model : undefined;
  let nextProvider: string | null | undefined;
  if (providerRuntimeCapability === "capable") {
    nextProvider = provider !== agentProvider ? provider : undefined;
  } else if (providerRuntimeCapability === "locked") {
    nextProvider = agentProvider !== null ? null : undefined;
  } else {
    nextProvider = undefined;
  }
  return { model: nextModel, provider: nextProvider };
}
