import { ShieldCheck } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { Switch } from "@/shared/ui/switch";
import {
  codingSessionGovernedLockReason,
  codingSessionLaunchIsGoverned,
  codingSessionLeadIdentityLine,
  type CodingSessionLaunchLead,
} from "../lib/codingSessionLaunchForm";

/** A managed agent this computer could seat as the lead. */
export type NewCodingSessionLeadCandidate = {
  pubkey: string;
  name: string;
  /**
   * The role this identity *is*, read from its pack. Never typed by hand:
   * free-text role names were item 103's finding 12, and a role slug the
   * relay refuses is a launch that dies after the genesis is already signed.
   */
  role: string | null;
  model: string | null;
  hasRolePack?: boolean;
};

/**
 * Who leads this session — you, or one seated agent.
 *
 * The whole reason this control exists is that the answer decides everything
 * below it: an agent lead means a governed session, a genesis, an authority
 * chain and a bench; leading it yourself means one execution and none of
 * that. The old dialog asked the same question as two tabs, which let the two
 * halves drift until the Team tab had no provider control at all.
 *
 * The identity line carries the whole name **and** the canonical short pubkey,
 * because two managed agents can be called Keystone and this is the screen
 * where picking the wrong one costs a session.
 */
export function NewCodingSessionLeadField({
  candidates,
  disabled = false,
  lead,
  onLeadChange,
}: {
  candidates: readonly NewCodingSessionLeadCandidate[];
  disabled?: boolean;
  lead: CodingSessionLaunchLead;
  onLeadChange: (actor: string | null) => void;
}) {
  const governed = codingSessionLaunchIsGoverned(lead);
  const lockReason = codingSessionGovernedLockReason(lead);
  const seatable = candidates.filter((candidate) => candidate.role !== null);
  return (
    <div className="flex flex-col gap-2" data-testid="new-coding-session-lead">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-lead"
      >
        Who leads
      </label>
      <select
        className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
        data-testid="new-coding-session-lead-select"
        disabled={disabled}
        id="coding-session-lead"
        onChange={(event) =>
          onLeadChange(event.target.value === "" ? null : event.target.value)
        }
        value={lead.kind === "agent" ? lead.actor : ""}
      >
        <option value="">You</option>
        {seatable.map((candidate) => (
          <option key={candidate.pubkey} value={candidate.pubkey}>
            {candidate.name} · {candidate.role}
          </option>
        ))}
      </select>
      <p
        className="text-2xs text-muted-foreground"
        data-testid="new-coding-session-lead-identity"
      >
        {codingSessionLeadIdentityLine(lead)}
        {lead.kind === "agent" ? ` · ${lead.role}` : ""}
        {lead.kind === "agent"
          ? lead.model
            ? ` · ${lead.model}`
            : " · no model of its own"
          : ""}
      </p>
      {seatable.length === 0 ? (
        <p className="text-2xs text-muted-foreground">
          No identity on this computer carries a role, so there is nobody to
          lead but you. Install role packs from the project's{" "}
          <code>personas/roles</code> to change that.
        </p>
      ) : null}

      <div
        className={cn(
          "flex items-start gap-3 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5",
        )}
        data-testid="new-coding-session-governed"
      >
        <ShieldCheck
          className={cn(
            "mt-0.5 size-4 shrink-0",
            governed ? "text-foreground" : "text-muted-foreground",
          )}
        />
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-sm font-medium">
            {governed ? "Governed" : "Not governed"}
          </span>
          {/* Derived, never free: the switch shows the state and says who
              decided it, rather than pretending to be a decision of its own. */}
          <span
            className="text-2xs text-muted-foreground"
            data-testid="new-coding-session-governed-reason"
          >
            {lockReason}
          </span>
        </div>
        <Switch
          aria-label="Governed session"
          checked={governed}
          className="ml-auto mt-0.5"
          data-testid="new-coding-session-governed-switch"
          disabled
        />
      </div>
    </div>
  );
}

/** Turn the picked pubkey into the lead the rest of the form reads. */
export function resolveNewCodingSessionLead(input: {
  actor: string | null;
  candidates: readonly NewCodingSessionLeadCandidate[];
  /** How the founder is named on their own screen. */
  youLabel: string;
}): CodingSessionLaunchLead {
  const candidate =
    input.actor === null
      ? null
      : (input.candidates.find(
          (entry) => entry.pubkey === input.actor && entry.role !== null,
        ) ?? null);
  if (!candidate || candidate.role === null) {
    return { kind: "you", label: input.youLabel };
  }
  return {
    kind: "agent",
    actor: candidate.pubkey.toLowerCase(),
    label: candidate.name,
    role: candidate.role,
    model: candidate.model,
    ...(candidate.hasRolePack === undefined
      ? {}
      : { hasRolePack: candidate.hasRolePack }),
  };
}
