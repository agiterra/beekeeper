import { Bot, ChevronDown } from "lucide-react";

import {
  CODING_SESSION_ROLE_SUGGESTIONS,
  MAX_CODING_SESSION_ROLE_BYTES,
} from "@/features/coding-sessions/lib/codingSessionActorSeat";
import type { ManagedAgent } from "@/shared/api/types";
import { Input } from "@/shared/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

/** The datalist id the role field offers its suggestions through. */
const ROLE_SUGGESTION_LIST_ID = "coding-session-role-suggestions";

/**
 * "Seat an agent": who runs this execution, and as what.
 *
 * The provider and model pickers answer *what runs*; this answers *who is
 * running it*. Leaving it empty is the ordinary case and the default — the
 * session is yours, created by you, with no agent identity injected — so the
 * field is one line until an agent is chosen, and the role only appears once
 * there is a seat to label.
 *
 * The agent list is this computer's managed agents, because those are the
 * only identities whose keys the provider can resolve host-locally (D6). An
 * agent on someone else's machine cannot be seated here, and pretending
 * otherwise would produce a create the provider refuses.
 */
export function NewCodingSessionAgentSeatField({
  actor,
  agents,
  disabled = false,
  error = null,
  onActorChange,
  onRoleChange,
  role,
}: {
  /** Pubkey of the seated agent, or null for an unseated execution. */
  actor: string | null;
  agents: readonly ManagedAgent[];
  disabled?: boolean;
  /** Validation copy for the seat as a whole. */
  error?: string | null;
  onActorChange: (actor: string | null) => void;
  onRoleChange: (role: string) => void;
  role: string;
}) {
  const selected = agents.find((agent) => agent.pubkey === actor) ?? null;
  const label = selected
    ? selected.name
    : actor
      ? "An agent this computer no longer manages"
      : "No agent — you run this session";

  return (
    <div className="flex flex-col gap-2" data-testid="new-coding-session-seat">
      <span className="text-xs font-medium text-muted-foreground">
        Seat an agent <span className="font-normal">(optional)</span>
      </span>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            className="flex items-center justify-between gap-2 rounded-md border border-input px-3 py-2 text-sm transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
            data-testid="new-coding-session-seat-agent"
            disabled={disabled}
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
            data-testid="new-coding-session-seat-agent-none"
            onSelect={() => onActorChange(null)}
          >
            No agent — you run this session
          </DropdownMenuItem>
          {agents.map((agent) => (
            <DropdownMenuItem
              data-testid={`new-coding-session-seat-agent-${agent.pubkey}`}
              key={agent.pubkey}
              onSelect={() => onActorChange(agent.pubkey)}
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

      {actor !== null ? (
        <div className="flex flex-col gap-1">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-seat-role"
          >
            Role
          </label>
          <Input
            data-testid="new-coding-session-seat-role"
            disabled={disabled}
            id="coding-session-seat-role"
            list={ROLE_SUGGESTION_LIST_ID}
            maxLength={MAX_CODING_SESSION_ROLE_BYTES}
            onChange={(event) => onRoleChange(event.target.value)}
            placeholder="lead, builder, verifier…"
            value={role}
          />
          <datalist id={ROLE_SUGGESTION_LIST_ID}>
            {CODING_SESSION_ROLE_SUGGESTIONS.map((suggestion) => (
              <option key={suggestion} value={suggestion} />
            ))}
          </datalist>
          <p className="text-2xs text-muted-foreground">
            The seat holds this agent's own identity on the relay and is added
            to the channel, so its work is signed as itself.
          </p>
        </div>
      ) : null}

      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="new-coding-session-seat-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}
