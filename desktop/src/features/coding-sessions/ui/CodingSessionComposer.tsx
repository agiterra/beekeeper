import * as React from "react";

import {
  buildCodingSessionTargetKey,
  createCodingSessionCommandId,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
  type CodingSessionCommandTarget,
  type CodingSessionTurnDelivery,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  clearCodingSessionDraftRecovery,
  forgetPendingCodingSessionTurn,
  markPendingCodingSessionTurnPublished,
  recordPendingCodingSessionTurn,
  useCodingSessionDraftRecovery,
} from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionResume,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  type CodingSessionSeatCustody,
  publishSeatedCodingSessionResume,
} from "@/features/coding-sessions/lib/codingSessionSeatedCreate";
import { buildCodingSessionResumeInput } from "@/features/coding-sessions/lib/codingSessionResumeSeat";
import { codingSessionResumeSeatDeps } from "@/features/coding-sessions/lib/codingSessionResumeSeatDeps";
import { useCodingSessionResumeSettle } from "@/features/coding-sessions/hooks/useCodingSessionResumeSettle";
import { useCodingSessionTurnRefusal } from "@/features/coding-sessions/hooks/useCodingSessionTurnRefusal";
import { useEndCodingSessionDialog } from "@/features/coding-sessions/hooks/useEndCodingSessionDialog";
import {
  IDLE_PROMPT_RECALL,
  stepPromptRecall,
  type PromptRecallState,
} from "@/features/coding-sessions/lib/codingSessionPromptHistory";
import {
  resolveCodingSessionReaddress,
  restoreCodingSessionDraft,
} from "@/features/coding-sessions/lib/codingSessionTurnRefusal";
import { buildCodingSessionExecutionKey } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionIngressClient } from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { Button } from "@/shared/ui/button";
import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { getCodingSessionComposerState } from "@/features/coding-sessions/lib/codingSessionComposerModel";
import {
  expandImageTokens,
  useCodingSessionImageAttachments,
  type CodingSessionAttachmentRef,
} from "@/features/coding-sessions/lib/useCodingSessionImageAttachments";
import type { CodingSessionComposerControlContext } from "./CodingSessionComposerDeck";
import { CodingSessionComposerSurface } from "./CodingSessionComposerSurface";

type CodingSessionComposerProps = {
  canInterrupt: boolean;
  canControl?: boolean;
  /**
   * Whether this operator may durably stop the provider execution. Defaults
   * to `canControl` for callers without separate founder authority evidence.
   */
  canStopExecution?: boolean;
  /** Authority evidence is unresolved; this is not a denied collaborator grant. */
  authorityUnresolved?: boolean;
  authorityReason?: string | null;
  /**
   * Whether this execution advertised native mid-turn steering (its 44223
   * `capabilities.threadSteer`). Defaults to `false`: a composer that has not
   * been told must not offer a control the provider would only degrade.
   */
  canSteer?: boolean;
  /**
   * Whether this execution advertised image prompts (its 44223
   * `capabilities.promptImage`). Defaults to `false` for the same reason
   * `canSteer` does: a composer that has not been told must not offer a
   * control the provider would only degrade — here, an image the agent would
   * never receive.
   */
  canAttachImages?: boolean;
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
  /**
   * The operator's own earlier prompts for this execution, newest first, for
   * ⌘↑/⌘↓ recall. Empty (the default) simply makes the chords inert.
   */
  promptHistory?: readonly string[];
  providerAuthorityPubkey?: string | null;
  /** Runtime slug, named in the disabled attach tooltip. */
  runtimeLabel?: string | null;
  /** Optional recipient picker rendered inside the immersive control deck. */
  recipientControl?: React.ReactNode;
  /** Publish seam; production passes nothing. */
  publishCommand?: typeof publishCodingSessionCommand;
  /** Refusal-ingress transport seam; production passes nothing. */
  refusalClient?: CodingSessionIngressClient;
  /** Resume publish seam; production passes nothing. */
  publishResume?: typeof publishCodingSessionResume;
  /**
   * The agent seated on this execution (its 44223 `agentRef`), or null when a
   * person created it.
   *
   * A reconnect spawns a *new* adapter process, and the provider consumed this
   * seat's host-local custody entry when it spawned the last one — so the
   * identity has to be staged again, under the resume's own `commandId`, or
   * the provider refuses the reconnect with `ACTOR_UNAVAILABLE`.
   */
  seatActorPubkey?: string | null;
  /**
   * The role that seat holds, from this generation's 44223. A resume stages
   * its own custody entry, and the host picks the pack by role — so without
   * this a resumed seat would be handed its actor's home-role pack.
   */
  seatRole?: string | null;
  /**
   * The project this execution is filed under, from its own 44223.
   *
   * A resume stages a fresh custody entry, and the host stages a seat's pack
   * from the project's kind:30624 when it is told which project (finding 84).
   * Without this a reconnect quietly restaged this computer's local copy in
   * place of the repository the project names — finding 85. `null` is a
   * standalone session, which stages the local copy on purpose.
   */
  projectRef?: string | null;
  /** Host-local seat custody seam; production passes nothing. */
  seatCustody?: CodingSessionSeatCustody;
  /** Display name for the stop-execution confirm; falls back to "this session". */
  sessionLabel?: string | null;
  target: CodingSessionCommandTarget;
  variant?: "panel" | "floating";
};

/** Reference-stable empty default; a fresh `[]` per render would churn effects. */
const EMPTY_PROMPT_HISTORY: readonly string[] = Object.freeze([]);

/**
 * Production custody seam: the real host-local staging calls, and the reader
 * that resolves the project's pack source
 * (`lib/codingSessionResumeSeatDeps.ts`). It used to be defined here without the
 * reader, which is half of finding 85 — a `projectRef` with nothing able to
 * read a 30624 for it resolves to `null` and stages the local copy anyway.
 */
const DEFAULT_SEAT_CUSTODY: CodingSessionSeatCustody =
  codingSessionResumeSeatDeps;

/** Composer for steering a selected governed coding-session generation. */
export function CodingSessionComposer({
  authorityReason = null,
  authorityUnresolved = false,
  canInterrupt,
  canControl = true,
  canStopExecution = canControl,
  canAttachImages = false,
  canSteer = false,
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
  promptHistory = EMPTY_PROMPT_HISTORY,
  providerAuthorityPubkey = null,
  recipientControl,
  runtimeLabel = null,
  publishCommand = publishCodingSessionCommand,
  publishResume = publishCodingSessionResume,
  refusalClient,
  seatActorPubkey = null,
  seatRole = null,
  projectRef = null,
  seatCustody = DEFAULT_SEAT_CUSTODY,
  sessionLabel = null,
  target,
  variant = "panel",
}: CodingSessionComposerProps) {
  const editorRef = React.useRef<HTMLTextAreaElement>(null);
  const [text, setText] = React.useState(prefill?.text ?? "");
  /**
   * Where ⌘↑/⌘↓ has walked to in this operator's earlier prompts.
   *
   * Declared beside the draft because every path that makes the draft the
   * person's again — a prefill, a restored refusal, typing, a send — has to
   * reset it, so the next ⌘↑ starts from the newest prompt instead of
   * resuming a walk from some earlier visit.
   *
   * Nothing renders from it, so the value slot stays empty: the cursor is read
   * only inside the updater, where it is guaranteed current.
   */
  const [, setRecall] = React.useState<PromptRecallState>(IDLE_PROMPT_RECALL);
  const [appliedPrefillId, setAppliedPrefillId] = React.useState<string | null>(
    prefill?.id ?? null,
  );
  if (prefill && prefill.id !== appliedPrefillId) {
    // Render-time state adjustment: a new prefill replaces the draft exactly
    // once, then the editor is the person's again.
    setAppliedPrefillId(prefill.id);
    setRecall(IDLE_PROMPT_RECALL);
    setText(prefill.text);
  }
  // Recovering the words of a turn whose delivery the provider could not
  // establish. Deliberately *not* the `prefill` path above: that one replaces
  // the draft, and this one must not — the person pressed a button on an old
  // message while a new one may be half-written, and eating it to return the
  // old one would lose more than it recovered. `restoreCodingSessionDraft` is
  // the same append the refusal path uses, including its "already there"
  // check for a second press.
  const draftRecovery = useCodingSessionDraftRecovery();
  const recoverable =
    draftRecovery !== null &&
    draftRecovery.channelId === channelId &&
    draftRecovery.targetKey === buildCodingSessionTargetKey(target)
      ? draftRecovery
      : null;
  React.useEffect(() => {
    if (recoverable === null) return;
    setRecall(IDLE_PROMPT_RECALL);
    setText((current) => restoreCodingSessionDraft(current, recoverable.text));
    clearCodingSessionDraftRecovery(recoverable.id);
    editorRef.current?.focus();
  }, [recoverable]);
  /**
   * Write an image token where the person is typing.
   *
   * The caret is what puts the picture in the right place, so a turn reads
   * *"when I do X I see this: [Image #1]"* rather than prose with a tray of
   * images bolted underneath. The token is spaced into the sentence rather
   * than forced onto its own line: it is a reference, and people put
   * references mid-sentence.
   */
  const insertAtCaret = React.useCallback((token: string) => {
    setText((current) => {
      const editor = editorRef.current;
      // Caret when the editor has focus; end of the draft otherwise — the
      // paperclip takes focus away, and a dropped file has no caret at all.
      const at =
        editor && document.activeElement === editor
          ? editor.selectionStart
          : current.length;
      const before = current.slice(0, at);
      const after = current.slice(at);
      const lead = before.length === 0 || /\s$/.test(before) ? "" : " ";
      const trail = after.length === 0 || /^\s/.test(after) ? "" : " ";
      return `${before}${lead}${token}${trail}${after}`;
    });
  }, []);

  const transformDraft = React.useCallback(
    (transform: (draft: string) => string) => setText(transform),
    [],
  );

  const attachments = useCodingSessionImageAttachments({
    enabled: canAttachImages,
    onInsertAtCaret: insertAtCaret,
    onTransformDraft: transformDraft,
  });
  const [pendingAction, setPendingAction] = React.useState<
    "send" | "interrupt" | "resume" | "stop" | null
  >(null);
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
      setRecall(IDLE_PROMPT_RECALL);
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
  // The execution behind the current target, generation excluded: the identity
  // that a resume preserves and a participant switch does not.
  const executionKey = buildCodingSessionExecutionKey(
    providerAuthorityPubkey,
    target,
  );
  const {
    clearReaddress,
    error: turnRefusalError,
    readdress,
    watch: watchTurn,
    watcher: turnRefusalWatcher,
  } = useCodingSessionTurnRefusal({
    channelId,
    client: refusalClient,
    executionKey,
    providerAuthorityPubkey,
    restoreDraft: restoreRefusedDraft,
    // The generation a re-armed watch belongs to; see the option's doc.
    targetGeneration: target.generation,
    // Turns this execution's provider is still holding are adopted on mount:
    // a queued turn outlives this component, and its terminal receipt has to
    // land on something.
    targetKey: buildCodingSessionTargetKey(target),
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
  // `[Image #N]` is what the person reads and keeps editing; the markdown it
  // becomes is what the relay stores and the transcript renders as a picture.
  // Expanding here — and never in the draft — is what keeps both true.
  const preparedText = expandImageTokens(
    (prepareText ? prepareText(text) : text).trim(),
    attachments.attachments,
  );
  const state = getCodingSessionComposerState({
    canSteer,
    hasUnsettledAttachments: attachments.isUploading || attachments.hasFailed,
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
  // Every sendable draft is publishable, working provider or not. What
  // changes mid-turn is the *delivery class* the command asks for, which the
  // provider then answers with its own signed receipt — a turn is never held
  // in this client's memory waiting for a moment that a crash would erase.
  const canSubmitText = canControl && !isUnavailable && state.canSend;
  const editorDisabled = !canControl || !isMember || isSending || isUnavailable;

  const publishPreparedText = React.useCallback(
    async ({
      attachmentRefs,
      deliver,
      draft,
      preparedText: textToPublish,
    }: {
      attachmentRefs: CodingSessionAttachmentRef[];
      deliver: CodingSessionTurnDelivery;
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
        // The raw draft rides the row, not just the watcher: a composer that
        // remounts while the provider still holds this turn has to be able to
        // hand these exact words back if it is dropped.
        draft,
        operatorPubkey: currentUserPubkey,
        recordedAt: Date.now(),
        published: false,
        // So a later recovery can say what it is not bringing back: the words
        // come home, the pictures do not.
        attachmentCount: attachmentRefs.length,
      });
      setPendingAction("send");
      setError(null);
      try {
        const published = await publishCommand({
          channelId,
          commandId,
          target,
          text: textToPublish,
          // Omitted at its default by the builder, so a turn with no images
          // is byte-identical to what this client has always published.
          attachments: attachmentRefs,
          deliver,
        });
        markPendingCodingSessionTurnPublished(channelId, published.commandId);
        // Acceptance by the relay is not consent from the provider. The receipt
        // that refuses this turn is keyed to this command id and nothing else.
        watchTurn({
          commandId: published.commandId,
          draft,
          executionKey,
          generation: target.generation,
        });
        // Cleared only once the relay has the turn. Clearing beside the
        // editor would drop the thumbnails on a publish that then failed,
        // leaving the restored draft describing images no longer attached.
        attachments.clear();
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
      attachments,
      channelId,
      currentUserPubkey,
      executionKey,
      isSending,
      publishCommand,
      restoreRefusedDraft,
      target,
      watchTurn,
    ],
  );

  /**
   * Publish the draft, in the delivery class the person actually asked for.
   *
   * `"primary"` is the button under the cursor by default: steer where the
   * execution advertised it and a turn is running, an ordinary boundary send
   * everywhere else. `"boundary"` is the explicit second choice — "not into
   * this turn; after it" — and it stays a boundary send even where steering
   * is available, which is the whole point of offering it.
   *
   * The class is resolved here, from this render's `isWorking`/`canSteer` and
   * against this render's `target`, rather than carried from whenever a menu
   * was opened. A capability that arrives late or a target that changes under
   * an open menu re-creates this callback, so the press that follows cannot
   * publish a class the execution in front of the person no longer offers.
   */
  const submit = React.useCallback(
    async (intent: "primary" | "boundary" = "primary") => {
      if (!canSubmitText || isSending) return;
      // Steering is asked for only where the execution advertised it, there is
      // a turn to steer, and the person did not ask for the boundary instead.
      // The provider may still downgrade a steer, and says so in a receipt.
      const deliver: CodingSessionTurnDelivery =
        intent === "primary" && isWorking && canSteer ? "steer" : "boundary";
      // Keep the person's own words, not the prepared wire text: a refusal has
      // to hand back exactly what they typed, routing handle and all.
      const draft = text;
      const attachmentRefs = attachments.attachmentRefs;
      setRecall(IDLE_PROMPT_RECALL);
      setText("");
      await publishPreparedText({
        attachmentRefs,
        deliver,
        draft,
        preparedText,
      });
    },
    [
      attachments.attachmentRefs,
      canSteer,
      canSubmitText,
      isSending,
      isWorking,
      preparedText,
      publishPreparedText,
      text,
    ],
  );

  /** ⌘↑/⌘↓: walk back and forward through the earlier prompts. */
  const recallHistory = React.useCallback(
    (direction: "older" | "newer") => {
      setRecall((current) => {
        const step = stepPromptRecall(direction, current, promptHistory, text);
        if (step.text !== null) {
          setText(step.text);
          // Caret to the end: recall is for editing the prompt, and landing at
          // character zero means every recall starts with a trip to the end.
          const editor = editorRef.current;
          if (editor) {
            requestAnimationFrame(() => {
              editor.setSelectionRange(
                editor.value.length,
                editor.value.length,
              );
            });
          }
        }
        return step.state;
      });
    },
    [promptHistory, text],
  );
  const handleTextChange = React.useCallback((next: string) => {
    setRecall(IDLE_PROMPT_RECALL);
    setText(next);
  }, []);

  // A turn the provider answered with `NO_LIVE_EXECUTION` or
  // `STALE_GENERATION` never ran, and nothing but the sender can decide it
  // still applies to the session that came back (ruling R1). The words are
  // already back in the editor; this says whether there is anywhere to send
  // them and, when there is, sends them there.
  const readdressOffer =
    readdress && readdress.executionKey === executionKey
      ? resolveCodingSessionReaddress({
          currentGeneration: target.generation,
          isEnded,
          refusedGeneration: readdress.generation,
        })
      : null;

  const resendToCurrentGeneration = React.useCallback(async () => {
    if (!canSubmitText || isSending) return;
    // Deliberately the editor's text, not a copy of the refused wire text:
    // the refusal put those words back where the person could change them,
    // and re-sending anything else would publish a message they can see on
    // screen and did not agree to.
    const draft = text;
    setText("");
    clearReaddress();
    // Never `steer`: the execution that answered has just been resumed, so
    // there is no running turn of its to steer into.
    // The refusal restored the draft, and the thumbnails were never cleared
    // (the turn was refused, not delivered), so whatever is still staged rides
    // the resend — the same words *and* the same pictures the person is
    // looking at.
    await publishPreparedText({
      attachmentRefs: attachments.attachmentRefs,
      deliver: "boundary",
      draft,
      preparedText,
    });
  }, [
    attachments.attachmentRefs,
    canSubmitText,
    clearReaddress,
    isSending,
    preparedText,
    publishPreparedText,
    text,
  ]);

  const readdressAction =
    readdressOffer === null ? null : readdressOffer.kind === "offer" ? (
      <div className="flex items-center gap-2">
        <Button
          data-testid="coding-session-composer-readdress"
          disabled={!canSubmitText || isSending}
          onClick={() => void resendToCurrentGeneration()}
          size="sm"
          type="button"
        >
          {readdressOffer.label}
        </Button>
        <span
          className="text-2xs text-muted-foreground"
          data-testid="coding-session-composer-readdress-generation"
        >
          {`generation ${readdressOffer.generation}`}
        </span>
      </div>
    ) : (
      <p
        className="text-sm text-muted-foreground"
        data-testid="coding-session-composer-readdress-unavailable"
      >
        {readdressOffer.reason}
      </p>
    );

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
      // A seated execution's identity is staged under this resume's own
      // `commandId` before the 44221 goes out. Unseated executions take the
      // publish path unchanged — no custody write at all.
      await publishSeatedCodingSessionResume(
        buildCodingSessionResumeInput({
          commandId,
          seat: {
            actorPubkey: seatActorPubkey,
            role: seatRole,
            projectRef,
          },
          deps: seatCustody,
          publish: () =>
            publishResume({
              channelId,
              commandId,
              target,
              providerAuthorityPubkey,
            }),
        }),
      );
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
    projectRef,
    providerAuthorityPubkey,
    publishResume,
    seatActorPubkey,
    seatCustody,
    seatRole,
    target,
  ]);

  // A durable execution stop is distinct from closing the umbrella session.
  // Route it through a confirm instead of publishing on the raw click.
  const endDialog = useEndCodingSessionDialog();
  const canSessionStop =
    canStopExecution &&
    isMember &&
    providerAuthorityPubkey !== null &&
    !isEnded;
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
        attachments={attachments}
        authorityReason={authorityReason}
        authorityUnresolved={authorityUnresolved}
        canAttachImages={canAttachImages}
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
        errorAction={readdressAction}
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
        onRecallHistory={promptHistory.length > 0 ? recallHistory : undefined}
        runtimeLabel={runtimeLabel}
        deliveryHint={state.deliveryHint}
        onInterrupt={() => void handleStop()}
        onPrimary={() => void submit("primary")}
        onQueueNext={() => void submit("boundary")}
        onReconnect={() => void handleResume()}
        onSessionStop={requestSessionEnd}
        onTextChange={handleTextChange}
        pendingAction={pendingAction}
        providerAuthorityPubkey={providerAuthorityPubkey}
        recipientControl={recipientControl}
        secondaryLabel={state.secondaryLabel}
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
