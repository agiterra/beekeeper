/** Test-store projection only: signature, protected CAS and runtime effects require native evidence. */
type PersonaFields = {
  id: string;
  display_name: string;
  avatar_url: string | null;
  system_prompt: string;
  runtime?: string | null;
  model?: string | null;
  provider?: string | null;
  name_pool?: string[];
  env_vars?: Record<string, string>;
  respond_to?: string | null;
  respond_to_allowlist?: string[];
  parallelism?: number | null;
  updated_at: string;
  is_builtin: boolean;
  shared: boolean;
  source_team?: string | null;
  catalog_source?: unknown;
};
type PersonaInput = {
  id: string;
  displayName: string;
  avatarUrl?: string | null;
  systemPrompt: string;
  runtime?: string | null;
  model?: string | null;
  provider?: string | null;
  namePool?: string[];
  envVars?: Record<string, string>;
  behavior?: {
    respondTo?: "owner-only" | "allowlist" | "anyone";
    respondToAllowlist?: string[];
    parallelism?: number;
  };
};
type EditableAgent = {
  pubkey: string;
  persona_id: string | null;
  backend: { type: string };
  name: string;
  model: string | null;
  system_prompt: string | null;
  env_vars?: Record<string, string>;
  respond_to: "owner-only" | "allowlist" | "anyone";
  respond_to_allowlist: string[];
  updated_at: string;
};
/** Existing mock instance-update fields, plus the explicit owned profile contract. */
export type MockManagedAgentUpdateInput = {
  pubkey: string;
  name?: string;
  model?: string | null;
  systemPrompt?: string | null;
  envVars?: Record<string, string>;
  respondTo?: "owner-only" | "allowlist" | "anyone";
  respondToAllowlist?: string[];
  ownedPersonaUpdate?: { expectedUpdatedAt: string; definition: PersonaInput };
};

/** Common mock persona field rules; ordinary update_persona retains its previous prompt trimming. */
export function applyMockPersonaFields(
  persona: PersonaFields,
  input: PersonaInput,
  trimPrompt = false,
): void {
  persona.display_name = input.displayName.trim();
  persona.avatar_url = input.avatarUrl?.trim() || null;
  persona.system_prompt = trimPrompt
    ? input.systemPrompt.trim()
    : input.systemPrompt;
  persona.runtime = input.runtime?.trim() || null;
  persona.model = input.model?.trim() || null;
  persona.provider = input.provider?.trim() || null;
  if (input.namePool !== undefined)
    persona.name_pool = input.namePool
      .map((name) => name.trim())
      .filter(Boolean);
  if (input.envVars !== undefined) persona.env_vars = { ...input.envVars };
  if (input.behavior !== undefined) {
    persona.respond_to = input.behavior.respondTo ?? null;
    persona.respond_to_allowlist =
      input.behavior.respondTo === "allowlist"
        ? [...(input.behavior.respondToAllowlist ?? [])]
        : [];
    persona.parallelism = input.behavior.parallelism ?? null;
  }
}

/** Stage both caches before any mutation; mirror the bounded owned-edit refusal, not native CAS. */
export function applyMockManagedAgentUpdate(
  agent: EditableAgent,
  input: MockManagedAgentUpdateInput,
  personas: PersonaFields[],
  agents: readonly EditableAgent[],
): void {
  let source: PersonaFields | undefined;
  let staged: PersonaFields | undefined;
  const edit = input.ownedPersonaUpdate;
  if (edit) {
    const matching = personas.filter((p) => p.id === agent.persona_id);
    source = matching[0];
    if (
      agent.backend.type !== "local" ||
      !source ||
      matching.length !== 1 ||
      edit.definition.id !== agent.persona_id
    )
      throw new Error("owned definition edit targets another persona");
    if (
      source.is_builtin ||
      source.shared ||
      source.source_team ||
      source.catalog_source
    )
      throw new Error(
        "this edit requires an owned private standalone definition",
      );
    if (agents.filter((a) => a.persona_id === source?.id).length !== 1)
      throw new Error(
        "this definition is linked to other agents; edit their configuration explicitly",
      );
    staged = { ...source };
    applyMockPersonaFields(staged, edit.definition);
    if (!staged.display_name) throw new Error("Display name is required");
    const exactAlreadyCommitted =
      JSON.stringify(source) === JSON.stringify(staged) &&
      agent.system_prompt === staged.system_prompt;
    if (source.updated_at !== edit.expectedUpdatedAt && !exactAlreadyCommitted)
      throw new Error("owned definition changed; refresh before editing");
  }
  const next = { ...agent };
  if (input.name !== undefined) next.name = input.name;
  if (input.model !== undefined) next.model = input.model;
  if (input.systemPrompt !== undefined) next.system_prompt = input.systemPrompt;
  if (input.envVars !== undefined) next.env_vars = { ...input.envVars };
  if (input.respondTo !== undefined) next.respond_to = input.respondTo;
  if (input.respondToAllowlist !== undefined)
    next.respond_to_allowlist = [...input.respondToAllowlist];
  if (staged) next.system_prompt = staged.system_prompt;
  const now = new Date().toISOString();
  next.updated_at = now;
  if (source && staged) Object.assign(source, staged, { updated_at: now });
  Object.assign(agent, next);
}
