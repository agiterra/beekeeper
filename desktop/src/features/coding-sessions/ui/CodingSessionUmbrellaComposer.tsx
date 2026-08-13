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
 */
export function CodingSessionUmbrellaComposer({
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
  });
  const [selectedKey, setSelectedKey] = React.useState<string | null>(() =>
    defaultCodingSessionUmbrellaParticipantKey(participants),
  );
  const [appliedPrefillId, setAppliedPrefillId] = React.useState<string | null>(
    null,
  );
  if (prefill && prefill.id !== appliedPrefillId) {
    setAppliedPrefillId(prefill.id);
    setSelectedKey(prefill.participantKey);
  }

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
                onClick={() => setSelectedKey(key)}
                title={gated ? (authority.reason ?? undefined) : undefined}
                type="button"
              >
                {participant.label}
              </button>
            );
          })}
        </fieldset>
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
          channelId={channelId}
          isMember={isMember}
          // Keyed by execution: the editor holds its draft in local state, so
          // without a fresh instance per participant a half-written prompt for
          // Claude would be sitting in the box — and would be *sent* — after
          // switching the selector to Codex.
          key={selected.executionKey}
          layout={layout}
          participant={selected}
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
  channelId,
  isMember,
  layout,
  participant,
  prefill,
}: {
  channelId: string;
  isMember: boolean;
  layout: "inline" | "stacked";
  participant: Extract<CodingSessionUmbrellaParticipant, { kind: "execution" }>;
  prefill: { id: string; text: string } | null;
}) {
  const record = participant.execution.activeGeneration;
  const target = record.commandTarget;
  const status = deriveCodingSessionWorkspaceStatus(record.transcript);
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
      canInterrupt={
        codingSessionTargetSupportsInterrupt(target) &&
        record.capabilities?.threadTurnInterrupt !== false
      }
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
      layout={layout}
      prefill={prefill}
      target={target}
      variant="floating"
    />
  );
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
