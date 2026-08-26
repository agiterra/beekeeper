import * as React from "react";
import { ArrowRightLeft, Flag } from "lucide-react";

import {
  buildCodingSessionHandoffPrefill,
  isCompletedCodingSessionTurnBlock,
  parseCodingSessionHandoffPrefill,
  readCodingSessionTurnBlockPrompt,
  resolveCodingSessionHandoffSource,
  type CodingSessionHandoffLink,
} from "@/features/coding-sessions/lib/codingSessionHandoff";
import type { CodingSessionUmbrellaTurnBlock as TurnBlock } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type {
  CodingSessionCatalogRecord,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { codingSessionAgentAccent } from "./CodingSessionAgentFocus";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import type { CodingSessionUmbrellaComposerPrefill } from "./CodingSessionUmbrellaComposer";

export function CodingSessionUmbrellaTurnBlock({
  block,
  blockKey,
  channelId,
  currentUserPubkey,
  isHighlighted,
  isFolded,
  isWorking,
  label,
  labelsByExecutionKey,
  onHandoff,
  onFocusExecution,
  onRegisterNode,
  onRevealFact,
  operatorProfiles,
  record,
  resolveFactLocation,
  showProvenance,
  stickyProvenance,
  umbrella,
}: {
  block: TurnBlock;
  blockKey: string;
  channelId: string;
  currentUserPubkey: string | null;
  isHighlighted: boolean;
  isFolded: boolean;
  isWorking: boolean;
  label: string;
  labelsByExecutionKey: ReadonlyMap<string, string>;
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onFocusExecution?: (executionKey: string | null) => void;
  onRegisterNode: (key: string, node: HTMLElement | null) => void;
  onRevealFact: (key: string) => void;
  operatorProfiles: UserProfileLookup | undefined;
  record: CodingSessionCatalogRecord | null;
  resolveFactLocation: (link: CodingSessionHandoffLink) => string | null;
  showProvenance: boolean;
  stickyProvenance: boolean;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const prompt = readCodingSessionTurnBlockPrompt(block);
  const handoff = prompt ? parseCodingSessionHandoffPrefill(prompt.text) : null;
  const execution = umbrella.executions.find(
    (candidate) => candidate.executionKey === block.executionKey,
  );
  const isForeign =
    umbrella.founderPubkey !== null &&
    execution?.operatorPubkey != null &&
    execution.operatorPubkey !== umbrella.founderPubkey;
  const handoffTargets = umbrella.executions.filter(
    (candidate) => candidate.executionKey !== block.executionKey,
  );
  const completed = isCompletedCodingSessionTurnBlock(block);
  const source = completed ? resolveCodingSessionHandoffSource(block) : null;
  const sourceLocation =
    handoff?.link != null ? resolveFactLocation(handoff.link) : null;
  const registerNode = React.useCallback(
    (node: HTMLElement | null) => onRegisterNode(blockKey, node),
    [blockKey, onRegisterNode],
  );
  const accent = codingSessionAgentAccent(block.executionKey);

  if (isFolded) {
    return (
      <button
        className="group/folded flex w-full items-center gap-2 rounded-xl border border-border/45 bg-muted/20 px-3 py-2 text-left text-xs text-muted-foreground transition-colors hover:border-border hover:bg-muted/35 hover:text-foreground"
        data-execution={block.executionKey}
        data-testid="coding-session-umbrella-folded-turn"
        onClick={() => onFocusExecution?.(block.executionKey)}
        ref={registerNode as React.Ref<HTMLButtonElement>}
        type="button"
      >
        <span aria-hidden className={cn("size-2 rounded-full", accent.dot)} />
        <span className="max-w-48 shrink-0 truncate font-medium text-foreground/80">
          {label}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {foldedTurnSummary(block)}
        </span>
        <span className="shrink-0 text-2xs">
          {isWorking ? "working" : completed ? "completed" : "activity"}
        </span>
      </button>
    );
  }

  return (
    <article
      className={cn(
        "group/turn relative border-t border-l-2 border-border/40 pt-5 pb-1 pl-4 first:border-t-0 first:pt-1 transition-colors",
        accent.border,
        isHighlighted &&
          "-mx-3 rounded-2xl bg-primary/5 px-3 ring-1 ring-primary/60",
      )}
      data-block={blockKey}
      data-execution={block.executionKey}
      data-highlighted={isHighlighted ? "true" : undefined}
      data-signer={block.signerPubkey}
      data-testid="coding-session-umbrella-turn-block"
      ref={registerNode}
    >
      <span className="sr-only">
        Response from {label}, signer {truncatePubkey(block.signerPubkey)},
        generation {block.generation}.
      </span>
      {showProvenance || stickyProvenance ? (
        <header
          className={cn(
            "mb-3 flex flex-wrap items-center gap-2 bg-background py-1.5",
            stickyProvenance &&
              "sticky top-0 z-10 -mx-2 border-b border-border/45 px-2",
          )}
          data-testid="coding-session-umbrella-provenance"
        >
          <span
            className={cn(
              "inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium",
              accent.soft,
              accent.text,
            )}
          >
            <span
              aria-hidden
              className={cn("size-2 rounded-full", accent.dot)}
            />
            {label}
          </span>
          <span
            className="font-mono text-2xs text-muted-foreground"
            title="Fact-stream signer for every item in this execution run"
          >
            {truncatePubkey(block.signerPubkey)}
          </span>
          <span className="text-2xs text-muted-foreground">
            generation {block.generation}
          </span>
          {isForeign ? (
            <span
              className="inline-flex items-center gap-1 rounded-md bg-amber-500/10 px-1.5 py-0.5 text-2xs text-amber-700 dark:text-amber-300"
              data-testid="coding-session-umbrella-foreign-flag"
              title="This execution was attached by an operator other than the session founder."
            >
              <Flag aria-hidden className="size-3" />
              foreign
            </span>
          ) : null}
        </header>
      ) : null}
      {handoff ? (
        <p
          className="mb-2 inline-flex flex-wrap items-center gap-1.5 rounded-lg bg-primary/10 px-2 py-1 text-xs"
          data-testid="coding-session-umbrella-handoff-chip"
        >
          <ArrowRightLeft aria-hidden className="size-3.5" />
          <span>
            Handoff from {handoff.sourceLabel} → {label}
          </span>
          {handoff.link === null ? null : sourceLocation !== null ? (
            <button
              className="underline underline-offset-2"
              data-testid="coding-session-umbrella-view-source"
              onClick={() => onRevealFact(sourceLocation)}
              type="button"
            >
              View source
            </button>
          ) : (
            <span
              className="text-muted-foreground"
              data-testid="coding-session-umbrella-source-unavailable"
              title={handoff.linkUrl}
            >
              source not in this view
            </span>
          )}
        </p>
      ) : null}
      <CodingSessionTranscript
        currentUserPubkey={currentUserPubkey}
        generationId={block.generationId}
        isWorking={isWorking}
        items={block.items}
        operatorProfiles={operatorProfiles}
      />
      {completed ? (
        <footer
          className="mt-2 flex flex-wrap items-center gap-1.5"
          data-testid="coding-session-umbrella-handoff-actions"
        >
          <button
            className="inline-flex items-center gap-1 rounded-full border border-border/70 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/35 hover:text-foreground"
            data-testid="coding-session-umbrella-reply"
            onClick={() =>
              onHandoff({
                id: `reply:${block.generationId}:${block.turnId ?? "no-turn"}:${Date.now()}`,
                participantKey: `execution:${block.executionKey}`,
                text: "",
              })
            }
            type="button"
          >
            Reply to {label}
          </button>
          {source
            ? handoffTargets.map((target) => (
                <button
                  className="inline-flex items-center gap-1 rounded-full border border-border/70 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/35 hover:text-foreground"
                  data-testid="coding-session-umbrella-send-to"
                  key={target.executionKey}
                  onClick={() =>
                    onHandoff(
                      buildUmbrellaTurnBlockHandoff({
                        block,
                        channelId,
                        quote: source.quote,
                        eventSeq: source.eventSeq,
                        record,
                        sourceLabel: label,
                        targetExecutionKey: target.executionKey,
                      }),
                    )
                  }
                  type="button"
                >
                  <ArrowRightLeft aria-hidden className="size-3" />
                  Send to{" "}
                  {labelsByExecutionKey.get(target.executionKey) ??
                    truncatePubkey(target.signerPubkey)}
                </button>
              ))
            : null}
        </footer>
      ) : null}
    </article>
  );
}

function foldedTurnSummary(block: TurnBlock): string {
  const source = resolveCodingSessionHandoffSource(block);
  if (source?.quote.trim()) return source.quote.trim().replace(/\s+/g, " ");
  const prompt = readCodingSessionTurnBlockPrompt(block);
  if (prompt?.text.trim()) return prompt.text.trim().replace(/\s+/g, " ");
  return "Signed execution activity";
}

export function buildUmbrellaTurnBlockHandoff(input: {
  block: TurnBlock;
  channelId: string;
  quote: string;
  eventSeq: number | null;
  record: CodingSessionCatalogRecord | null;
  sourceLabel: string;
  targetExecutionKey: string;
}): CodingSessionUmbrellaComposerPrefill {
  const target = input.record?.commandTarget ?? null;
  const text =
    target !== null && input.eventSeq !== null
      ? buildCodingSessionHandoffPrefill({
          sourceLabel: input.sourceLabel,
          link: {
            channelId: input.channelId,
            targetKey: buildCodingSessionTargetKey(target),
            eventSeq: input.eventSeq,
          },
          quote: input.quote,
        })
      : `> From ${input.sourceLabel} (this session)\n${input.quote
          .trim()
          .split("\n")
          .map((line) => `> ${line}`)
          .join("\n")}\n\n`;
  return {
    id: `handoff:${input.block.generationId}:${input.block.turnId ?? "no-turn"}:${input.targetExecutionKey}:${Date.now()}`,
    participantKey: `execution:${input.targetExecutionKey}`,
    text,
  };
}
