import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { truncatePubkey } from "@/shared/lib/pubkey";

/** One identity or runtime the lead may hire from. */
export type NewCodingSessionBenchOption = {
  /** Pubkey for an identity, provider alias for a runtime. */
  value: string;
  label: string;
  /** Extra fact shown beside the label — a role, a runtime name. */
  detail: string | null;
};

/**
 * The bench: who the lead may hire, and on what.
 *
 * Not a roster. Nothing here is created by the launch — the lead hires each
 * seat itself, one `bee sessions hire` at a time, once it knows what the work
 * is. The old Team tab listed four roles beside a button called "Launch team"
 * and started one agent; naming this "bench" and publishing it as
 * `bench.identities` / `bench.providers` on the session's policy is the same
 * fact told honestly, and it is on the wire rather than in a first-turn
 * paragraph.
 *
 * Identities are shown with the canonical short pubkey beside the name for the
 * same reason the lead is: two identities can share a display name, and the
 * bench is what the lead will pick from without a person in the room.
 */
export function NewCodingSessionBenchField({
  challengerRate,
  disabled = false,
  identities,
  onChallengerRateChange,
  onToggleIdentity,
  onToggleProvider,
  providers,
  selectedIdentities,
  selectedProviders,
}: {
  /** 0–1, or null when the founder set none. */
  challengerRate: number | null;
  disabled?: boolean;
  identities: readonly NewCodingSessionBenchOption[];
  onChallengerRateChange: (rate: number | null) => void;
  onToggleIdentity: (value: string, selected: boolean) => void;
  onToggleProvider: (value: string, selected: boolean) => void;
  providers: readonly NewCodingSessionBenchOption[];
  selectedIdentities: readonly string[];
  selectedProviders: readonly string[];
}) {
  const percent =
    challengerRate === null ? "" : String(Math.round(challengerRate * 100));
  return (
    <div className="flex flex-col gap-2" data-testid="new-coding-session-bench">
      <span className="text-xs font-medium text-muted-foreground">Bench</span>
      <p className="text-2xs text-muted-foreground">
        Who the lead may hire, and on which runtimes. Nobody here is seated by
        this launch — the lead hires them with <code>bee sessions hire</code>{" "}
        once it knows what the work is.
      </p>

      {identities.length === 0 ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-bench-empty"
        >
          No identity on this computer carries a role, so there is nobody to
          bench. The lead will work alone.
        </p>
      ) : (
        <ul className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5">
          {identities.map((option) => (
            <li className="flex items-center gap-2 text-sm" key={option.value}>
              <Checkbox
                checked={selectedIdentities.includes(option.value)}
                data-testid={`new-coding-session-bench-identity-${option.value}`}
                disabled={disabled}
                id={`bench-identity-${option.value}`}
                onCheckedChange={(checked) =>
                  onToggleIdentity(option.value, checked === true)
                }
              />
              <label
                className="flex min-w-0 items-baseline gap-2"
                htmlFor={`bench-identity-${option.value}`}
              >
                <span className="truncate">{option.label}</span>
                <span className="text-2xs text-muted-foreground">
                  {truncatePubkey(option.value)}
                  {option.detail ? ` · ${option.detail}` : ""}
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}

      {providers.length === 0 ? null : (
        <ul className="flex flex-wrap gap-3 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5">
          {providers.map((option) => (
            <li className="flex items-center gap-2 text-sm" key={option.value}>
              <Checkbox
                checked={selectedProviders.includes(option.value)}
                data-testid={`new-coding-session-bench-provider-${option.value}`}
                disabled={disabled}
                id={`bench-provider-${option.value}`}
                onCheckedChange={(checked) =>
                  onToggleProvider(option.value, checked === true)
                }
              />
              <label htmlFor={`bench-provider-${option.value}`}>
                {option.label}
              </label>
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center gap-2">
        <label
          className="text-2xs text-muted-foreground"
          htmlFor="coding-session-challenger-rate"
        >
          Challenger rate
        </label>
        <Input
          className="w-24"
          data-testid="new-coding-session-bench-challenger"
          disabled={disabled}
          id="coding-session-challenger-rate"
          inputMode="numeric"
          onChange={(event) => {
            const raw = event.target.value.trim();
            if (raw === "") {
              onChallengerRateChange(null);
              return;
            }
            const parsed = Number.parseInt(raw, 10);
            // An unreadable entry is not a rate of zero. Zero is a policy the
            // founder may deliberately set, so a half-typed value must not be
            // able to mean it.
            if (!Number.isFinite(parsed)) return;
            onChallengerRateChange(Math.min(100, Math.max(0, parsed)) / 100);
          }}
          placeholder="—"
          value={percent}
        />
        <span className="text-2xs text-muted-foreground">
          % of jobs given to a second opinion. Blank sets none.
        </span>
      </div>
    </div>
  );
}
