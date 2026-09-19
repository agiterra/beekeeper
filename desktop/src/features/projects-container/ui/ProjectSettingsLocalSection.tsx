import { useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronDown, CircleAlert, FolderOpen, X } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import {
  CODING_SESSION_ROLE_SUGGESTIONS,
  MAX_CODING_SESSION_ROLE_BYTES,
} from "@/features/coding-sessions/lib/codingSessionActorSeat";
import { describeWorkdirProblem } from "@/features/coding-sessions/ui/NewCodingSessionWorkdirField";
import { useCommunities } from "@/features/communities/useCommunities";
import { useIdentityQuery } from "@/shared/api/hooks";
import {
  clearCodingSessionWorkdir,
  getCodingSessionWorkdirState,
  pickCodingSessionWorkdir,
  setCodingSessionWorkdir,
  validateCodingSessionWorkdir,
  type CodingSessionWorkdirValidation,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";

import type { ProjectContainer } from "../hooks";
import { useProjectDefaultAgent } from "../lib/projectDefaultAgentStorage";

/** The datalist id the default-agent role field offers suggestions through. */
const ROLE_SUGGESTION_LIST_ID = "project-default-agent-role-suggestions";

/**
 * Same literal key `CodingSessionHireHost`, the Roles tab redirect
 * (`app/routes/index.tsx`) and `useRolePacksProject` read the workdir store
 * through. A write here that does not invalidate it is exactly what left a
 * hire host refusing `HIRE_CHECKOUT_NOT_RECORDED` for over a minute after
 * this screen wrote the project's checkout (item 167) — the hire path now
 * reads the store fresh and does not depend on this key, but every other
 * view still renders from the cache, so this still has to invalidate it.
 */
const WORKDIR_STATE_QUERY_KEY = ["coding-session-workdir-state"] as const;

/**
 * "On this computer" — the project settings that live on this machine and
 * never enter a relay event: the checkout directory sessions start in, and
 * the managed agent seated by default on new sessions. Both apply
 * immediately (no Save button), and both are editable by any viewer — they
 * describe this machine, not the shared project head.
 */
export function ProjectSettingsLocalSection({
  project,
}: {
  project: ProjectContainer;
}) {
  return (
    <div className="flex flex-col gap-5">
      <p className="text-xs text-muted-foreground">
        These apply to this computer only, take effect immediately, and are
        never published.
      </p>
      <ProjectWorkdirField project={project} />
      <ProjectDefaultAgentField project={project} />
    </div>
  );
}

/**
 * The directory this project's coding sessions start in. Same store key the
 * manage panel's "Sessions run in" row writes (`byProject[address]`).
 *
 * Exported for `ProjectSettingsLocalSection.test.mjs`: the sibling field,
 * `ProjectDefaultAgentField`, reaches `useCommunities()`, which throws
 * outside a `CommunitiesProvider` — this field alone is what item 167's fix
 * touches, so it is what the test mounts.
 */
export function ProjectWorkdirField({
  project,
}: {
  project: ProjectContainer;
}) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState("");
  const [savedPath, setSavedPath] = React.useState<string | null>(null);
  const [validation, setValidation] =
    React.useState<CodingSessionWorkdirValidation | null>(null);
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    void getCodingSessionWorkdirState()
      .then((state) => {
        if (cancelled) return;
        const path = state.byProject[project.address]?.path ?? null;
        setSavedPath(path);
        setDraft(path ?? "");
      })
      .catch(() => {
        if (!cancelled) setSavedPath(null);
      });
    return () => {
      cancelled = true;
    };
  }, [project.address]);

  React.useEffect(() => {
    const candidate = draft.trim();
    if (candidate.length === 0) {
      setValidation(null);
      return;
    }
    let cancelled = false;
    const handle = window.setTimeout(() => {
      void validateCodingSessionWorkdir(candidate)
        .then((next) => {
          if (!cancelled) setValidation(next);
        })
        .catch(() => {
          if (!cancelled) setValidation(null);
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [draft]);

  const commit = React.useCallback(
    (path: string) => {
      const trimmed = path.trim();
      if (trimmed.length === 0 || trimmed === savedPath) return;
      setBusy(true);
      void setCodingSessionWorkdir({
        scope: "project",
        key: project.address,
        path: trimmed,
      })
        .then((state) => {
          const next = state.byProject[project.address]?.path ?? trimmed;
          setSavedPath(next);
          setDraft(next);
          void queryClient.invalidateQueries({
            queryKey: WORKDIR_STATE_QUERY_KEY,
          });
        })
        .catch((error) => {
          toast.error(
            error instanceof Error
              ? error.message
              : "Could not save the working directory.",
          );
        })
        .finally(() => setBusy(false));
    },
    [project.address, queryClient, savedPath],
  );

  const handleBrowse = React.useCallback(() => {
    void pickCodingSessionWorkdir()
      .then((picked) => {
        if (picked) {
          setDraft(picked);
          commit(picked);
        }
      })
      .catch(() => {
        // A cancelled or unavailable picker leaves the text field in charge.
      });
  }, [commit]);

  const handleClear = React.useCallback(() => {
    setBusy(true);
    void clearCodingSessionWorkdir({ scope: "project", key: project.address })
      .then(() => {
        setSavedPath(null);
        setDraft("");
        void queryClient.invalidateQueries({
          queryKey: WORKDIR_STATE_QUERY_KEY,
        });
      })
      .catch((error) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Could not clear the working directory.",
        );
      })
      .finally(() => setBusy(false));
  }, [project.address, queryClient]);

  const problem = describeWorkdirProblem(draft, validation);

  return (
    <div className="flex flex-col gap-2">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="project-settings-workdir"
      >
        Repository folder
      </label>
      <div className="flex items-center gap-2">
        <Input
          aria-describedby={
            problem ? "project-settings-workdir-problem" : undefined
          }
          aria-invalid={problem !== null}
          autoComplete="off"
          className={cn(
            "font-mono text-xs placeholder:text-muted-foreground/50",
            problem && "border-destructive/60",
          )}
          data-testid="project-settings-workdir-input"
          disabled={busy}
          id="project-settings-workdir"
          onBlur={() => commit(draft)}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit(draft);
            }
          }}
          placeholder="Not set — falls back to the most recent directory"
          spellCheck={false}
          value={draft}
        />
        <Button
          data-testid="project-settings-workdir-browse"
          disabled={busy}
          onClick={handleBrowse}
          size="sm"
          type="button"
          variant="outline"
        >
          <FolderOpen />
          Browse
        </Button>
        {savedPath ? (
          <Button
            aria-label="Clear repository folder"
            data-testid="project-settings-workdir-clear"
            disabled={busy}
            onClick={handleClear}
            size="sm"
            type="button"
            variant="ghost"
          >
            <X />
          </Button>
        ) : null}
      </div>
      {problem ? (
        <p
          className="flex items-start gap-1.5 text-xs text-destructive"
          data-testid="project-settings-workdir-problem"
          id="project-settings-workdir-problem"
        >
          <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
          {problem}
        </p>
      ) : (
        <p className="text-2xs text-muted-foreground">
          New coding sessions for this project start here, ahead of any
          per-channel or most-recently-used guess.
        </p>
      )}
    </div>
  );
}

/**
 * The managed agent seated by default when creating a coding session in
 * this project. Stored per device — managed agents are this computer's
 * identities, so the default cannot follow the project to other machines.
 */
function ProjectDefaultAgentField({ project }: { project: ProjectContainer }) {
  const identityQuery = useIdentityQuery();
  const { activeCommunity } = useCommunities();
  const { defaultSeat, setDefaultSeat } = useProjectDefaultAgent(
    identityQuery.data?.pubkey?.toLowerCase(),
    activeCommunity?.relayUrl,
    project.id,
  );

  const agentsQuery = useManagedAgentsQuery();
  const agents = agentsQuery.data ?? [];

  const selected = defaultSeat
    ? (agents.find((agent) => agent.pubkey === defaultSeat.pubkey) ?? null)
    : null;
  const label = selected
    ? selected.name
    : defaultSeat
      ? "An agent this computer no longer manages"
      : "No default — sessions start unseated";

  return (
    <div
      className="flex flex-col gap-2"
      data-testid="project-settings-default-agent"
    >
      <span className="text-xs font-medium text-muted-foreground">
        Default agent <span className="font-normal">(optional)</span>
      </span>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            className="flex items-center justify-between gap-2 rounded-md border border-input px-3 py-2 text-sm transition-colors hover:bg-muted"
            data-testid="project-settings-default-agent-trigger"
            type="button"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Bot aria-hidden className="size-4 shrink-0" />
              <span className="truncate">{label}</span>
            </span>
            <ChevronDown aria-hidden className="size-3 shrink-0" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="max-h-72 overflow-y-auto">
          <DropdownMenuItem
            data-testid="project-settings-default-agent-none"
            onSelect={() => setDefaultSeat(null)}
          >
            No default — sessions start unseated
          </DropdownMenuItem>
          {agents.map((agent) => (
            <DropdownMenuItem
              data-testid={`project-settings-default-agent-${agent.pubkey}`}
              key={agent.pubkey}
              onSelect={() =>
                setDefaultSeat({
                  pubkey: agent.pubkey,
                  role: defaultSeat?.role ?? "builder",
                })
              }
            >
              <span className="flex min-w-0 flex-col">
                <span className="truncate text-sm">{agent.name}</span>
                <span className="text-2xs text-muted-foreground">
                  {agent.status === "running" ? "Running" : "Stopped"}
                </span>
              </span>
            </DropdownMenuItem>
          ))}
          {agents.length === 0 ? (
            <DropdownMenuItem disabled>
              This computer manages no agents yet
            </DropdownMenuItem>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>

      {defaultSeat ? (
        <div className="flex flex-col gap-1">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="project-settings-default-agent-role"
          >
            Role
          </label>
          <Input
            data-testid="project-settings-default-agent-role"
            id="project-settings-default-agent-role"
            list={ROLE_SUGGESTION_LIST_ID}
            maxLength={MAX_CODING_SESSION_ROLE_BYTES}
            onChange={(event) =>
              setDefaultSeat({
                pubkey: defaultSeat.pubkey,
                role: event.target.value,
              })
            }
            placeholder="lead, builder, verifier…"
            value={defaultSeat.role}
          />
          <datalist id={ROLE_SUGGESTION_LIST_ID}>
            {CODING_SESSION_ROLE_SUGGESTIONS.map((suggestion) => (
              <option key={suggestion} value={suggestion} />
            ))}
          </datalist>
        </div>
      ) : null}
      <p className="text-2xs text-muted-foreground">
        Pre-fills the agent seat when you create a coding session in this
        project. You can always change it per session.
      </p>
    </div>
  );
}
