import type { CodingSessionUmbrellaTurnBlock } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type { CodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";

/** Signed-stream provenance shown only in Trace; never a scroll shortcut. */
export function CodingSessionMissionTraceDetails({
  block,
  record,
}: {
  block: CodingSessionUmbrellaTurnBlock;
  record: CodingSessionCatalogRecord | null;
}) {
  return (
    <details
      className="mb-2 rounded-lg border border-border/60 bg-muted/15 px-3 py-2"
      data-testid="coding-session-mission-trace-detail"
      open
    >
      <summary className="cursor-pointer text-xs font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        Signed execution provenance · {block.items.length} raw projected item
        {block.items.length === 1 ? "" : "s"}
      </summary>
      <dl className="mt-2 grid grid-cols-[auto,minmax(0,1fr)] gap-x-3 gap-y-1 text-2xs">
        <dt className="text-muted-foreground">Provider signer</dt>
        <dd className="truncate font-mono" title={block.signerPubkey}>
          {block.signerPubkey || "Unknown"}
        </dd>
        <dt className="text-muted-foreground">Execution</dt>
        <dd className="break-all font-mono">{block.executionKey}</dd>
        <dt className="text-muted-foreground">Generation</dt>
        <dd>{block.generation}</dd>
        <dt className="text-muted-foreground">Turn</dt>
        <dd className="break-all font-mono">{block.turnId ?? "Unscoped"}</dd>
        <dt className="text-muted-foreground">Runtime / model</dt>
        <dd>
          {[record?.runtime, record?.model].filter(Boolean).join(" · ") ||
            "Not reported"}
        </dd>
      </dl>
      <ol className="mt-2 space-y-1 border-t border-border/50 pt-2">
        {block.items.map((item) => (
          <li className="flex min-w-0 gap-2 text-2xs" key={item.id}>
            <time className="shrink-0 text-muted-foreground">
              {item.timestamp}
            </time>
            <code className="min-w-0 truncate" title={item.id}>
              {item.id}
            </code>
            <span className="shrink-0 text-muted-foreground">
              {item.renderClass}
            </span>
          </li>
        ))}
      </ol>
    </details>
  );
}
