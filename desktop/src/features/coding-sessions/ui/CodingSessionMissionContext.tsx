import { renderCodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
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
};

/** Mission-only diagnostic context, separated from the work-state Inspector. */
export function CodingSessionMissionContext({
  errorMessage = null,
  loading = false,
  model,
  onRefresh,
  variant,
}: CodingSessionMissionContextProps) {
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

        <ContextSection title="Terminal usage">
          <Usage usage={model.usage} />
        </ContextSection>
      </div>
    </aside>
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
    ["Cost", usage.costUsd === null ? null : `$${usage.costUsd.toFixed(2)}`],
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
