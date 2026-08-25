import * as React from "react";

import {
  buildCodingSessionTargetKey,
  createCodingSessionCommandId,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
  type CodingSessionCommandTarget,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  forgetPendingCodingSessionTurn,
  markPendingCodingSessionTurnPublished,
  recordPendingCodingSessionTurn,
} from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionResume,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { useCodingSessionResumeSettle } from "@/features/coding-sessions/hooks/useCodingSessionResumeSettle";
import { useCodingSessionTurnRefusal } from "@/features/coding-sessions/hooks/useCodingSessionTurnRefusal";
import { useEndCodingSessionDialog } from "@/features/coding-sessions/hooks/useEndCodingSessionDialog";
import { restoreCodingSessionDraft } from "@/features/coding-sessions/lib/codingSessionTurnRefusal";
import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { getCodingSessionComposerState } from "@/features/coding-sessions/lib/codingSessionComposerModel";
import type { CodingSessionComposerControlContext } from "./CodingSessionComposerDeck";
import { CodingSessionComposerSurface } from "./CodingSessionComposerSurface";

type CodingSessionComposerProps = {
  canInterrupt: boolean;
  canControl?: boolean;
  authorityReason?: string | null;
  canSteer?: boolean;
  channelId: string;
  contextWindow?: CodingSessionContextWindow | null;
  controlContext?: CodingSessionComposerControlContext;
  /**
   * The signed-in identity, stamped onto the optimistic row this composer shows
   * while a sent turn waits for the provider's echo. `null` (the default) just
   * means the row cannot tell two operators' identical messages apart; it never
   * blocks sending.
   */
  currentUserPubkey?: string | null;
  immersive?: boolean;
  isMember: boolean;
  isWorking: boolean;
  isUngovernedSession?: boolean;
  lifecycleStatus?: CodingSessionStatus;
  layout?: "inline" | "stacked";
  /**
   * Observe the live draft. The umbrella composer uses this to follow a typed
   * `@handle` with the participant selector; nothing here depends on it.
   */
  /**
   * Opens the add-provider flow. A stopped execution is deliberately not
   * resumable, so the only honest way forward is another provider — the banner
   * that says the execution ended offers it rather than leaving the operator
   * hunting for a Resume button that will never exist (§2 item 42).
   */
  onAddProvider?: () => void;
  onTextChange?: (text: string) => void;
  /**
   * Last transform applied to the draft before it is published — the umbrella
   * composer strips a leading `@handle` that already did its routing. The
   * *prepared* text is what gates Send, so a message that prepares to nothing
   * (a bare handle) is not sendable rather than sent empty.
   */
  prepareText?: (text: string) => string;
  /**
   * Editable text staged into the editor (e.g. a handoff provenance block).
   * Applied once per `id`; the person keeps full control of the text after.
   */
  prefill?: { id: string; text: string } | null;
  providerAuthorityPubkey?: string | null;
  /** Publish seam; production passes nothing. */
  publishCommand?: typeof publishCodingSessionCommand;
  /** Display name for the stop-execution confirm; falls back to "this session". */
  sessionLabel?: string | null;
  target: CodingSessionCommandTarget;
  variant?: "panel" | "floating";
};

/** Composer for steering a selected governed coding-session generation. */
export function CodingSessionComposer({
  authorityReason = null,
  canInterrupt,
  canControl = true,
  canSteer = true,
  channelId,
  contextWindow = null,
  controlContext,
  currentUserPubkey = null,
  immersive = false,
  isMember,
  isWorking,
  isUngovernedSession = false,
  lifecycleStatus,
  layout = "inline",
  onAddProvider,
  onTextChange,
  prepareText,
  prefill = null,
  providerAuthorityPubkey = null,
  publishCommand = publishCodingSessionCommand,
  sessionLabel = null,
  target,
  variant = "panel",
}: CodingSessionComposerProps) {
  const editorRef = React.useRef<HTMLTextAreaElement>(null);
  const [text, setText] = React.useState(prefill?.text ?? "");
  const [appliedPrefillId, setAppliedPrefillId] = React.useState<string | null>(
    prefill?.id ?? null,
  );
  if (prefill && prefill.id !== appliedPrefillId) {
    // Render-time state adjustment: a new prefill replaces the draft exactly
    // once, then the editor is the person's again.
    setAppliedPrefillId(prefill.id);
    setText(prefill.text);
  }
  const [pendingAction, setPendingAction] = React.useState<
    "send" | "interrupt" | "resume" | "stop" | null
  >(null);
  const [queuedDraft, setQueuedDraft] = React.useState<{
    draft: string;
    preparedText: string;
  } | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  // A reconnect is only finished when the provider's receipt names the
  // generation it resumed into; until then this composer is as busy as it is
  // during a turn.
  const {
    begin: beginResume,
    error: resumeError,
    fail: failResume,
    isPending: isResuming,
    watcher: resumeWatcher,
  } = useCodingSessionResumeSettle({
    channelId,
    providerAuthorityPubkey,
  });
  const restoreRefusedDraft = React.useCallback(
    (refused: string, refusedCommandId?: string) => {
      setText((current) => restoreCodingSessionDraft(current, refused));
      // The optimistic row for this turn is waiting on an echo that a refusal
      // guarantees will never come; the words are back in the editor instead.
      if (refusedCommandId) {
        forgetPendingCodingSessionTurn(channelId, refusedCommandId);
      }
    },
    [channelId],
  );
  // A sent turn the provider refuses (an operator it has not granted) answers
  // with a receipt and nothing else; without this watch the message the person
  // typed disappears with no explanation at all.
  const {
    error: turnRefusalError,
    watch: watchTurn,
    watcher: turnRefusalWatcher,
  } = useCodingSessionTurnRefusal({
    channelId,
    providerAuthorityPubkey,
    restoreDraft: restoreRefusedDraft,
  });
  const isSending = pendingAction !== null || isResuming;
  const visibleError = error ?? resumeError ?? turnRefusalError;
  React.useEffect(() => {
    onTextChange?.(text);
  }, [onTextChange, text]);
  React.useLayoutEffect(() => {
    if (!immersive || !editorRef.current) return;
    const editor = editorRef.current;
    const rootSize = Number.parseFloat(
      window.getComputedStyle(document.documentElement).fontSize,
    );
    const maxHeight = (Number.isFinite(rootSize) ? rootSize : 16) * 12;
    editor.style.height = "auto";
    editor.style.height = `${Math.min(editor.scrollHeight, maxHeight)}px`;
    editor.style.overflowY =
      editor.scrollHeight > maxHeight ? "auto" : "hidden";
  });
  const preparedText = (prepareText ? prepareText(text) : text).trim();
  const state = getCodingSessionComposerState({
    isMember,
    isWorking,
    text: preparedText,
  });
  const isDisconnected = lifecycleStatus === "disconnected";
  const isEnded = lifecycleStatus === "stopped";
  // Coordination, not the newest report: a provider whose lease has lapsed
  // answers nothing, however recently it said `idle` (§2 item 41). Sending
  // into that is how three stops sat unanswered for two hours.
  const unreachableStatus =
    controlContext?.status.kind === "unknown" &&
    controlContext.status.attention === "unreachable"
      ? controlContext.status
      : null;
  const unreachableDetail =
    unreachableStatus === null
      ? null
      : codingSessionWorkspaceStatusDetail(unreachableStatus);
  const isUnavailable = isDisconnected || isEnded || unreachableStatus !== null;
  const canPublishText =
    canControl &&
    !isUnavailable &&
    state.canSend &&
    (!immersive || !isWorking || canSteer);
  const canQueueText =
    canControl &&
    !isUnavailable &&
    state.canSend &&
    immersive &&
    isWorking &&
    !canSteer &&
    queuedDraft === null;
  const canSubmitText = canPublishText || canQueueText;
  const editorDisabled = !canControl || !isMember || isSending || isUnavailable;

  const publishPreparedText = React.useCallback(
    async ({
      draft,
      preparedText: textToPublish,
    }: {
      draft: string;
      preparedText: string;
    }) => {
      if (isSending) return;
      const commandId = createCodingSessionCommandId();
      // Clear and record *before* awaiting the relay. Neither the empty editor
      // nor the pending row is a claim about delivery — the row says "Sending…"
      // until the relay answers — and both are undone below if the publish
      // fails, which is the only outcome where the words were never sent.
      recordPendingCodingSessionTurn({
        channelId,
        targetKey: buildCodingSessionTargetKey(target),
        commandId,
        text: textToPublish,
        operatorPubkey: currentUserPubkey,
        recordedAt: Date.now(),
        published: false,
      });
      setPendingAction("send");
      setError(null);
      try {
        const published = await publishCommand({
          channelId,
          commandId,
          target,
          text: textToPublish,
        });
        markPendingCodingSessionTurnPublished(channelId, published.commandId);
        // Acceptance by the relay is not consent from the provider. The receipt
        // that refuses this turn is keyed to this command id and nothing else.
        watchTurn({ commandId: published.commandId, draft });
      } catch (submitError) {
        forgetPendingCodingSessionTurn(channelId, commandId);
        // The words never left this machine, so they belong back in the editor —
        // same rule the refusal path already follows, including its handling of
        // a person who has started typing again.
        restoreRefusedDraft(draft);
        setError(
          submitError instanceof Error
            ? submitError.message
            : "Unable to send coding-session command.",
        );
      } finally {
        setPendingAction(null);
      }
    },
    [
      channelId,
      currentUserPubkey,
      isSending,
      publishCommand,
      restoreRefusedDraft,
      target,
      watchTurn,
    ],
  );

  const submit = React.useCallback(async () => {
    if (!canPublishText || isSending) return;
    // Keep the person's own words, not the prepared wire text: a refusal has
    // to hand back exactly what they typed, routing handle and all.
    const draft = text;
    setText("");
    await publishPreparedText({ draft, preparedText });
  }, [canPublishText, isSending, preparedText, publishPreparedText, text]);

  const queueNextTurn = React.useCallback(() => {
    if (!canQueueText) return;
    setQueuedDraft({ draft: text, preparedText });
    setText("");
  }, [canQueueText, preparedText, text]);

  React.useEffect(() => {
    if (
      isWorking ||
      isUnavailable ||
      isSending ||
      !canControl ||
      !isMember ||
      queuedDraft === null
    )
      return;
    const next = queuedDraft;
    setQueuedDraft(null);
    void publishPreparedText(next);
  }, [
    isSending,
    isUnavailable,
    isWorking,
    canControl,
    isMember,
    publishPreparedText,
    queuedDraft,
  ]);

  const handlePrimaryAction = React.useCallback(async () => {
    if (canQueueText) {
      queueNextTurn();
      return;
    }
    await submit();
  }, [canQueueText, queueNextTurn, submit]);

  const handleStop = React.useCallback(async () => {
    if (!canControl || !isMember || !canInterrupt || isSending) return;
    setPendingAction("interrupt");
    setError(null);
    try {
      await publishCodingSessionInterrupt({
        channelId,
        commandId: createCodingSessionCommandId(),
        target,
      });
    } catch (interruptError) {
      setError(
        interruptError instanceof Error
          ? interruptError.message
          : "Unable to interrupt coding-session turn.",
      );
    } finally {
      setPendingAction(null);
    }
  }, [canControl, canInterrupt, channelId, isMember, isSending, target]);

  const handleResume = React.useCallback(async () => {
    if (
      !canControl ||
      !isMember ||
      !isDisconnected ||
      isSending ||
      !providerAuthorityPubkey
    ) {
      return;
    }
    setError(null);
    // The commandId is the only handle on the receipt that answers this
    // resume, so it is kept rather than discarded, and the wait is armed
    // *before* the publish — the settle watcher owns the pending state from
    // here on.
    const commandId = createCodingSessionLifecycleCommandId();
    beginResume(commandId);
    try {
      await publishCodingSessionResume({
        channelId,
        commandId,
        target,
        providerAuthorityPubkey,
      });
    } catch (publishError) {
      failResume(
        publishError instanceof Error
          ? publishError.message
          : "Unable to reconnect the coding session.",
      );
    }
  }, [
    beginResume,
    channelId,
    canControl,
    failResume,
    isDisconnected,
    isMember,
    isSending,
    providerAuthorityPubkey,
    target,
  ]);

  // A durable execution stop is distinct from closing the umbrella session.
  // Route it through a confirm instead of publishing on the raw click.
  const endDialog = useEndCodingSessionDialog();
  const canSessionStop =
    canControl && isMember && providerAuthorityPubkey !== null && !isEnded;
  const requestSessionEnd = React.useCallback(() => {
    if (!canSessionStop || !providerAuthorityPubkey) return;
    endDialog.requestEnd({
      label: sessionLabel?.trim() || "this session",
      channelId,
      stops: [{ target, providerAuthorityPubkey }],
      providerUnanswered: unreachableStatus !== null,
    });
  }, [
    canSessionStop,
    channelId,
    endDialog,
    providerAuthorityPubkey,
    sessionLabel,
    target,
    unreachableStatus,
  ]);

  return (
    <>
      <CodingSessionComposerSurface
        authorityReason={authorityReason}
        canControl={canControl}
        canInterrupt={canInterrupt}
        canSessionStop={canSessionStop}
        canSteer={canSteer}
        canSubmitText={canSubmitText}
        context={controlContext}
        contextWindow={contextWindow}
        editorDisabled={editorDisabled}
        editorRef={editorRef}
        error={visibleError}
        immersive={immersive}
        isDisconnected={isDisconnected}
        isEnded={isEnded}
        isMember={isMember}
        isResuming={isResuming}
        isSending={isSending}
        isUnavailable={isUnavailable}
        isUngovernedSession={isUngovernedSession}
        isWorking={isWorking}
        layout={layout}
        onAddProvider={onAddProvider}
        onCancelQueued={() => {
          if (!queuedDraft) return;
          setQueuedDraft(null);
          restoreRefusedDraft(queuedDraft.draft);
        }}
        onInterrupt={() => void handleStop()}
        onPrimary={() => void handlePrimaryAction()}
        onReconnect={() => void handleResume()}
        onSessionStop={requestSessionEnd}
        onTextChange={setText}
        pendingAction={pendingAction}
        providerAuthorityPubkey={providerAuthorityPubkey}
        queuedDraft={queuedDraft?.draft ?? null}
        sendLabel={state.sendLabel}
        showAuthorityFailure={state.showAuthorityFailure}
        showStopAction={state.showStopAction}
        text={text}
        unreachable={unreachableStatus !== null}
        unreachableDetail={unreachableDetail}
        variant={variant}
      />
      {endDialog.dialog}
      {resumeWatcher}
      {turnRefusalWatcher}
    </>
  );
}
