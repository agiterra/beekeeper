import * as React from "react";
import { ArrowUp, Check, ChevronDown, MessagesSquare } from "lucide-react";

import {
  buildCodingSessionTargetKey,
  codingSessionTargetSupportsInterrupt,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionPromptHistory } from "@/features/coding-sessions/lib/codingSessionPromptHistory";
import { usePendingCodingSessionTurns } from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import { publishCodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionLanePublish";
import {
  listCodingSessionUmbrellaParticipants,
  type CodingSessionActorNameResolver,
  type CodingSessionUmbrellaParticipant,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import {
  codingSessionUmbrellaParticipantKey,
  defaultCodingSessionUmbrellaParticipantKey,
  resolveCodingSessionUmbrellaComposerAuthority,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import {
  resolveCodingSessionMention,
  stripCodingSessionMentionForTarget,
  type CodingSessionMentionResolution,
} from "@/features/coding-sessions/lib/codingSessionMentionRouting";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import {
  UNKNOWN_CODING_SESSION_REACHABILITY,
  type CodingSessionReachabilityResolver,
} from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { shouldSubmitCodingSessionComposerKey } from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { CodingSessionComposer } from "./CodingSessionComposer";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { codingSessionAgentAccent } from "./CodingSessionAgentFocus";

/** A staged handoff: select the target execution and pre-load its editor. */
export type CodingSessionUmbrellaComposerPrefill = {
  id: string;
  participantKey: string;
  text: string;
};

type CodingSessionUmbrellaComposerProps = {
  /** Live operator grants from the session roster; null while unknown. */
  acceptedOperators?: ReadonlySet<string> | null;
  /**
   * Names for the umbrella's seated actors, resolved by the workspace that
   * owns a query client. Absent, a seat labels itself by its role alone.
   */
  actorNames?: CodingSessionActorNameResolver;
  channelId: string;
  isMember: boolean;
  umbrella: CodingSessionUmbrellaRecord;
  /** The signed-in identity, for founder preflight; null while loading. */
  currentUserPubkey: string | null;
  /** Opens the add-provider flow from an ended or unanswered execution. */
  onAddProvider?: () => void;
  prefill?: CodingSessionUmbrellaComposerPrefill | null;
  /**
   * Coordination's answer for one execution, threaded from the surface that
   * owns the read. Defaults to "no claim", which is what a composer rendered
   * outside a coordination-capable tree honestly knows (§2 item 41).
   */
  resolveReachability?: CodingSessionReachabilityResolver;
  layout?: "inline" | "stacked";
  publishLaneMessage?: typeof publishCodingSessionLaneMessage;
  onSelectedParticipantChange?: (participantKey: string | null) => void;
};

/**
 * Composer for an umbrella session: a participant selector (one entry per
 * execution plus "Session" when a lane exists) over the existing per-target
 * composer. At N=1 the selector is not rendered and the single execution's
 * composer appears exactly as today.
 *
 * Typing a leading `@handle` is sugar over that same selection: the selector
 * moves to the named execution so the target is never implicit, and the handle
 * is stripped from the text that is actually published. Nothing about the wire
 * changes — it is still a 44220 turn command against that execution's governed
 * target — and at N=1 there are no handles at all.
 */
export function CodingSessionUmbrellaComposer({
  acceptedOperators = null,
  actorNames,
  channelId,
  currentUserPubkey,
  isMember,
  layout = "inline",
  onAddProvider,
  prefill = null,
  publishLaneMessage = publishCodingSessionLaneMessage,
  onSelectedParticipantChange,
  resolveReachability = UNKNOWN_CODING_SESSION_REACHABILITY,
  umbrella,
}: CodingSessionUmbrellaComposerProps) {
  const participants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella, actorNames),
    [actorNames, umbrella],
  );
  const authority = resolveCodingSessionUmbrellaComposerAuthority({
    umbrella,
    currentUserPubkey,
    acceptedOperators,
  });
  const [selectedKey, setSelectedKey] = React.useState<string | null>(() =>
    defaultCodingSessionUmbrellaParticipantKey(participants),
  );
  // Bumped only by an *explicit* selection — a chip click or a staged handoff.
  // It keys the editor, so picking a participant by hand starts a clean draft,
  // while a typed @mention carries the draft it was written in (that draft is
  // the whole reason the target moved).
  const [draftEpoch, setDraftEpoch] = React.useState(0);
  const [draft, setDraft] = React.useState("");
  const [appliedPrefillId, setAppliedPrefillId] = React.useState<string | null>(
    null,
  );
  if (prefill && prefill.id !== appliedPrefillId) {
    setAppliedPrefillId(prefill.id);
    setSelectedKey(prefill.participantKey);
    setDraftEpoch((epoch) => epoch + 1);
    setDraft(prefill.text);
  }
  const selectParticipant = (key: string) => {
    if (key === selectedKey) return;
    setSelectedKey(key);
    setDraftEpoch((epoch) => epoch + 1);
    setDraft("");
  };

  const selected =
    participants.find(
      (participant) =>
        codingSessionUmbrellaParticipantKey(participant) === selectedKey,
    ) ??
    participants.find(
      (
        participant,
      ): participant is Extract<
        CodingSessionUmbrellaParticipant,
        { kind: "execution" }
      > => participant.kind === "execution",
    ) ??
    null;
  const selectedParticipantKey =
    selected === null ? null : codingSessionUmbrellaParticipantKey(selected);
  React.useEffect(() => {
    onSelectedParticipantChange?.(selectedParticipantKey);
  }, [onSelectedParticipantChange, selectedParticipantKey]);

  const mention = resolveCodingSessionMention({ participants, text: draft });
  if (
    mention.kind === "match" &&
    selected?.kind === "execution" &&
    mention.participantKey !== selectedParticipantKey
  ) {
    // Render-phase retarget: the selector follows the typed handle without
    // remounting the editor, so the draft survives and the caret does not
    // move. Only from an execution — in the Session lane you are talking to
    // people *about* the agents, and prose there must stay prose.
    setSelectedKey(mention.participantKey);
  }
  const mentionHint = describeCodingSessionMention(mention);
  const showMentionHint =
    participants.length > 1 &&
    selected?.kind === "execution" &&
    (mention.kind === "match" ||
      mention.kind === "ambiguous" ||
      mention.kind === "unavailable") &&
    mentionHint.length > 0;
  const prepareText = React.useCallback(
    (text: string) =>
      stripCodingSessionMentionForTarget({
        participants,
        participantKey: selectedParticipantKey,
        text,
      }),
    [participants, selectedParticipantKey],
  );
  const recipientControl =
    participants.length > 1 ? (
      <CodingSessionParticipantPicker
        authority={authority}
        onSelect={selectParticipant}
        participants={participants}
        selected={selected}
      />
    ) : undefined;

  return (
    <div data-testid="coding-session-umbrella-composer">
      {showMentionHint ? (
        <p
          // Where a typed handle is about to send is not something to discover
          // by sight only.
          aria-live="polite"
          className="mb-2 px-1 text-2xs text-muted-foreground"
          data-state={mention.kind}
          data-testid="coding-session-mention-hint"
        >
          {mentionHint}
        </p>
      ) : null}
      {selected === null ? null : selected.kind === "session" ? (
        <CodingSessionLaneComposer
          channelId={channelId}
          isMember={isMember}
          publishLaneMessage={publishLaneMessage}
          recipientControl={recipientControl}
          sessionRef={selected.sessionRef}
        />
      ) : !authority.canPromptExecutions ? (
        <div
          className="rounded-3xl border border-border/35 bg-muted/35 p-4 shadow-lg"
          data-testid="coding-session-umbrella-composer-gated"
        >
          <p className="text-sm text-muted-foreground">{authority.reason}</p>
          <div className="mt-4 flex min-h-10 items-center">
            {recipientControl}
          </div>
        </div>
      ) : (
        <ExecutionComposer
          authority={authority}
          channelId={channelId}
          currentUserPubkey={currentUserPubkey}
          isMember={isMember}
          // Keyed by explicit selection: the editor holds its draft in local
          // state, so without a fresh instance per hand-picked participant a
          // half-written prompt for Claude would be sitting in the box — and
          // would be *sent* — after switching the selector to Codex. A typed
          // @mention deliberately does not bump this: it is the draft itself
          // that chose the new target.
          key={`draft-${draftEpoch}`}
          layout={layout}
          onAddProvider={onAddProvider}
          onTextChange={setDraft}
          participant={selected}
          prepareText={prepareText}
          prefill={
            prefill &&
            prefill.participantKey ===
              codingSessionUmbrellaParticipantKey(selected)
              ? { id: prefill.id, text: prefill.text }
              : null
          }
          recipientControl={recipientControl}
          resolveReachability={resolveReachability}
        />
      )}
    </div>
  );
}

function ExecutionComposer({
  authority,
  channelId,
  currentUserPubkey,
  isMember,
  layout,
  onAddProvider,
  onTextChange,
  participant,
  prepareText,
  prefill,
  recipientControl,
  resolveReachability,
}: {
  authority: ReturnType<typeof resolveCodingSessionUmbrellaComposerAuthority>;
  channelId: string;
  currentUserPubkey: string | null;
  isMember: boolean;
  layout: "inline" | "stacked";
  onAddProvider?: () => void;
  onTextChange: (text: string) => void;
  participant: Extract<CodingSessionUmbrellaParticipant, { kind: "execution" }>;
  prepareText: (text: string) => string;
  prefill: { id: string; text: string } | null;
  recipientControl?: React.ReactNode;
  resolveReachability: CodingSessionReachabilityResolver;
}) {
  const record = participant.execution.activeGeneration;
  const target = record.commandTarget;
  // Recall spans the whole execution, prior generations included: a reconnect
  // starts a new generation but not a new conversation, and a prompt sent
  // before the resume is exactly the one worth sending again.
  const pendingTurns = usePendingCodingSessionTurns();
  const promptHistory = React.useMemo(() => {
    const targetKey = target ? buildCodingSessionTargetKey(target) : null;
    return buildCodingSessionPromptHistory({
      transcript: [
        ...participant.execution.priorGenerations,
        participant.execution.activeGeneration,
      ].flatMap((generation) => generation.transcript),
      pending: targetKey
        ? pendingTurns.filter((turn) => turn.targetKey === targetKey)
        : [],
      currentPubkey: currentUserPubkey,
    });
  }, [
    currentUserPubkey,
    participant.execution.activeGeneration,
    participant.execution.priorGenerations,
    pendingTurns,
    target,
  ]);
  // The lease, not the newest report, decides whether this composer is talking
  // to anything (§2 item 41).
  const reachability = resolveReachability(target);
  const status = deriveCodingSessionWorkspaceStatus(
    record.transcript,
    record.status,
    record.statusAt,
    reachability,
  );
  const isWorking = status.kind === "working";
  const runtime = record.runtime ?? record.provider;
  const runtimeLabel = runtime
    ? formatCodingSessionRuntimeLabel(runtime)
    : null;
  const providerLabel = record.provider
    ? formatCodingSessionRuntimeLabel(record.provider)
    : null;
  if (!target) {
    return (
      <p
        className="rounded-3xl border border-border/70 bg-background/95 p-4 text-sm text-muted-foreground shadow-lg backdrop-blur-xl"
        data-testid="coding-session-umbrella-composer-untargeted"
      >
        This execution has not published a governed command target yet.
      </p>
    );
  }
  return (
    <CodingSessionComposer
      authorityReason={authority.reason}
      authorityUnresolved={authority.isUnresolved}
      canInterrupt={
        codingSessionTargetSupportsInterrupt(target) &&
        record.capabilities?.threadTurnInterrupt !== false
      }
      canControl={authority.canPromptExecutions}
      canSteer={record.capabilities?.threadSteer === true}
      canAttachImages={record.capabilities?.promptImage === true}
      runtimeLabel={runtimeLabel}
      channelId={channelId}
      onAddProvider={onAddProvider}
      controlContext={{
        capabilities: record.capabilities,
        model: record.model,
        providerLabel,
        runtimeLabel,
        status,
        turnBudget: record.turnBudget,
      }}
      currentUserPubkey={currentUserPubkey}
      immersive
      isMember={isMember}
      isWorking={isWorking}
      isUngovernedSession={authority.isUngovernedSession}
      lifecycleStatus={record.status}
      layout={layout}
      onTextChange={onTextChange}
      prepareText={prepareText}
      prefill={prefill}
      promptHistory={promptHistory}
      providerAuthorityPubkey={participant.execution.signerPubkey}
      recipientControl={recipientControl}
      seatActorPubkey={record.agentRef}
      seatRole={record.role}
      projectRef={record.projectRef}
      target={target}
      variant="floating"
    />
  );
}

function CodingSessionParticipantPicker({
  authority,
  onSelect,
  participants,
  selected,
}: {
  authority: ReturnType<typeof resolveCodingSessionUmbrellaComposerAuthority>;
  onSelect: (participantKey: string) => void;
  participants: readonly CodingSessionUmbrellaParticipant[];
  selected: CodingSessionUmbrellaParticipant | null;
}) {
  const selectedPresentation = selected
    ? participantPresentation(selected)
    : { title: "Choose recipient", detail: null };
  const selectedAccent =
    selected?.kind === "execution"
      ? codingSessionAgentAccent(selected.executionKey)
      : null;
  return (
    <div data-testid="coding-session-participant-selector">
      <Popover>
        <PopoverTrigger asChild>
          <button
            aria-label={`Send to ${selectedPresentation.title}`}
            className="flex min-w-0 max-w-72 items-center gap-2 rounded-lg py-1.5 pr-2 text-foreground/80 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="coding-session-participant-picker-trigger"
            type="button"
          >
            {selected?.kind === "session" ? (
              <MessagesSquare aria-hidden className="size-4 shrink-0" />
            ) : selectedAccent ? (
              <span
                aria-hidden
                className={cn(
                  "grid size-5 shrink-0 place-items-center rounded-full",
                  selectedAccent.soft,
                )}
              >
                <span
                  className={cn("size-2 rounded-full", selectedAccent.dot)}
                />
              </span>
            ) : null}
            <span className="truncate">
              Send to {selectedPresentation.title}
            </span>
            <ChevronDown aria-hidden className="size-3.5 shrink-0 opacity-60" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-96 max-w-[calc(100vw-2rem)] p-2"
          side="top"
        >
          <p className="px-2 pt-1 pb-2 text-xs font-medium text-muted-foreground">
            Send to
          </p>
          <div className="grid gap-1">
            {participants.map((participant) => {
              const key = codingSessionUmbrellaParticipantKey(participant);
              const isSelected =
                selected !== null &&
                codingSessionUmbrellaParticipantKey(selected) === key;
              const gated =
                participant.kind === "execution" &&
                !authority.canPromptExecutions;
              const presentation = participantPresentation(participant);
              const accent =
                participant.kind === "execution"
                  ? codingSessionAgentAccent(participant.executionKey)
                  : null;
              return (
                <button
                  aria-pressed={isSelected}
                  className="flex min-w-0 items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-muted/60 disabled:cursor-not-allowed disabled:opacity-45"
                  data-participant={key}
                  data-testid={`coding-session-participant-${
                    participant.kind === "execution" ? "execution" : "session"
                  }`}
                  disabled={gated}
                  key={key}
                  onClick={() => onSelect(key)}
                  title={gated ? (authority.reason ?? undefined) : undefined}
                  type="button"
                >
                  <span
                    className={cn(
                      "grid size-8 shrink-0 place-items-center rounded-full",
                      accent?.soft ?? "bg-muted",
                    )}
                  >
                    {participant.kind === "session" ? (
                      <MessagesSquare aria-hidden className="size-4" />
                    ) : accent ? (
                      <span
                        aria-hidden
                        className={cn("size-2.5 rounded-full", accent.dot)}
                      />
                    ) : null}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium text-foreground">
                      {presentation.title}
                    </span>
                    {presentation.detail ? (
                      <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                        {presentation.detail}
                      </span>
                    ) : null}
                  </span>
                  <Check
                    aria-hidden
                    className={cn(
                      "size-4 shrink-0 text-primary",
                      !isSelected && "invisible",
                    )}
                  />
                </button>
              );
            })}
          </div>
          <p className="px-3 pt-2 pb-1 text-xs text-muted-foreground">
            A leading @name also routes an agent prompt.
          </p>
        </PopoverContent>
      </Popover>
    </div>
  );
}

function participantPresentation(
  participant: CodingSessionUmbrellaParticipant,
): { title: string; detail: string | null } {
  if (participant.kind === "session") {
    return { title: "Session", detail: "Everyone in this session" };
  }
  return { title: participant.label, detail: null };
}

/**
 * The composer's one line about mentions: which handles exist, or what the
 * handle currently typed is going to do. An unknown handle says nothing — the
 * person is probably mid-word — but an ambiguous or unreachable one says so
 * plainly, since that is exactly where an unspoken assumption ("Claude got
 * this") would otherwise be wrong. Empty means: render no line at all.
 */
function describeCodingSessionMention(
  mention: CodingSessionMentionResolution,
): string {
  switch (mention.kind) {
    case "match":
      return `Sending to ${mention.label} — @${mention.raw} is removed from the prompt.`;
    case "ambiguous":
      return `@${mention.raw} fits ${mention.labels.join(" and ")}, so it stays plain text. Pick one above.`;
    case "unavailable":
      return `@${mention.raw} is ${mention.label}, but ${mention.reason}, so it stays plain text.`;
    default:
      return "";
  }
}

/**
 * Composer for the "Session" participant: an ordinary kind:9 message in the
 * host channel tagged with the umbrella's sessionRef. It is chat — any member
 * with channel write may speak, founder or not.
 */
function CodingSessionLaneComposer({
  channelId,
  isMember,
  publishLaneMessage,
  recipientControl,
  sessionRef,
}: {
  channelId: string;
  isMember: boolean;
  publishLaneMessage: typeof publishCodingSessionLaneMessage;
  recipientControl?: React.ReactNode;
  sessionRef: string;
}) {
  const [text, setText] = React.useState("");
  const [isSending, setIsSending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const canSend = isMember && !isSending && text.trim().length > 0;

  const submit = React.useCallback(async () => {
    if (!canSend) return;
    setIsSending(true);
    setError(null);
    try {
      await publishLaneMessage({
        channelId,
        content: text.trim(),
        sessionRef,
      });
      setText("");
    } catch (sendError) {
      setError(
        sendError instanceof Error
          ? sendError.message
          : "Unable to send the session message.",
      );
    } finally {
      setIsSending(false);
    }
  }, [canSend, channelId, publishLaneMessage, sessionRef, text]);

  return (
    <div
      className="overflow-hidden rounded-3xl border border-border/35 bg-muted/35 shadow-lg"
      data-testid="coding-session-lane-composer"
    >
      {error ? (
        <p className="px-4 pt-3 text-sm text-destructive">{error}</p>
      ) : null}
      <Textarea
        aria-label="Session conversation message"
        className="block min-h-24 w-full resize-none rounded-none border-0 bg-transparent px-4 pt-4 pb-1 shadow-none focus-visible:ring-0"
        disabled={!isMember || isSending}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (shouldSubmitCodingSessionComposerKey(event)) {
            event.preventDefault();
            void submit();
          }
        }}
        placeholder="Message everyone in this session…"
        value={text}
      />
      <div className="flex min-h-14 items-center gap-3 px-4 pb-3 text-sm text-muted-foreground">
        {recipientControl}
        <span className="hidden sm:inline">Visible to everyone</span>
        <button
          aria-label={isSending ? "Sending" : "Send session message"}
          className="ml-auto grid size-9 place-items-center rounded-full bg-primary text-primary-foreground shadow-sm transition-transform enabled:hover:scale-105 disabled:cursor-not-allowed disabled:opacity-30"
          data-testid="coding-session-lane-send"
          disabled={!canSend}
          onClick={() => void submit()}
          type="button"
        >
          {isSending ? (
            <span
              aria-hidden
              className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent"
            />
          ) : (
            <ArrowUp aria-hidden className="size-4" />
          )}
        </button>
      </div>
    </div>
  );
}
