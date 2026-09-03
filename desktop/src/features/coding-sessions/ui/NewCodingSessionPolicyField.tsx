import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import {
  CODING_SESSION_POLICY_ATTENTIONS,
  CODING_SESSION_POLICY_CONTEXT_TIERS,
  CODING_SESSION_POLICY_IRREVERSIBLE_ACTS,
  CODING_SESSION_POLICY_POSTURES,
  CODING_SESSION_POLICY_STATED_NOT_ENFORCED,
  codingSessionPolicyDraftSetsAnything,
  type CodingSessionPolicyDraft,
  type CodingSessionPolicyIrreversibleAct,
} from "../lib/codingSessionPolicy";

/** Read a positive integer, or null for "not set". Never zero-by-accident. */
function readPositiveInteger(raw: string): number | null | undefined {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  const parsed = Number.parseInt(trimmed, 10);
  // `undefined` means "keep what you had": a half-typed number must not be
  // able to clear a limit the founder already set, and a zero is refused by
  // the signer anyway — zero and "no limit" would be one record read two ways.
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
}

/**
 * Posture, budget, attention, irreversible acts and stop — the 44245 record.
 *
 * All of it is *stated*, and the disclosure under it says so in the same
 * words `docs/design/portable-team-loop/POLICY.md` §4 uses: until a consumer
 * exists, a published policy is an intention, not a limit. That sentence is
 * the price of showing a budget field at all; a number in a box with nothing
 * counting it is a promise the app cannot keep, and it would be believed.
 *
 * Nothing here validates a bound. The draft goes to `buzz-core` through the
 * native boundary, which refuses a zero limit, an unknown word, an empty
 * collection or a per-seat ceiling above the session's — each by name.
 */
export function NewCodingSessionPolicyField({
  disabled = false,
  draft,
  onDraftChange,
}: {
  disabled?: boolean;
  draft: CodingSessionPolicyDraft;
  onDraftChange: (next: CodingSessionPolicyDraft) => void;
}) {
  const patch = (next: Partial<CodingSessionPolicyDraft>) =>
    onDraftChange({ ...draft, ...next });
  const setsAnything = codingSessionPolicyDraftSetsAnything(draft);
  return (
    <details
      className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
      data-testid="new-coding-session-policy"
    >
      <summary className="cursor-pointer text-xs font-medium text-muted-foreground">
        Posture, budget and limits
        {setsAnything ? " — set" : " — none set"}
      </summary>
      <div className="mt-3 flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-posture"
          >
            Posture
          </label>
          <select
            className="h-8 rounded-md border border-input bg-transparent px-2 text-sm disabled:opacity-50"
            data-testid="new-coding-session-policy-posture"
            disabled={disabled}
            id="policy-posture"
            onChange={(event) =>
              patch({
                posture:
                  event.target.value === ""
                    ? null
                    : (event.target
                        .value as CodingSessionPolicyDraft["posture"]),
              })
            }
            value={draft.posture ?? ""}
          >
            <option value="">Not set</option>
            {CODING_SESSION_POLICY_POSTURES.map((word) => (
              <option key={word} value={word}>
                {word}
              </option>
            ))}
          </select>

          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-attention"
          >
            Tell me about
          </label>
          <select
            className="h-8 rounded-md border border-input bg-transparent px-2 text-sm disabled:opacity-50"
            data-testid="new-coding-session-policy-attention"
            disabled={disabled}
            id="policy-attention"
            onChange={(event) =>
              patch({
                attention:
                  event.target.value === ""
                    ? null
                    : (event.target
                        .value as CodingSessionPolicyDraft["attention"]),
              })
            }
            value={draft.attention ?? ""}
          >
            <option value="">Not set</option>
            {CODING_SESSION_POLICY_ATTENTIONS.map((word) => (
              <option key={word} value={word}>
                {word}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-turns"
          >
            Turns
          </label>
          <Input
            className="w-28"
            data-testid="new-coding-session-policy-turns"
            disabled={disabled}
            id="policy-turns"
            inputMode="numeric"
            onChange={(event) => {
              const read = readPositiveInteger(event.target.value);
              if (read !== undefined) patch({ turns: read });
            }}
            placeholder="—"
            value={draft.turns === null ? "" : String(draft.turns)}
          />
          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-timebox"
          >
            Stop after (hours)
          </label>
          <Input
            className="w-28"
            data-testid="new-coding-session-policy-timebox"
            disabled={disabled}
            id="policy-timebox"
            inputMode="numeric"
            onChange={(event) => {
              const read = readPositiveInteger(event.target.value);
              if (read !== undefined) {
                patch({ timeBoxSecs: read === null ? null : read * 3600 });
              }
            }}
            placeholder="—"
            value={
              draft.timeBoxSecs === null
                ? ""
                : String(Math.round(draft.timeBoxSecs / 3600))
            }
          />
          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-context"
          >
            Context
          </label>
          <select
            className="h-8 rounded-md border border-input bg-transparent px-2 text-sm disabled:opacity-50"
            data-testid="new-coding-session-policy-context"
            disabled={disabled}
            id="policy-context"
            onChange={(event) =>
              patch({
                contextTier:
                  event.target.value === ""
                    ? null
                    : (event.target
                        .value as CodingSessionPolicyDraft["contextTier"]),
              })
            }
            value={draft.contextTier ?? ""}
          >
            <option value="">Not set</option>
            {CODING_SESSION_POLICY_CONTEXT_TIERS.map((word) => (
              <option key={word} value={word}>
                {word}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-col gap-1">
          <span className="text-2xs text-muted-foreground">
            Needs your word before it happens
          </span>
          <div className="flex flex-wrap gap-3">
            {CODING_SESSION_POLICY_IRREVERSIBLE_ACTS.map((act) => (
              <label
                className="flex items-center gap-2 text-sm"
                htmlFor={`policy-irreversible-${act}`}
                key={act}
              >
                <Checkbox
                  checked={draft.irreversible.includes(act)}
                  data-testid={`new-coding-session-policy-irreversible-${act}`}
                  disabled={disabled}
                  id={`policy-irreversible-${act}`}
                  onCheckedChange={(checked) =>
                    patch({
                      irreversible:
                        checked === true
                          ? [...draft.irreversible, act]
                          : draft.irreversible.filter(
                              (entry: CodingSessionPolicyIrreversibleAct) =>
                                entry !== act,
                            ),
                    })
                  }
                />
                {act}
              </label>
            ))}
          </div>
        </div>

        {/* The one gate field the 44244 fold actually counts, and until this
            lane the only enforced field the form could not set: the
            disclosure below already named it (finding 39). Three-way, not a
            checkbox — `verifierRequired` is a nullable boolean on the wire,
            and a two-state control would publish `false` for a founder who
            never touched it, which is a stated policy nobody chose. */}
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <label
              className="text-2xs text-muted-foreground"
              htmlFor="policy-verifier-required"
            >
              Completing this mission
            </label>
            <select
              className="h-8 rounded-md border border-input bg-transparent px-2 text-sm disabled:opacity-50"
              data-testid="new-coding-session-policy-verifier-required"
              disabled={disabled}
              id="policy-verifier-required"
              onChange={(event) =>
                patch({
                  verifierRequired:
                    event.target.value === "unset"
                      ? null
                      : event.target.value === "true",
                })
              }
              value={
                draft.verifierRequired === null
                  ? "unset"
                  : String(draft.verifierRequired)
              }
            >
              <option value="unset">Not set</option>
              <option value="true">
                A verifier must clear every settled report
              </option>
              <option value="false">
                The lead&rsquo;s approval settles it
              </option>
            </select>
          </div>
          {/* What this switch decides, in the words of the rule that reads it.
              It is NOT the push gate: `verdict_admission_fold_context` passes
              `verifier_required: false` and says no refusal there may be read
              as "no verifier is required". A label that promised otherwise
              would be a control lying about what it enforces. */}
          <p className="text-2xs text-muted-foreground">
            Set, a <code>mission.completed</code> is refused while any settled
            report has no verifier&rsquo;s ruling. It does not decide who may
            push: on a <code>require-verdict</code> ref the repository&rsquo;s
            own rule admits a founder&rsquo;s push outright, and anyone
            else&rsquo;s only behind a verifier&rsquo;s verdict.
          </p>
        </div>

        <div className="flex items-center gap-2">
          <label
            className="text-2xs text-muted-foreground"
            htmlFor="policy-milestone"
          >
            Stop on
          </label>
          <Input
            data-testid="new-coding-session-policy-milestone"
            disabled={disabled}
            id="policy-milestone"
            onChange={(event) =>
              patch({
                onMilestone:
                  event.target.value.trim() === "" ? null : event.target.value,
              })
            }
            placeholder="The milestone that ends this mission"
            value={draft.onMilestone ?? ""}
          />
        </div>

        {/* Never optional, and never softened: a budget nothing counts is a
            promise this app cannot keep, and a person will believe it. */}
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-policy-disclosure"
        >
          {CODING_SESSION_POLICY_STATED_NOT_ENFORCED}
        </p>
      </div>
    </details>
  );
}
