import { ChevronRight } from "lucide-react";

import type {
  CodingSessionTranscriptBlock,
  ProjectedTranscriptItem,
} from "../domain/index.ts";

const ROLE_CLASS: Record<ProjectedTranscriptItem["role"], string> = {
  user: "text-black dark:text-white",
  assistant: "text-black/80 dark:text-white/80",
  tool: "text-black/70 dark:text-white/70",
  lifecycle: "text-black/60 dark:text-white/60",
};

function MetaChips({ meta }: { meta: readonly string[] }) {
  if (meta.length === 0) return null;
  return (
    <span className="ml-2 inline-flex flex-wrap gap-1 align-middle">
      {meta.map((chip) => (
        <span
          key={chip}
          className="rounded bg-black/5 px-1.5 py-0.5 font-mono text-xs text-black/50 dark:bg-white/10 dark:text-white/50"
        >
          {chip}
        </span>
      ))}
    </span>
  );
}

function LifecycleFacts({
  lifecycle,
}: {
  lifecycle: NonNullable<ProjectedTranscriptItem["lifecycle"]>;
}) {
  const facts: string[] = [];
  if (lifecycle.durationMs !== null) {
    facts.push(`${Math.round(lifecycle.durationMs / 100) / 10}s`);
  }
  if (lifecycle.costUsd !== null) {
    facts.push(`$${lifecycle.costUsd.toFixed(4)}`);
  }
  if (lifecycle.isError) {
    facts.push("error");
  }
  if (facts.length === 0) return null;
  return (
    <span
      className={`ml-2 font-mono text-xs ${
        lifecycle.isError
          ? "text-red-600 dark:text-red-400"
          : "text-black/50 dark:text-white/50"
      }`}
    >
      {facts.join(" · ")}
    </span>
  );
}

function ToolDetails({
  tool,
}: {
  tool: NonNullable<ProjectedTranscriptItem["tool"]>;
}) {
  const argKeys = Object.keys(tool.args);
  return (
    <div className="mt-2 space-y-2">
      {argKeys.length > 0 && (
        <pre className="overflow-x-auto rounded bg-black/5 p-2 font-mono text-xs text-black/70 dark:bg-white/10 dark:text-white/70">
          {JSON.stringify(tool.args, null, 2)}
        </pre>
      )}
      {tool.result.length > 0 && (
        <pre className="overflow-x-auto whitespace-pre-wrap rounded bg-black/5 p-2 font-mono text-xs text-black/70 dark:bg-white/10 dark:text-white/70">
          {tool.result}
        </pre>
      )}
    </div>
  );
}

function RowBody({ item }: { item: ProjectedTranscriptItem }) {
  return (
    <>
      {item.text.length > 0 && (
        <p className="mt-1 whitespace-pre-wrap text-sm leading-relaxed">
          {item.text}
        </p>
      )}
      {item.tool !== null && <ToolDetails tool={item.tool} />}
    </>
  );
}

/**
 * One projected row.
 *
 * Folded rows use a native `<details>`: expansion needs no state, survives a
 * re-render from a live event mid-read, and stays keyboard-reachable.
 * A row that carries no payload — an unknown kind, an elision — renders its
 * title and stops. Inventing a body for it would be inventing a fact.
 */
export function CodingSessionTranscriptRow({
  item,
}: {
  item: ProjectedTranscriptItem;
}) {
  const heading = (
    <>
      <span className="font-medium">{item.title}</span>
      {item.unknownKind !== null && (
        <span className="ml-2 font-mono text-xs text-black/50 dark:text-white/50">
          kind {item.unknownKind}
        </span>
      )}
      {item.tool !== null && (
        <span className="ml-2 font-mono text-xs text-black/50 dark:text-white/50">
          {item.tool.toolName} · {item.tool.status}
        </span>
      )}
      {item.lifecycle !== null && <LifecycleFacts lifecycle={item.lifecycle} />}
      <MetaChips meta={item.meta} />
    </>
  );

  if (item.folded) {
    return (
      <details
        className={`group px-3 py-2 text-sm ${ROLE_CLASS[item.role]}`}
        data-testid="coding-session-transcript-row"
        data-role={item.role}
        data-folded="true"
      >
        <summary className="flex cursor-pointer list-none items-center gap-1.5">
          <ChevronRight className="h-3.5 w-3.5 shrink-0 transition-transform group-open:rotate-90" />
          <span className="min-w-0 flex-1 truncate">{heading}</span>
        </summary>
        <div className="pl-5">
          <RowBody item={item} />
        </div>
      </details>
    );
  }

  return (
    <div
      className={`px-3 py-2 text-sm ${ROLE_CLASS[item.role]}`}
      data-testid="coding-session-transcript-row"
      data-role={item.role}
      data-folded="false"
    >
      <div className="flex items-baseline">
        <span className="min-w-0 flex-1">{heading}</span>
      </div>
      <RowBody item={item} />
    </div>
  );
}

function TurnSeparator() {
  return (
    <div
      className="my-1 border-t border-dashed border-black/10 dark:border-white/10"
      data-testid="coding-session-turn-separator"
    />
  );
}

function Block({ block }: { block: CodingSessionTranscriptBlock }) {
  let previousTurnId: string | null = null;
  let seenFirstRow = false;
  return (
    <section
      className="overflow-hidden rounded-lg border border-black/10 bg-white/50 dark:border-white/10 dark:bg-white/5"
      data-testid="coding-session-transcript-block"
    >
      <header className="border-b border-black/10 bg-black/5 px-3 py-1.5 font-mono text-xs text-black/60 dark:border-white/10 dark:bg-white/10 dark:text-white/60">
        {block.label}
      </header>
      <div className="divide-y divide-black/5 dark:divide-white/5">
        {block.items.map((item) => {
          const opensTurn =
            item.turnId !== null &&
            item.turnId !== previousTurnId &&
            seenFirstRow;
          previousTurnId = item.turnId;
          seenFirstRow = true;
          return (
            <div key={item.id}>
              {opensTurn && <TurnSeparator />}
              <CodingSessionTranscriptRow item={item} />
            </div>
          );
        })}
      </div>
    </section>
  );
}

/**
 * The transcript: blocks interleave, rows never do.
 *
 * Two providers writing into one session have no shared clock, so merging
 * their rows would invent an ordering nobody signed.
 */
export function CodingSessionTranscript({
  blocks,
}: {
  blocks: readonly CodingSessionTranscriptBlock[];
}) {
  if (blocks.length === 0) {
    return (
      <p
        className="mt-4 rounded-lg border border-black/10 px-4 py-6 text-center text-sm text-black/50 dark:border-white/10 dark:text-white/50"
        data-testid="coding-session-transcript-empty"
      >
        No transcript has been published for this session yet.
      </p>
    );
  }
  return (
    <div className="mt-4 space-y-4" data-testid="coding-session-transcript">
      {blocks.map((block) => (
        <Block key={block.blockKey} block={block} />
      ))}
    </div>
  );
}
