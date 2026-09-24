import * as React from "react";

import { useUpdateManagedAgentMutation } from "@/features/agents/hooks";
import { codingSessionSeatRecordModelUpdate } from "../../lib/codingSessionHireModel";
import type { CodingSessionLaunchLead } from "../../lib/codingSessionLaunchForm";
import type { NewCodingSessionSeatRecordUpdate } from "../NewCodingSessionProviderPicker";

/**
 * The "update the record" action under the founded page's model picker.
 *
 * Writes the lead's host-owned `model` through the same mutation the Agents
 * screen's edit dialog saves with, and — like that dialog — only on an
 * explicit click: Start never rewrites an identity, and nothing is written
 * when the record already names what this create will run (ledger 255(e)).
 * On success the managed-agents cache carries the new id, so the lead's
 * record now matches the catalog and the notice and this action both go away.
 */
export function useCodingSessionSeatRecordModel(input: {
  lead: CodingSessionLaunchLead;
  /** The model this create will run the lead on. */
  leadModel: string | null;
  allowedModels: readonly string[];
}): NewCodingSessionSeatRecordUpdate | null {
  const { mutateAsync } = useUpdateManagedAgentMutation();
  const [state, setState] = React.useState<{
    status: "idle" | "saving" | "error";
    error: string | null;
  }>({ status: "idle", error: null });
  const agent = input.lead.kind === "agent" ? input.lead : null;
  const update = codingSessionSeatRecordModelUpdate({
    recordedModel: agent?.model ?? null,
    nextModel: input.leadModel,
    allowedModels: input.allowedModels,
  });
  const actor = agent?.actor ?? null;
  const nextModel = update?.nextModel ?? null;
  const onUpdate = React.useCallback(() => {
    if (actor === null || nextModel === null) return;
    setState({ status: "saving", error: null });
    mutateAsync({ pubkey: actor, model: nextModel }).then(
      () => setState({ status: "idle", error: null }),
      (error: unknown) =>
        setState({
          status: "error",
          error: error instanceof Error ? error.message : String(error),
        }),
    );
  }, [actor, mutateAsync, nextModel]);
  if (agent === null || update === null) return null;
  return {
    identityLabel: agent.label,
    recordedModel: update.recordedModel,
    nextModel: update.nextModel,
    state: state.status,
    error: state.error,
    onUpdate,
  };
}
