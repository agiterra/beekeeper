import { toast } from "sonner";
import { showAgentProfileSyncWarning } from "@/features/agents/ui/agentProfileSyncWarning";

import { personaManagedAgentUpdate } from "@/features/profile/ui/UserProfilePanelUtils";
import type {
  AcpRuntimeCatalogEntry,
  AgentPersona,
  CreateManagedAgentResponse,
  CreatePersonaInput,
  ManagedAgent,
  UpdateManagedAgentInput,
  UpdatePersonaInput,
} from "@/shared/api/types";

type SubmitProfilePersonaDialogOptions = {
  createManagedAgentForPersona: (
    persona: AgentPersona,
  ) => Promise<CreateManagedAgentResponse>;
  createPersona: (input: CreatePersonaInput) => Promise<AgentPersona>;
  input: CreatePersonaInput | UpdatePersonaInput;
  managedAgent: ManagedAgent | undefined;
  onDone: () => void;
  previousPersona?: AgentPersona;
  runtimes?: readonly AcpRuntimeCatalogEntry[];
  updateManagedAgent: (
    input: UpdateManagedAgentInput,
  ) => Promise<{ agent: ManagedAgent; profileSyncError: string | null }>;
  updatePersona: (input: UpdatePersonaInput) => Promise<AgentPersona>;
};

type ValidateLinkedAgentRuntimeEditOptions = {
  input: UpdatePersonaInput;
  managedAgent: ManagedAgent | undefined;
  previousPersona?: AgentPersona;
  runtimes?: readonly AcpRuntimeCatalogEntry[];
};

function normalizeRuntimePreference(value: string | null | undefined): string {
  return value?.trim() ?? "";
}

export function validateLinkedAgentRuntimeEdit({
  input,
  managedAgent,
  previousPersona,
  runtimes,
}: ValidateLinkedAgentRuntimeEditOptions): string | null {
  if (!managedAgent || !previousPersona) {
    return null;
  }

  const previousRuntime = normalizeRuntimePreference(previousPersona.runtime);
  const nextRuntime = normalizeRuntimePreference(input.runtime);
  if (previousRuntime === nextRuntime) {
    return null;
  }

  const runtime = runtimes?.find((candidate) => candidate.id === nextRuntime);
  if (runtime?.availability === "available" && runtime.command) {
    return null;
  }

  const runtimeLabel = runtime?.label ?? "This provider";
  return `${runtimeLabel} is not available. Install it before saving this linked agent.`;
}

export async function submitProfilePersonaDialog({
  createManagedAgentForPersona,
  createPersona,
  input,
  managedAgent,
  onDone,
  previousPersona,
  runtimes,
  updateManagedAgent,
  updatePersona,
}: SubmitProfilePersonaDialogOptions) {
  try {
    if ("id" in input) {
      const runtimeEditError = validateLinkedAgentRuntimeEdit({
        input,
        managedAgent,
        previousPersona,
        runtimes,
      });
      if (runtimeEditError) {
        toast.error(runtimeEditError);
        return;
      }

      if (managedAgent?.backend.type === "local") {
        if (
          !previousPersona ||
          managedAgent.personaId !== input.id ||
          previousPersona.id !== input.id
        ) {
          throw new Error(
            "Refresh this agent's owned definition before editing it.",
          );
        }
        const desiredPersona: AgentPersona = {
          ...previousPersona,
          ...input,
          avatarUrl: input.avatarUrl ?? null,
          runtime: input.runtime ?? null,
          model: input.model ?? null,
          provider: input.provider ?? null,
          envVars: input.envVars ?? previousPersona.envVars,
          // Omitted behavior means the dialog did not edit instance access.
          // Preserve its override rather than applying the definition default.
          respondTo: input.behavior
            ? (input.behavior.respondTo ?? null)
            : managedAgent.respondTo,
          respondToAllowlist: input.behavior
            ? input.behavior.respondTo === "allowlist"
              ? (input.behavior.respondToAllowlist ?? [])
              : []
            : managedAgent.respondToAllowlist,
          parallelism: input.behavior
            ? (input.behavior.parallelism ?? null)
            : previousPersona.parallelism,
        };
        const patch = personaManagedAgentUpdate(managedAgent, desiredPersona, {
          previousPersona,
          runtimes,
        }) ?? { pubkey: managedAgent.pubkey };
        const result = await updateManagedAgent({
          ...patch,
          ownedPersonaUpdate: {
            expectedUpdatedAt: previousPersona.updatedAt,
            definition: input,
          },
        });
        if (result.profileSyncError)
          showAgentProfileSyncWarning(
            result.agent.name,
            result.profileSyncError,
          );
        toast.success(`Updated ${input.displayName}.`);
        onDone();
        return;
      }
      const persona = await updatePersona(input);
      const agentUpdate = managedAgent
        ? personaManagedAgentUpdate(managedAgent, persona, {
            previousPersona,
            runtimes,
          })
        : null;
      const result = agentUpdate ? await updateManagedAgent(agentUpdate) : null;
      if (result?.profileSyncError) {
        showAgentProfileSyncWarning(result.agent.name, result.profileSyncError);
      }
      toast.success(`Updated ${input.displayName}.`);
    } else {
      const persona = await createPersona(input);
      try {
        const created = await createManagedAgentForPersona(persona);
        if (created.spawnError) {
          toast.error(
            `${persona.displayName} was created, but it did not start: ${created.spawnError}`,
          );
        } else {
          toast.success(`Created and started ${created.agent.name}.`);
        }
        if (created.profileSyncError) {
          toast.warning(
            `${created.agent.name} was created, but profile sync failed: ${created.profileSyncError}`,
          );
        }
      } catch (error) {
        toast.error(
          error instanceof Error
            ? `${persona.displayName} was created, but the agent instance could not be created: ${error.message}`
            : `${persona.displayName} was created, but the agent instance could not be created.`,
        );
      }
    }

    onDone();
  } catch (error) {
    toast.error(
      error instanceof Error ? error.message : "Failed to save agent.",
    );
  }
}
