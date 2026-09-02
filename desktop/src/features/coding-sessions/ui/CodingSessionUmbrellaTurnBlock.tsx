import * as React from "react";
import { ArrowRightLeft, ChevronDown, Flag } from "lucide-react";

import {
  buildCodingSessionHandoffPrefill,
  isCompletedCodingSessionTurnBlock,
  parseCodingSessionHandoffPrefill,
  readCodingSessionTurnBlockPrompt,
  resolveCodingSessionHandoffSource,
  type CodingSessionHandoffLink,
} from "@/features/coding-sessions/lib/codingSessionHandoff";
import type { CodingSessionPromptSeatResolver } from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import {
  CODING_SESSION_CONTINUITY_STATUSES,
  CODING_SESSION_CONTINUITY_TITLE,
} from "@/features/coding-sessions/lib/codingSessionTranscriptItems";
import { buildCodingSessionTurnByline } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import type { CodingSessionUmbrellaTurnBlock as TurnBlock } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type {
  CodingSessionCatalogRecord,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { cn } from "@/shared/lib/cn";
import { isCodingSessionMissionExecutionItem } from "@/features/coding-sessions/lib/codingSessionMissionExecutionBundle";
import { codingSessionAgentAccent } from "./CodingSessionAgentFocus";
import { CodingSessionMissionExecutionBundle } from "./CodingSessionMissionExecutionBundle";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import type { CodingSessionUmbrellaComposerPrefill } from "./CodingSessionUmbrellaComposer";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

export function CodingSessionUmbrellaTurnBlock({
  actorNames,
  block,
  blockKey,
  channelId,
  currentUserPubkey,
  isHighlighted,
  isFolded,
  isWorking,
  label,
  labelsByExecutionKey,
  liveness = null,
  missionExecutionBundle = false,
  missionRowClassName,
  onHandoff,
  onFocusExecution,
  onRegisterNode,
  onRevealFact,
  operatorProfiles,
  resolvePromptSeat,
  record,
  resolveFactLocation,
  showProvenance,
  stickyProvenance,
  umbrella,
}: {
  /**
   * Resolves a seat's `agentRef` to a display name, exactly as the Agents rail
   * does. Absent, the byline falls back to role and runtime — never to the
   * provider key, which is identical on every seat in the session.
   */
  actorNames?: CodingSessionActorNameResolver;
  block: TurnBlock;
  blockKey: string;
  channelId: string;
  currentUserPubkey: string | null;
  isHighlighted: boolean;
  isFolded: boolean;
  isWorking: boolean;
  /** The surface's resolved participant label, or null when it has none. */
  label: string | null;
  labelsByExecutionKey: ReadonlyMap<string, string>;
  /**
   * This seat's W1 answer, resolved by the surface from exactly the functions
   * the roster chips use and handed down — never derived here from the
   * transcript, which is how two places end up telling a reader two different
   * things about one seat.
   *
   * Rendered only on a block that is still working, and only in Mission. A
   * settled block is a record of what happened, and stamping today's word on
   * yesterday's turn would make the record lie as soon as the seat moved on.
   */
  liveness?: CodingSessionTurnBlockLiveness | null;
  /**
   * Collapse this block's signed tool items into one C2 bundle row. Only ever
   * honoured together with `missionRowClassName`, so Conversation — which
   * passes neither — cannot reach this path at all.
   */
  missionExecutionBundle?: boolean;
  /**
   * Mission's shared card grammar for the block shell. Conversation never
   * passes it, so the one-seat lens renders byte-identical DOM.
   */
  missionRowClassName?: string;
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onFocusExecution?: (executionKey: string | null) => void;
  onRegisterNode: (key: string, node: HTMLElement | null) => void;
  onRevealFact: (key: string) => void;
  operatorProfiles: UserProfileLookup | undefined;
  /**
   * Names a seat from its actor pubkey, so a turn one seat sent to another is
   * attributed to that seat instead of to whoever is reading. Mission passes
   * it; Conversation has no second seat and passes nothing, so its prompt
   * captions are byte-identical to before.
   */
  resolvePromptSeat?: CodingSessionPromptSeatResolver;
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
  const [bundleExpanded, setBundleExpanded] = React.useState(false);
  const toggleBundle = React.useCallback(
    () => setBundleExpanded((open) => !open),
    [],
  );
  // Mission-gated, twice over: `missionRowClassName` is the prop Conversation
  // never passes, and the word only exists while the block is open.
  const mission = missionRowClassName != null;
  const liveWord =
    mission && isWorking && liveness?.word ? liveness.word.trim() : null;
  // The **word** is printed whenever the block is open, because a seat whose
  // provider stopped answering mid-turn is exactly what a reader needs told.
  // The **animation** is gated on W1's own answer, never on "a word exists":
  // `isWorking` is the raw wire status, and a demoted seat is still `running`
  // there while W1 reads `no provider answering`. Breathing over that word
  // would say two things about one seat, and the roster chip — which tests
  // `status.kind === "working"` — would say the calmer of them
  // (`CodingSessionParticipantBar.tsx:69`). REVIEW-A3 F3.
  const breathes = liveWord !== null && liveness?.live === true;
  // Double-gated on purpose: the bundle is Mission's, and `missionRowClassName`
  // is the one prop Conversation is guaranteed never to pass. A future caller
  // that sets only `missionExecutionBundle` still gets Conversation's DOM.
  const bundleExecution = missionExecutionBundle && missionRowClassName != null;
  // Finding 9: a freshly hired seat's first block opened with "Rehydrated —
  // verified session history is available to this agent" over an umbrella that
  // held no earlier generation of it. The provider's claim is about its own
  // native session and is left on the wire untouched; Mission simply does not
  // repeat it when nothing on this screen could be the history it names.
  const hasPriorGeneration = (execution?.priorGenerations.length ?? 0) > 0;
  const missionItems = React.useMemo(
    () =>
      resolveCodingSessionMissionBlockItems({
        hasPriorGeneration,
        items: block.items,
        mission,
      }),
    [block.items, hasPriorGeneration, mission],
  );
  const narrativeItems = bundleExecution
    ? missionItems.filter((item) => !isCodingSessionMissionExecutionItem(item))
    : missionItems;
  const executionItems = bundleExecution
    ? missionItems.filter(isCodingSessionMissionExecutionItem)
    : [];
  const accent = codingSessionAgentAccent(block.executionKey);
  const foldedSummary = isFolded ? foldedTurnSummary(block) : null;
  // C1a: the seat's own identity, from `agentRef` resolved through kind-0 —
  // never `block.signerPubkey`, which one provider stamps on every seat.
  const byline = buildCodingSessionTurnByline({
    agentDisplayName: record?.agentRef
      ? (actorNames?.(record.agentRef) ?? null)
      : null,
    agentRef: record?.agentRef,
    generation: block.generation,
    label,
    model: record?.model,
    providerInstanceRef: record?.provider,
    role: record?.role,
    runtime: record?.runtime,
  });
  const name = byline.name;

  if (isFolded) {
    if (foldedSummary === null) return null;
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
          {name}
        </span>
        <span className="min-w-0 flex-1 truncate">{foldedSummary}</span>
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
        // Mission's card grammar goes in the middle, never last: it carries a
        // neutral `border-border/60` and a `bg-background`, and tailwind-merge
        // lets the *last* colour in each group win. Merged after the accent it
        // painted every seat's block the same grey and flattened the focus
        // tint, so two seats became byte-identical shells. Shape from the
        // grammar; colour from identity and focus.
        missionRowClassName,
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
      {breathes ? (
        // DESIGN-SPEC §3 C1a: the **identity rail** breathes, not the card.
        // `coding-session-agent-breathe` animates a box-shadow, so on the
        // article it drew a ring around the whole block; every other use in
        // the app is a chip or an 8px dot. This overlays the shell's own
        // `border-l-2` so the glow hugs the rail. `coding-session.css` turns
        // the animation off under `prefers-reduced-motion`. REVIEW-A3 F7.
        <span
          aria-hidden
          className={cn(
            "pointer-events-none absolute inset-y-0 -left-0.5 w-0.5 rounded-full coding-session-agent-breathe",
            accent.dot,
          )}
          data-testid="coding-session-umbrella-turn-rail"
        />
      ) : null}
      <span className="sr-only">{byline.screenReader}</span>
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
            title={byline.via ?? undefined}
          >
            <span
              aria-hidden
              className={cn("size-2 rounded-full", accent.dot)}
            />
            {name}
          </span>
          {liveWord !== null ? (
            <span
              className="text-2xs font-medium text-foreground/75"
              data-testid="coding-session-umbrella-byline-liveness"
            >
              {liveWord}
            </span>
          ) : null}
          {byline.detail ? (
            <span
              className="text-2xs text-muted-foreground"
              data-testid="coding-session-umbrella-byline-detail"
            >
              {byline.detail}
            </span>
          ) : null}
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
            Handoff from {handoff.sourceLabel} → {name}
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
        items={narrativeItems}
        operatorProfiles={operatorProfiles}
        resolveSeat={resolvePromptSeat}
      />
      {bundleExecution ? (
        <CodingSessionMissionExecutionBundle
          expanded={bundleExpanded}
          items={executionItems}
          onToggle={toggleBundle}
        >
          <CodingSessionTranscript
            currentUserPubkey={currentUserPubkey}
            generationId={block.generationId}
            isWorking={false}
            items={executionItems}
            operatorProfiles={operatorProfiles}
            resolveSeat={resolvePromptSeat}
          />
        </CodingSessionMissionExecutionBundle>
      ) : null}
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
            Reply to {name}
          </button>
          {source && handoffTargets.length > 0 ? (
            <CodingSessionHandoffMenu
              block={block}
              channelId={channelId}
              eventSeq={source.eventSeq}
              labelsByExecutionKey={labelsByExecutionKey}
              onHandoff={onHandoff}
              quote={source.quote}
              record={record}
              sourceLabel={name}
              targets={handoffTargets}
            />
          ) : null}
        </footer>
      ) : null}
    </article>
  );
}

/** One seat's W1 answer, as the surface resolved it for this block. */
export type CodingSessionTurnBlockLiveness = {
  /** The word itself — `live`, `waiting for you`, `no provider answering`, … */
  word: string;
  /**
   * True only when W1 answers `working`. This is the animation's gate, and it
   * is deliberately **not** "the word is non-empty": a demoted seat has a word
   * and is not working.
   */
  live: boolean;
};

/**
 * The items Mission renders for one block, keeping `items`' identity when the
 * rehydration rule cannot bite.
 *
 * Reference stability is the point. `block.items` feeds
 * `useStableCodingSessionTranscriptModel`'s `useMemo`
 * (`CodingSessionTranscript.tsx`), so a fresh array on every render re-derives
 * the transcript model for nothing. Filtering unconditionally did exactly that
 * for every Mission block of a fresh seat, not just the one block that carries
 * the row (REVIEW-A3 F8).
 */
export function resolveCodingSessionMissionBlockItems(input: {
  hasPriorGeneration: boolean;
  items: TurnBlock["items"];
  mission: boolean;
}): TurnBlock["items"] {
  const hides = (item: TurnBlock["items"][number]) =>
    hidesCodingSessionRehydrationClaim({
      hasPriorGeneration: input.hasPriorGeneration,
      item,
      mission: input.mission,
    });
  if (!input.items.some(hides)) return input.items;
  return input.items.filter((item) => !hides(item));
}

/**
 * Should Mission omit this row as an unsupported rehydration claim?
 *
 * True for exactly one row: the provider's `session_rehydrated` continuity
 * line, in Mission, on an execution this umbrella holds no earlier generation
 * of. A freshly hired seat published it on the 2026-09-01 run and there was
 * nothing on screen it could have been describing; the wire keeps the claim
 * (it is about the provider's own native session) and the lens declines to
 * repeat it.
 *
 * Matched on the row's title plus the exact prose
 * `CODING_SESSION_CONTINUITY_STATUSES` mints for that slug, so `Started
 * fresh`, `Resumed` and `Loaded` — each of which a fresh seat can say
 * honestly — are untouched, and so is any slug a future provider adds.
 */
export function hidesCodingSessionRehydrationClaim(input: {
  hasPriorGeneration: boolean;
  item: TurnBlock["items"][number];
  mission: boolean;
}): boolean {
  if (!input.mission || input.hasPriorGeneration) return false;
  const { item } = input;
  return (
    item.type === "lifecycle" &&
    item.title === CODING_SESSION_CONTINUITY_TITLE &&
    item.text.startsWith(REHYDRATED_CONTINUITY_PROSE)
  );
}

/** The `session_rehydrated` prose, read from the map that mints it. */
const REHYDRATED_CONTINUITY_PROSE =
  CODING_SESSION_CONTINUITY_STATUSES.get("session_rehydrated") ?? "Rehydrated";

function foldedTurnSummary(block: TurnBlock): string | null {
  const source = resolveCodingSessionHandoffSource(block);
  if (source?.quote.trim()) return source.quote.trim().replace(/\s+/g, " ");
  const prompt = readCodingSessionTurnBlockPrompt(block);
  if (prompt?.text.trim()) return prompt.text.trim().replace(/\s+/g, " ");
  return null;
}

function CodingSessionHandoffMenu({
  block,
  channelId,
  eventSeq,
  labelsByExecutionKey,
  onHandoff,
  quote,
  record,
  sourceLabel,
  targets,
}: {
  block: TurnBlock;
  channelId: string;
  eventSeq: number | null;
  labelsByExecutionKey: ReadonlyMap<string, string>;
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  quote: string;
  record: CodingSessionCatalogRecord | null;
  sourceLabel: string;
  targets: CodingSessionUmbrellaRecord["executions"];
}) {
  const [open, setOpen] = React.useState(false);
  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <button
          className="inline-flex items-center gap-1 rounded-full border border-border/70 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/35 hover:text-foreground"
          data-testid="coding-session-umbrella-send-to"
          type="button"
        >
          <ArrowRightLeft aria-hidden className="size-3" />
          Send to…
          <ChevronDown aria-hidden className="size-3 opacity-60" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 p-2">
        <p className="px-2 pt-1 pb-2 text-xs font-medium text-muted-foreground">
          Hand this result to
        </p>
        <div className="grid gap-1">
          {targets.map((target) => {
            const accent = codingSessionAgentAccent(target.executionKey);
            // Same rule as the byline: one provider signs every seat here,
            // so its key names none of them.
            const targetLabel = buildCodingSessionTurnByline({
              agentRef: target.activeGeneration.agentRef,
              generation:
                target.activeGeneration.commandTarget?.generation ?? 1,
              label: labelsByExecutionKey.get(target.executionKey) ?? null,
              model: target.activeGeneration.model,
              role: target.activeGeneration.role,
              runtime: target.activeGeneration.runtime,
            }).name;
            return (
              <button
                className="flex min-h-10 items-center gap-3 rounded-xl px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60"
                data-testid="coding-session-umbrella-send-to-target"
                key={target.executionKey}
                onClick={() => {
                  onHandoff(
                    buildUmbrellaTurnBlockHandoff({
                      block,
                      channelId,
                      quote,
                      eventSeq,
                      record,
                      sourceLabel,
                      targetExecutionKey: target.executionKey,
                    }),
                  );
                  setOpen(false);
                }}
                type="button"
              >
                <span
                  aria-hidden
                  className={cn(
                    "grid size-7 shrink-0 place-items-center rounded-full",
                    accent.soft,
                  )}
                >
                  <span className={cn("size-2 rounded-full", accent.dot)} />
                </span>
                <span className="min-w-0 truncate font-medium">
                  {targetLabel}
                </span>
              </button>
            );
          })}
        </div>
      </PopoverContent>
    </Popover>
  );
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
