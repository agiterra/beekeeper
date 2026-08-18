import * as React from "react";

import { codingSessionTargetSupportsInterrupt } from "@/features/coding-sessions/lib/codingSessionCommand";
import { publishCodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionLanePublish";
import {
  listCodingSessionUmbrellaParticipants,
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
  suggestCodingSessionMentionHandles,
  type CodingSessionMentionResolution,
} from "@/features/coding-sessions/lib/codingSessionMentionRouting";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { shouldSubmitCodingSessionComposerKey } from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { CodingSessionComposer } from "./CodingSessionComposer";

/** A staged handoff: select the target execution and pre-load its editor. */
export type CodingSessionUmbrellaComposerPrefill = {
  id: string;
  participantKey: string;
  text: string;
};

type CodingSessionUmbrellaComposerProps = {
  /** Live operator grants from the session roster; null while unknown. */
  acceptedOperators?: ReadonlySet<string> | null;
  channelId: string;
  isMember: boolean;
  umbrella: CodingSessionUmbrellaRecord;
  /** The signed-in identity, for founder preflight; null while loading. */
  currentUserPubkey: string | null;
  prefill?: CodingSessionUmbrellaComposerPrefill | null;
  layout?: "inline" | "stacked";
  publishLaneMessage?: typeof publishCodingSessionLaneMessage;
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
  channelId,
  currentUserPubkey,
  isMember,
  layout = "inline",
  prefill = null,
  publishLaneMessage = publishCodingSessionLaneMessage,
  umbrella,
}: CodingSessionUmbrellaComposerProps) {
  const participants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella),
    [umbrella],
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
  const mentionHint = describeCodingSessionMention(
    mention,
    suggestCodingSessionMentionHandles(participants),
  );
  const showMentionHint =
    participants.length > 1 &&
    selected?.kind === "execution" &&
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

  return (
    <div data-testid="coding-session-umbrella-composer">
      {participants.length > 1 ? (
        <fieldset
          aria-label="Send to"
          className="mb-2 flex flex-wrap items-center gap-1.5"
          data-testid="coding-session-participant-selector"
        >
          {participants.map((participant) => {
            const key = codingSessionUmbrellaParticipantKey(participant);
            const isSelected =
              selected !== null &&
              codingSessionUmbrellaParticipantKey(selected) === key;
            const gated =
              participant.kind === "execution" &&
              !authority.canPromptExecutions;
            return (
              <button
                aria-pressed={isSelected}
                className={cn(
                  "rounded-full border px-2.5 py-1 text-xs transition-colors",
                  isSelected
                    ? "border-primary/50 bg-primary/10 text-foreground"
                    : "border-border/70 text-muted-foreground hover:text-foreground",
                  gated && "cursor-not-allowed opacity-60",
                )}
                data-participant={key}
                data-testid={`coding-session-participant-${
                  participant.kind === "execution" ? "execution" : "session"
                }`}
                disabled={gated}
                key={key}
                onClick={() => selectParticipant(key)}
                title={gated ? (authority.reason ?? undefined) : undefined}
                type="button"
              >
                {participant.label}
              </button>
            );
          })}
        </fieldset>
      ) : null}
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
          sessionRef={selected.sessionRef}
        />
      ) : !authority.canPromptExecutions ? (
        <p
          className="rounded-3xl border border-border/70 bg-background/95 p-4 text-sm text-muted-foreground shadow-lg backdrop-blur-xl"
          data-testid="coding-session-umbrella-composer-gated"
        >
          {authority.reason}
        </p>
      ) : (
        <ExecutionComposer
          authority={authority}
          channelId={channelId}
          isMember={isMember}
          // Keyed by explicit selection: the editor holds its draft in local
          // state, so without a fresh instance per hand-picked participant a
          // half-written prompt for Claude would be sitting in the box — and
          // would be *sent* — after switching the selector to Codex. A typed
          // @mention deliberately does not bump this: it is the draft itself
          // that chose the new target.
          key={`draft-${draftEpoch}`}
          layout={layout}
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
        />
      )}
    </div>
  );
}

function ExecutionComposer({
  authority,
  channelId,
  isMember,
  layout,
  onTextChange,
  participant,
  prepareText,
  prefill,
}: {
  authority: ReturnType<typeof resolveCodingSessionUmbrellaComposerAuthority>;
  channelId: string;
  isMember: boolean;
  layout: "inline" | "stacked";
  onTextChange: (text: string) => void;
  participant: Extract<CodingSessionUmbrellaParticipant, { kind: "execution" }>;
  prepareText: (text: string) => string;
  prefill: { id: string; text: string } | null;
}) {
  const record = participant.execution.activeGeneration;
  const target = record.commandTarget;
  const status = deriveCodingSessionWorkspaceStatus(
    record.transcript,
    record.status,
    record.statusAt,
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
      canInterrupt={
        codingSessionTargetSupportsInterrupt(target) &&
        record.capabilities?.threadTurnInterrupt !== false
      }
      canControl={authority.canPromptExecutions}
      canSteer={record.capabilities?.threadSteer === true}
      channelId={channelId}
      controlContext={{
        capabilities: record.capabilities,
        model: record.model,
        providerLabel,
        runtimeLabel,
        status,
      }}
      immersive
      isMember={isMember}
      isWorking={isWorking}
      isUngovernedSession={authority.isUngovernedSession}
      lifecycleStatus={record.status}
      layout={layout}
      onTextChange={onTextChange}
      prepareText={prepareText}
      prefill={prefill}
      providerAuthorityPubkey={participant.execution.signerPubkey}
      target={target}
      variant="floating"
    />
  );
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
  handles: readonly { handle: string }[],
): string {
  switch (mention.kind) {
    case "match":
      return `Sending to ${mention.label} — @${mention.raw} is removed from the prompt.`;
    case "ambiguous":
      return `@${mention.raw} fits ${mention.labels.join(" and ")}, so it stays plain text. Pick one above.`;
    case "unavailable":
      return `@${mention.raw} is ${mention.label}, but ${mention.reason}, so it stays plain text.`;
    default:
      return handles.length === 0
        ? ""
        : `Start a message with ${handles
            .map((entry) => `@${entry.handle}`)
            .join(" or ")} to address one directly.`;
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
  sessionRef,
}: {
  channelId: string;
  isMember: boolean;
  publishLaneMessage: typeof publishCodingSessionLaneMessage;
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
      className="rounded-3xl border border-border/70 bg-background/95 p-3 shadow-lg backdrop-blur-xl"
      data-testid="coding-session-lane-composer"
    >
      {error ? <p className="mb-2 text-sm text-destructive">{error}</p> : null}
      <div className="flex items-end gap-2">
        <Textarea
          aria-label="Session conversation message"
          className="min-h-12 min-w-0 flex-1 resize-y border-0 bg-transparent shadow-none"
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
        <Button
          data-testid="coding-session-lane-send"
          disabled={!canSend}
          onClick={() => void submit()}
          size="sm"
          type="button"
        >
          {isSending ? "Sending…" : "Send"}
        </Button>
      </div>
      <p className="mt-1 px-1 text-2xs text-muted-foreground">
        Visible to the whole channel; scoped to this session for new clients.
      </p>
    </div>
  );
}
