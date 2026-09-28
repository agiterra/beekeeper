import { renderCodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
import {
  codingSessionMissionPolicyRefusedOmitted,
  codingSessionMissionPolicyView,
  type CodingSessionMissionPolicyView,
  type CodingSessionPolicyRefusedRow,
  type CodingSessionPolicyFoldResult,
} from "@/features/coding-sessions/lib/codingSessionMissionPolicyView";
import type {
  CodingSessionMissionInspectorModel,
  CodingSessionMissionInspectorSection,
  CodingSessionMissionUsageInput,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";

export type CodingSessionMissionContextProps = {
  model: CodingSessionMissionInspectorModel;
  variant: "panel" | "drawer";
  loading?: boolean;
  errorMessage?: string | null;
  onRefresh?: () => void;
  /**
   * The native fold's answer about this umbrella's kind:44245 records, or null
   * while it is unknown. Item 107 owed this surface a reader; TypeScript never
   * decides which record won.
   */
  policyFold?: CodingSessionPolicyFoldResult | null;
  /** Why the policy could not be read, when it could not. */
  policyErrorMessage?: string | null;
  /** The umbrella's founder, so their own record reads `the founder`. */
  founderPubkey?: string | null;
  /** Actor pubkey → display name, from the surface's own resolver. */
  resolveActorLabel?: (pubkey: string) => string | null;
};

/** Mission-only diagnostic context, separated from the work-state Inspector. */
export function CodingSessionMissionContext({
  errorMessage = null,
  founderPubkey = null,
  loading = false,
  model,
  onRefresh,
  policyErrorMessage = null,
  policyFold = null,
  resolveActorLabel,
  variant,
}: CodingSessionMissionContextProps) {
  const policy = codingSessionMissionPolicyView({
    fold: policyFold,
    founderPubkey,
    resolveActorLabel,
  });
  const policyRefusedOmitted =
    codingSessionMissionPolicyRefusedOmitted(policyFold);
  return (
    <aside
      aria-label="Mission context"
      className="flex h-full min-h-0 w-full flex-col bg-background text-foreground"
      data-testid="coding-session-mission-context"
      data-variant={variant}
    >
      {loading ? (
        <p
          className="border-b border-border/60 bg-muted/20 px-4 py-2 text-xs text-muted-foreground"
          role="status"
        >
          Loading signed Mission evidence…
        </p>
      ) : null}
      {errorMessage ? (
        <div
          className="border-b border-destructive/35 bg-destructive/10 px-4 py-2"
          role="alert"
        >
          <p className="text-xs text-destructive">{errorMessage}</p>
          {onRefresh ? (
            <button
              className="mt-1 rounded-sm text-2xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={onRefresh}
              type="button"
            >
              Retry signed evidence
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-8">
        {/*
          Context load, not a second roster. The Inspector's Team section owns
          seat identity, status, model, runtime and seat authority; this table
          exists for exactly one number per seat, and the seat name is here to
          say whose number it is — not to restate the roster.
        */}
        <ContextSection
          title="Context load"
          truncations={truncationsFor(model, "team")}
        >
          {model.participants.length === 0 ? (
            <EmptyCopy>No signed session seats projected.</EmptyCopy>
          ) : (
            <dl className="space-y-2">
              {model.participants.map((participant) => (
                <div
                  className="flex items-baseline justify-between gap-3"
                  key={participant.executionKey}
                >
                  <dt className="min-w-0 truncate text-xs font-medium">
                    {participant.label}
                  </dt>
                  <dd className="shrink-0 text-xs text-muted-foreground tabular-nums">
                    {renderCodingSessionContextLoad(participant.contextLoad)}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </ContextSection>

        <ContextSection
          title="Signed work context"
          truncations={truncationsFor(model, "context")}
        >
          {model.contextFacts.length === 0 ? (
            <EmptyCopy>
              No report has published assignment or Git context.
            </EmptyCopy>
          ) : (
            <dl className="space-y-2">
              {model.contextFacts.map((fact) => (
                <div key={fact.id}>
                  <dt className="break-all text-2xs font-medium text-muted-foreground">
                    {fact.label} · {visibleSourceAuthor(fact.authorLabel)}
                  </dt>
                  <dd>
                    <code className="break-all text-xs">
                      {visibleSourceIdentifier(fact.value)}
                    </code>
                  </dd>
                  <SignedSource
                    authorLabel={fact.authorLabel}
                    eventId={fact.sourceEventId}
                    rawValue={
                      isRawSourceIdentifier(fact.value) ? fact.value : undefined
                    }
                  />
                </div>
              ))}
            </dl>
          )}
        </ContextSection>

        <ContextSection title="Session policy">
          <SessionPolicy
            errorMessage={policyErrorMessage}
            omittedRefusals={policyRefusedOmitted}
            view={policy}
          />
        </ContextSection>

        <ContextSection title="Terminal usage">
          <Usage usage={model.usage} />
        </ContextSection>
      </div>
    </aside>
  );
}

/**
 * The policy in force, what it says, and every record that is not it.
 *
 * Three outcomes, three sentences (§1g) — a withdrawal is not "none". Every
 * set field prints as a row of words; **no field gets a bar, a meter or a
 * progress ring**, because `budget.turns` is the only one anything counts and
 * a bar over an uncounted limit is the same lie as a status reading Idle over
 * a disconnected provider.
 */
function SessionPolicy({
  errorMessage,
  omittedRefusals,
  view,
}: {
  errorMessage: string | null;
  omittedRefusals: number;
  view: CodingSessionMissionPolicyView;
}) {
  if (errorMessage !== null) {
    return (
      <p className="text-xs text-destructive" role="alert">
        Session policy could not be read: {errorMessage}
      </p>
    );
  }
  if (view.kind === "unknown" || view.kind === "none") {
    return (
      <div data-policy-state={view.kind} data-testid="mission-session-policy">
        <p className="text-xs text-muted-foreground">{view.sentence}</p>
        {/* F5: a refused ceiling in this channel is a fact even when nothing
            was selected — "nobody set one" and "somebody tried and was
            refused" are different answers. */}
        <RefusedRecords omitted={omittedRefusals} rows={view.refused} />
      </div>
    );
  }
  return (
    <div data-policy-state={view.kind} data-testid="mission-session-policy">
      <p className="text-xs font-medium">{view.sentence}</p>
      {view.kind === "record" ? (
        <dl className="mt-2 space-y-1">
          {view.facts.map((fact) => (
            <div
              className="flex items-baseline justify-between gap-3"
              key={fact.field}
            >
              <dt className="min-w-0 text-2xs text-muted-foreground">
                {fact.label}
                {/* The one enforced field is named, not implied. Everything
                    else is a stated intention and says so. */}
                <span className="ml-1">
                  {fact.enforced ? "· enforced" : "· stated"}
                </span>
              </dt>
              <dd
                className="shrink-0 text-xs"
                data-policy-field={fact.field}
                data-policy-enforced={fact.enforced ? "yes" : "no"}
              >
                {fact.value}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
      <SignedSource eventId={view.eventId} />
      <RefusedRecords omitted={omittedRefusals} rows={view.refused} />
    </div>
  );
}

/** Every record the fold refused, with author, code and its own reason. */
function RefusedRecords({
  omitted,
  rows,
}: {
  omitted: number;
  rows: readonly CodingSessionPolicyRefusedRow[];
}) {
  if (rows.length === 0) return null;
  return (
    <div className="mt-2">
      <p className="text-2xs font-medium text-muted-foreground">
        Refused records
      </p>
      <ul className="mt-1 space-y-1">
        {rows.map((row) => (
          <li
            className="text-2xs text-muted-foreground"
            data-testid="mission-session-policy-refused"
            key={row.eventId}
          >
            <span className="font-medium text-foreground">{row.code}</span> ·{" "}
            {row.authorLabel} · {row.reason}
          </li>
        ))}
      </ul>
      {omitted > 0 ? (
        <p className="mt-1 text-2xs text-muted-foreground" role="status">
          {omitted} more refused records not shown
        </p>
      ) : null}
    </div>
  );
}

function ContextSection({
  children,
  title,
  truncations = [],
}: {
  children: React.ReactNode;
  title: string;
  truncations?: readonly CodingSessionMissionInspectorModel["truncations"][number][];
}) {
  return (
    <section className="border-b border-border/50 py-4 last:border-b-0">
      <h3 className="mb-2 text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
        {title}
      </h3>
      {children}
      {truncations.map((truncation) => (
        <p
          className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
          key={truncation.id}
          role="status"
        >
          {truncation.notice}
        </p>
      ))}
    </section>
  );
}

function truncationsFor(
  model: CodingSessionMissionInspectorModel,
  section: CodingSessionMissionInspectorSection,
) {
  return model.truncations.filter((item) => item.section === section);
}

function EmptyCopy({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}

function Usage({ usage }: { usage: CodingSessionMissionUsageInput | null }) {
  if (!usage) return <EmptyCopy>Terminal usage not reported.</EmptyCopy>;
  const fields = [
    ["Input", formatCount(usage.inputTokens)],
    ["Output", formatCount(usage.outputTokens)],
    ["Total", formatCount(usage.totalTokens)],
    ["Tools", formatCount(usage.toolCalls)],
    [
      "Est. cost",
      usage.costUsd === null ? null : `$${usage.costUsd.toFixed(2)}`,
    ],
  ].filter((field): field is [string, string] => field[1] !== null);
  return (
    <div>
      <dl className="grid grid-cols-2 gap-x-3 gap-y-1">
        {fields.map(([label, value]) => (
          <div
            className="flex items-baseline justify-between gap-2"
            key={label}
          >
            <dt className="text-2xs text-muted-foreground">{label}</dt>
            <dd className="text-xs tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <SignedSource eventId={usage.sourceEventId} />
    </div>
  );
}

function SignedSource({
  authorLabel,
  eventId,
  rawValue,
}: {
  authorLabel?: string;
  eventId: string;
  rawValue?: string;
}) {
  return (
    <details className="mt-1 text-2xs text-muted-foreground">
      <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        Signed source
      </summary>
      <dl className="mt-1 space-y-1">
        {authorLabel && isRawSourceIdentifier(authorLabel) ? (
          <div>
            <dt className="font-medium">Author</dt>
            <dd>
              <code className="block break-all">{authorLabel}</code>
            </dd>
          </div>
        ) : null}
        <div>
          <dt className="font-medium">Event</dt>
          <dd>
            <code className="block break-all">{eventId}</code>
          </dd>
        </div>
        {rawValue ? (
          <div>
            <dt className="font-medium">Value</dt>
            <dd>
              <code className="block break-all">{rawValue}</code>
            </dd>
          </div>
        ) : null}
      </dl>
    </details>
  );
}

function visibleSourceAuthor(authorLabel: string): string {
  return visibleSourceIdentifier(authorLabel);
}

function visibleSourceIdentifier(value: string): string {
  return isRawSourceIdentifier(value)
    ? `${value.slice(0, 8)}…${value.slice(-6)}`
    : value;
}

function isRawSourceIdentifier(value: string): boolean {
  return /^[0-9a-f]{64}$/i.test(value);
}

function formatCount(value: number | null): string | null {
  return value === null ? null : value.toLocaleString();
}
