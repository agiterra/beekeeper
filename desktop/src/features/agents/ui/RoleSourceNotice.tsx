import { useMutation, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  managedAgentsQueryKey,
  personasQueryKey,
} from "@/features/agents/hooks";
import {
  reapplyRoleSource,
  ROLE_INSTRUCTIONS_EDITED,
  type ReapplyRoleSourceResponse,
} from "@/shared/api/tauriTeams";
import { Button } from "@/shared/ui/button";

export const ROLE_SOURCE_NOTICE_TESTID = "agent-role-source";
export const ROLE_SOURCE_REAPPLY_TESTID = "agent-role-source-reapply";
export const ROLE_SOURCE_DISCARD_TESTID = "agent-role-source-discard";

/** What each pin state means to the person reading the agent's detail. */
export const ROLE_SOURCE_STATE_COPY = {
  missing: "This agent was never pinned to its role pack.",
  diverged: "This agent's shared role changed since it was pinned.",
} as const;

function describeResult(result: ReapplyRoleSourceResponse): string {
  const moved = [
    result.moved.runtime ? `runtime ${result.moved.runtime}` : null,
    result.moved.model ? `model ${result.moved.model}` : null,
    result.moved.provider ? `provider ${result.moved.provider}` : null,
  ].filter(Boolean);
  const parts = ["Role re-applied from its pack."];
  if (moved.length > 0) {
    parts.push(
      result.applyError
        ? `Not applied to this agent: ${moved.join(", ")} (${result.applyError}). Set them in Provider & settings.`
        : `Moved to this agent: ${moved.join(", ")}.`,
    );
  }
  if (result.cleared.length > 0) {
    parts.push(`Cleared from the shared role: ${result.cleared.join(", ")}.`);
  }
  if (result.replaced.length > 0) {
    parts.push(`Replaced: ${result.replaced.join(", ")}.`);
  }
  parts.push(`Recovery copy: ${result.recoveryCopy}`);
  return parts.join(" ");
}

/**
 * A role agent whose pin is missing or diverged, and the one supported way
 * back: re-apply its pack (ledger 401). Edited instructions need a second,
 * explicit confirm; the backend refuses to replace them otherwise.
 */
export function RoleSourceNotice({
  pubkey,
  state,
}: {
  pubkey: string;
  state: "missing" | "current" | "diverged";
}) {
  const queryClient = useQueryClient();
  const [outcome, setOutcome] = React.useState<string | null>(null);
  const [instructionsEdited, setInstructionsEdited] = React.useState<
    string | null
  >(null);
  const mutation = useMutation({
    mutationFn: (discard: boolean) => reapplyRoleSource(pubkey, discard),
    onSuccess: (result) => {
      setInstructionsEdited(null);
      setOutcome(describeResult(result));
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes(ROLE_INSTRUCTIONS_EDITED)) {
        setInstructionsEdited(
          message.slice(message.indexOf(":") + 1).trim() || message,
        );
      } else {
        setOutcome(message);
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: managedAgentsQueryKey });
      void queryClient.invalidateQueries({ queryKey: personasQueryKey });
    },
  });

  // A current pin needs nothing, but a re-apply that just made it current
  // keeps its outcome on screen: it may name settings it could not apply.
  if (state === "current" && !outcome && !instructionsEdited) return null;
  return (
    <div
      className="space-y-2 rounded-lg border border-border/70 bg-muted/30 px-3 py-2.5 text-sm"
      data-testid={ROLE_SOURCE_NOTICE_TESTID}
      data-state={state}
    >
      {state === "current" ? null : <p>{ROLE_SOURCE_STATE_COPY[state]}</p>}
      {instructionsEdited ? (
        <>
          <p>{instructionsEdited}</p>
          <Button
            data-testid={ROLE_SOURCE_DISCARD_TESTID}
            disabled={mutation.isPending}
            onClick={() => mutation.mutate(true)}
            size="sm"
            variant="destructive"
          >
            Replace the edits and re-apply
          </Button>
        </>
      ) : state === "current" ? null : (
        <Button
          data-testid={ROLE_SOURCE_REAPPLY_TESTID}
          disabled={mutation.isPending}
          onClick={() => mutation.mutate(false)}
          size="sm"
          variant="outline"
        >
          Re-apply role source
        </Button>
      )}
      {outcome ? <p className="text-muted-foreground">{outcome}</p> : null}
    </div>
  );
}
