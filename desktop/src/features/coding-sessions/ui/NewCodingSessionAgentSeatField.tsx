import { Bot, ChevronDown } from "lucide-react";

import {
  CODING_SESSION_ROLE_SUGGESTIONS,
  codingSessionSeatPackNotice,
  codingSessionSeatRoleNotice,
  MAX_CODING_SESSION_ROLE_BYTES,
} from "@/features/coding-sessions/lib/codingSessionActorSeat";
import type { CodingSessionSeatAgent } from "@/features/coding-sessions/lib/codingSessionSeatAgent";
import { useCodingSessionPackStatusPreview } from "@/features/coding-sessions/lib/useCodingSessionPackStatusPreview";
import { Input } from "@/shared/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { NewCodingSessionPackPreviewLine } from "./NewCodingSessionPackPreviewLine";

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
 * otherwise would produce a create the provider refuses. The caller narrows it
 * to the agents a seat in this session may hold (its project's, or agents in
 * no project) and says who was left out in `scopeSentence`.
 *
 * `agents` is deliberately structural ({@link CodingSessionSeatAgent}) rather
 * than the full `ManagedAgent`: the two disclosure lines below read optional
 * fields, and an `undefined` field must render nothing rather than a comfortable
 * default.
 */
export function NewCodingSessionAgentSeatField({
  actor,
  agents,
  disabled = false,
  error = null,
  onActorChange,
  onRoleChange,
  projectRef = null,
  role,
  roleLocked = false,
  scopeSentence = null,
}: {
  /** Pubkey of the seated agent, or null for an unseated execution. */
  actor: string | null;
  agents: readonly CodingSessionSeatAgent[];
  disabled?: boolean;
  /** Validation copy for the seat as a whole. */
  error?: string | null;
  onActorChange: (actor: string | null) => void;
  onRoleChange: (role: string) => void;
  /**
   * The project this session belongs to, when the caller knows one
   * (LANE-L23) — feeds the pack preview below the role box. `null` (the
   * default, and every pre-L23 caller's implicit value) shows no preview at
   * all, byte-identical to before this prop existed.
   */
  projectRef?: string | null;
  role: string;
  /** The chosen agent's primary role fixes the role; the box is read-only. */
  roleLocked?: boolean;
  /** Who is not offered and why, or that the session's project is unknown. */
  scopeSentence?: string | null;
}) {
  const selected = agents.find((agent) => agent.pubkey === actor) ?? null;
  // Both lines are conditional on the backend having answered: a build with no
  // home-role/pack knowledge renders exactly the field it rendered before.
  const roleNotice = codingSessionSeatRoleNotice({ agent: selected, role });
  const packNotice = codingSessionSeatPackNotice(selected);
  // LANE-L23: a *different* pack notion from `packNotice` above — that one is
  // "does this computer have a role-pack installer for this agent at all";
  // this is "what would the host actually stage from the project's 30624
  // source". Both can be true or false independently, so both render.
  const packPreview = useCodingSessionPackStatusPreview({
    agentPubkey: actor,
    projectRef,
    role,
  });
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
              {scopeSentence
                ? "No agent on this computer can be seated in this session"
                : "This computer manages no agents yet"}
            </DropdownMenuItem>
          ) : null}
        </DropdownMenuContent>
      </DropdownMenu>
      {scopeSentence ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-seat-scope"
        >
          {scopeSentence}
        </p>
      ) : null}

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
            readOnly={roleLocked}
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
          {roleLocked ? (
            <p
              className="text-2xs text-muted-foreground"
              data-testid="new-coding-session-seat-role-locked"
            >
              A new seat takes the agent's primary role, so this agent is seated
              as {role}.
            </p>
          ) : null}
          {roleNotice ? (
            <p
              className={
                roleNotice.tone === "warn"
                  ? "text-2xs text-amber-600 dark:text-amber-500"
                  : "text-2xs text-muted-foreground"
              }
              data-testid="new-coding-session-seat-role-notice"
            >
              {roleNotice.message}
            </p>
          ) : null}
          {packNotice ? (
            <p
              className="text-2xs text-amber-600 dark:text-amber-500"
              data-testid="new-coding-session-seat-pack"
            >
              {packNotice}
            </p>
          ) : null}
          <NewCodingSessionPackPreviewLine preview={packPreview} />
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
