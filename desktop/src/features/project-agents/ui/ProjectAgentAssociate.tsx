import * as React from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { managedAgentsQueryKey } from "@/features/agents/hooks";
import { associateManagedAgentWithProject } from "@/shared/api/tauriProjectAgents";
import { Button } from "@/shared/ui/button";

import type { ProjectAgentAssociateAccess } from "../lib/publishedProjectAgents";
import {
  ASSOCIATE_CANCEL,
  ASSOCIATE_CONFIRM,
  ASSOCIATE_PENDING,
  associateConfirmText,
  associateLabel,
} from "./projectAgentsCopy";

function causeMessage(cause: unknown): string {
  if (cause instanceof Error) return cause.message;
  return String(cause);
}

/**
 * "Associate with <Project>" for a local agent that belongs to no project.
 *
 * Explicit and permanent: it asks first, says what does and does not change,
 * and shows a refusal in native's own words. Viewer access is decided by the
 * caller; a denied viewer sees the control disabled with the reason beside it.
 */
export function ProjectAgentAssociate({
  access,
  name,
  projectName,
  projectRef,
  pubkey,
  role,
}: {
  access: ProjectAgentAssociateAccess;
  name: string;
  projectName: string;
  projectRef: string;
  pubkey: string;
  role: string;
}) {
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: associateManagedAgentWithProject,
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: managedAgentsQueryKey }),
        queryClient.invalidateQueries({
          queryKey: ["project-published-agents"],
        }),
      ]);
    },
  });
  const mutateAsync = mutation.mutateAsync;
  const [confirming, setConfirming] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  // The association landed but the roster op did not: said here, in the
  // host's words, because the agent is now a project agent that the relay
  // will still refuse to let write.
  const [rosterError, setRosterError] = React.useState<string | null>(null);
  const denied = access.kind === "denied" ? access.reason : null;

  async function confirm() {
    setError(null);
    setRosterError(null);
    try {
      const associated = await mutateAsync({ pubkey, projectRef });
      setRosterError(associated.rosterError);
      setConfirming(false);
    } catch (cause) {
      setError(causeMessage(cause));
    }
  }

  return (
    <div
      className="flex min-w-0 flex-col gap-1"
      data-testid="project-agent-associate"
    >
      {confirming ? (
        <fieldset
          className="flex min-w-0 flex-col gap-2 rounded-md border border-border px-2 py-2"
          data-testid="project-agent-associate-confirm"
        >
          <p className="text-xs text-foreground">
            {associateConfirmText(name, projectName, role)}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              data-testid="project-agent-associate-yes"
              disabled={mutation.isPending || denied !== null}
              onClick={() => {
                void confirm();
              }}
              size="sm"
              type="button"
            >
              {mutation.isPending ? ASSOCIATE_PENDING : ASSOCIATE_CONFIRM}
            </Button>
            <Button
              data-testid="project-agent-associate-cancel"
              disabled={mutation.isPending}
              onClick={() => {
                setConfirming(false);
                setError(null);
              }}
              size="sm"
              type="button"
              variant="ghost"
            >
              {ASSOCIATE_CANCEL}
            </Button>
          </div>
        </fieldset>
      ) : (
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <Button
            className="h-auto min-h-7 whitespace-normal text-left"
            data-testid="project-agent-associate-button"
            disabled={denied !== null}
            onClick={() => setConfirming(true)}
            size="sm"
            title={denied ?? undefined}
            type="button"
            variant="outline"
          >
            {associateLabel(projectName)}
          </Button>
          {denied ? (
            <span
              className="min-w-0 text-2xs text-muted-foreground"
              data-testid="project-agent-associate-denied"
            >
              {denied}
            </span>
          ) : null}
        </div>
      )}
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-agent-associate-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      {rosterError ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-agent-associate-roster-error"
          role="alert"
        >
          Associated, but not added to the project roster: {rosterError}. The
          agent cannot write the project until it is.
        </p>
      ) : null}
    </div>
  );
}
