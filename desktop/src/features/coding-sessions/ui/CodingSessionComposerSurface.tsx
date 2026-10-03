import type * as React from "react";

import {
  type CodingSessionChannelAccess,
  type CodingSessionChannelAccessCopy,
  codingSessionChannelAccessAllowsSend,
  describeCodingSessionChannelAccess,
} from "@/features/coding-sessions/lib/codingSessionChannelAccess";
import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import {
  matchCodingSessionHistoryKey,
  shouldSubmitCodingSessionComposerKey,
} from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import type { CodingSessionAttachmentController } from "@/features/coding-sessions/lib/useCodingSessionTurnAttachments";
import { CodingSessionComposerAttachments } from "./CodingSessionComposerAttachments";
import {
  useCodingSessionComposerRecipient,
  useCodingSessionMissionLens,
} from "./CodingSessionUmbrellaWorkspaceModel";
import {
  CodingSessionComposerDeck,
  type CodingSessionComposerControlContext,
} from "./CodingSessionComposerDeck";

type ComposerSurfaceProps = {
  /**
   * Staged image attachments. Absent means this surface offers no attach
   * control at all — the lane composer, for instance, is people-to-people.
   */
  attachments?: CodingSessionAttachmentController;
  authorityReason: string | null;
  authorityUnresolved: boolean;
  /** This execution's advertised `promptImage` capability. */
  canAttachImages?: boolean;
  canControl: boolean;
  canInterrupt: boolean;
  canSessionStop: boolean;
  canSteer: boolean;
  canSubmitText: boolean;
  context: CodingSessionComposerControlContext | undefined;
  contextWindow: CodingSessionContextWindow | null;
  editorDisabled: boolean;
  editorRef: React.RefObject<HTMLTextAreaElement | null>;
  error: string | null;
  /**
   * Rendered directly under the error line. The composer puts the owed-turn
   * resend here: the offer belongs beside the sentence that explains why the
   * turn never ran, not in a menu the person would have to go looking for.
   */
  errorAction?: React.ReactNode;
  immersive: boolean;
  isDisconnected: boolean;
  isEnded: boolean;
  /** Channel write access; see `lib/codingSessionChannelAccess.ts`. */
  channelAccess: CodingSessionChannelAccess;
  isResuming: boolean;
  isSending: boolean;
  isUnavailable: boolean;
  isUngovernedSession: boolean;
  isWorking: boolean;
  layout: "inline" | "stacked";
  /**
   * One line naming what the mid-turn controls will do, or `null` when the
   * execution is idle and Send means only what it says.
   */
  deliveryHint: string | null;
  onAddProvider?: () => void;
  onInterrupt: () => void;
  onPrimary: () => void;
  /**
   * Publish explicitly at the next turn boundary, whatever the primary would
   * have done. Rendered only where {@link secondaryLabel} is set.
   */
  onQueueNext: () => void;
  /** Walk this operator's prompt history; absent when there is none. */
  onRecallHistory?: (direction: "older" | "newer") => void;
  onReconnect: () => void;
  onSessionStop: () => void;
  onTextChange: (text: string) => void;
  pendingAction: "send" | "interrupt" | "resume" | "stop" | null;
  providerAuthorityPubkey: string | null;
  recipientControl?: React.ReactNode;
  /** Runtime slug named in the disabled attach tooltip. */
  runtimeLabel?: string | null;
  /**
   * The second delivery choice, or `null` when there is only one. Set only
   * where the primary steers, because an execution that cannot steer already
   * queues from its primary button.
   */
  secondaryLabel: string | null;
  sendLabel: string;
  showAuthorityFailure: boolean;
  showStopAction: boolean;
  text: string;
  unreachable: boolean;
  unreachableDetail: string | null;
  variant: "panel" | "floating";
};

/** Visual composer shell; command semantics remain in CodingSessionComposer. */
export function CodingSessionComposerSurface({
  attachments,
  authorityReason,
  authorityUnresolved,
  canAttachImages = false,
  canControl,
  canInterrupt,
  canSessionStop,
  canSteer,
  canSubmitText,
  context,
  contextWindow,
  editorDisabled,
  editorRef,
  error,
  errorAction = null,
  immersive,
  isDisconnected,
  isEnded,
  channelAccess,
  isResuming,
  isSending,
  isUnavailable,
  isUngovernedSession,
  isWorking,
  layout,
  deliveryHint,
  onAddProvider,
  onInterrupt,
  runtimeLabel,
  onPrimary,
  onQueueNext,
  onRecallHistory,
  onReconnect,
  onSessionStop,
  onTextChange,
  pendingAction,
  providerAuthorityPubkey,
  recipientControl,
  secondaryLabel,
  sendLabel,
  showAuthorityFailure,
  showStopAction,
  text,
  unreachable,
  unreachableDetail,
  variant,
}: ComposerSurfaceProps) {
  const mission = useCodingSessionMissionLens();
  const recipientLabel = useCodingSessionComposerRecipient();
  const canWriteChannel = codingSessionChannelAccessAllowsSend(channelAccess);
  const accessCopy = describeCodingSessionChannelAccess(channelAccess);
  return (
    <div
      className={cn(
        immersive
          ? "relative"
          : variant === "floating"
            ? "rounded-3xl border border-border/70 bg-background/95 p-3 shadow-lg backdrop-blur-xl"
            : "border-t border-border/70 bg-background px-3 py-3",
      )}
      data-layout={layout}
      data-mode={immersive ? "immersive" : "compact"}
      data-testid="coding-session-composer"
    >
      {immersive ? (
        <ComposerLifecycleNotice
          authorityReason={authorityReason}
          canControl={canControl}
          canSessionStop={canSessionStop}
          deliveryHint={deliveryHint}
          error={error}
          errorAction={errorAction}
          isDisconnected={isDisconnected}
          isEnded={isEnded}
          accessCopy={accessCopy}
          isResuming={isResuming}
          isSending={isSending}
          onAddProvider={onAddProvider}
          onReconnect={onReconnect}
          onSessionStop={onSessionStop}
          providerAuthorityPubkey={providerAuthorityPubkey}
          // A7 is a Mission ruling, and I8 freezes Conversation's DOM: the
          // masked outerHTML diff against the base caught this sentence
          // changing on the Conversation lens too, because the recipient
          // context wraps the whole workspace body rather than one lens.
          // Conversation keeps `for this execution`, byte for byte.
          recipientLabel={mission ? recipientLabel : null}
          unreachable={unreachable}
          unreachableDetail={unreachableDetail}
        />
      ) : (
        <CompactComposerNotices
          accessCopy={accessCopy}
          authorityReason={authorityReason}
          canControl={canControl}
          error={error}
          errorAction={errorAction}
          isUngovernedSession={isUngovernedSession}
          showAuthorityFailure={showAuthorityFailure}
        />
      )}
      {immersive && !canControl && authorityUnresolved ? (
        <p
          className="px-4 py-2 text-sm text-muted-foreground"
          data-testid="coding-session-composer-authority-unresolved"
        >
          {authorityReason ?? "Session access is unresolved."}
        </p>
      ) : null}
      {attachments && !immersive ? (
        <CodingSessionComposerAttachments
          canAttach={canAttachImages}
          controller={attachments}
          disabled={editorDisabled}
          runtimeLabel={runtimeLabel}
        />
      ) : null}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: pointer-only drop target; the Image button is the keyboard-accessible path */}
      <div
        className={cn(
          immersive
            ? "relative z-10 overflow-hidden rounded-3xl border border-border/35 bg-muted/35 shadow-[0_18px_60px_-24px_rgba(0,0,0,0.75)]"
            : "flex gap-2",
          !immersive && (layout === "stacked" ? "flex-col" : "items-end"),
          attachments?.isDragOver && "ring-2 ring-primary/60",
        )}
        onDragEnter={attachments?.handleDragEnter}
        onDragLeave={attachments?.handleDragLeave}
        onDragOver={attachments?.handleDragOver}
        onDrop={attachments?.handleDrop}
      >
        {/* Immersive stacks its children, so the strip belongs inside the
            card. The compact container is a flex row — putting it there would
            seat the thumbnails beside the textarea, so it goes above instead
            (rendered below the container's closing tag). */}
        {attachments && immersive ? (
          <CodingSessionComposerAttachments
            canAttach={canAttachImages}
            controller={attachments}
            disabled={editorDisabled}
            runtimeLabel={runtimeLabel}
          />
        ) : null}
        <Textarea
          aria-label="Coding-session instruction"
          className={cn(
            "min-h-16 min-w-0 flex-1 resize-y text-foreground caret-primary",
            // SESSION_VIEW_UX_PLAN L4: compact at rest. Conversation opens at
            // about two lines (it was four: a 6 rem box over an empty draft)
            // and grows with the text — `CodingSessionComposer`'s auto-grow and
            // its 12-rem ceiling are unchanged. Mission's job is watching, so
            // it still opens at one line (B4).
            immersive &&
              "block min-h-16 w-full resize-none rounded-none border-0 bg-transparent px-4 pt-3 pb-1 shadow-none focus-visible:ring-0",
            immersive && mission && "min-h-11",
            !immersive &&
              variant === "floating" &&
              "border-0 bg-transparent shadow-none",
          )}
          disabled={editorDisabled}
          onChange={(event) => onTextChange(event.target.value)}
          onPaste={attachments?.handlePaste}
          onKeyDown={(event) => {
            // History recall is checked first: ⌘↑/⌘↓ carry no submit meaning,
            // and on macOS an unclaimed one jumps the caret to the start or
            // end of the textarea instead.
            const recall = onRecallHistory
              ? matchCodingSessionHistoryKey(event)
              : null;
            if (recall) {
              event.preventDefault();
              onRecallHistory?.(recall);
              return;
            }
            if (!shouldSubmitCodingSessionComposerKey(event)) return;
            event.preventDefault();
            if (canSubmitText) onPrimary();
          }}
          placeholder={composerPlaceholder({
            authorityReason,
            authorityUnresolved,
            canControl,
            canSteer,
            isDisconnected,
            isEnded,
            accessCopy,
            isWorking,
          })}
          ref={editorRef}
          value={text}
        />
        {!immersive ? (
          <CompactComposerActions
            canControl={canControl}
            canInterrupt={canInterrupt}
            canSessionStop={canSessionStop}
            canSubmitText={canSubmitText}
            deliveryHint={deliveryHint}
            isDisconnected={isDisconnected}
            canWriteChannel={canWriteChannel}
            isSending={isSending}
            layout={layout}
            onInterrupt={onInterrupt}
            onPrimary={onPrimary}
            onQueueNext={onQueueNext}
            onSessionStop={onSessionStop}
            secondaryLabel={secondaryLabel}
            sendLabel={sendLabel}
            showStopAction={showStopAction}
          />
        ) : (
          <CodingSessionComposerDeck
            authorityReason={authorityReason}
            authorityUnresolved={authorityUnresolved}
            canControl={canControl}
            canInterrupt={canInterrupt}
            canSessionStop={canSessionStop}
            canSteer={canSteer}
            context={context}
            contextWindow={contextWindow}
            accessCopy={accessCopy}
            isSending={isSending}
            isUnavailable={isUnavailable}
            isUngovernedSession={isUngovernedSession}
            isWorking={isWorking}
            onInterrupt={onInterrupt}
            onPrimary={onPrimary}
            onQueueNext={onQueueNext}
            onSessionStop={onSessionStop}
            pendingAction={pendingAction}
            primaryDisabled={!canSubmitText || isSending}
            recipientControl={recipientControl}
            secondaryLabel={secondaryLabel}
          />
        )}
      </div>
    </div>
  );
}

function CompactComposerActions({
  canControl,
  canInterrupt,
  canSessionStop,
  canSubmitText,
  deliveryHint,
  isDisconnected,
  canWriteChannel,
  isSending,
  layout,
  onInterrupt,
  onPrimary,
  onQueueNext,
  onSessionStop,
  secondaryLabel,
  sendLabel,
  showStopAction,
}: {
  canControl: boolean;
  canInterrupt: boolean;
  canSessionStop: boolean;
  canSubmitText: boolean;
  deliveryHint: string | null;
  isDisconnected: boolean;
  canWriteChannel: boolean;
  isSending: boolean;
  layout: "inline" | "stacked";
  onInterrupt: () => void;
  onPrimary: () => void;
  onQueueNext: () => void;
  onSessionStop: () => void;
  secondaryLabel: string | null;
  sendLabel: string;
  showStopAction: boolean;
}) {
  return (
    <div
      className={cn(
        "flex shrink-0 flex-wrap items-center justify-end gap-2",
        layout === "stacked" && "w-full",
      )}
      data-testid="coding-session-composer-actions"
    >
      {/* Ahead of the buttons in the reading order, because it is what tells
          the person which of them they want. Wraps onto its own line in the
          stacked layout rather than squeezing the controls. */}
      {deliveryHint === null ? null : (
        <p
          className="me-auto text-2xs text-muted-foreground"
          data-testid="coding-session-composer-delivery-hint"
        >
          {deliveryHint}
        </p>
      )}
      <Button
        data-testid="coding-session-composer-primary"
        disabled={!canSubmitText || isSending}
        onClick={onPrimary}
        type="button"
      >
        {sendLabel}
      </Button>
      {secondaryLabel === null ? null : (
        <Button
          data-testid="coding-session-composer-queue-next"
          disabled={!canSubmitText || isSending}
          onClick={onQueueNext}
          title="Send now; this provider runs it when the current turn ends, and it cannot be recalled"
          type="button"
          variant="outline"
        >
          {secondaryLabel}
        </Button>
      )}
      {showStopAction ? (
        <Button
          data-testid="coding-session-composer-stop"
          disabled={
            !canControl || !canWriteChannel || !canInterrupt || isSending
          }
          onClick={onInterrupt}
          title={
            canInterrupt
              ? "Interrupt only the current turn"
              : "Current-turn interrupt is unavailable for this provider."
          }
          type="button"
          variant="outline"
        >
          Interrupt
        </Button>
      ) : null}
      {canSessionStop && !isDisconnected ? (
        <Button
          data-testid="coding-session-composer-session-stop"
          disabled={isSending}
          onClick={onSessionStop}
          title="Stop this provider execution; the session stays open."
          type="button"
          variant="outline"
        >
          Stop execution
        </Button>
      ) : null}
    </div>
  );
}

/**
 * The strip above the immersive composer: notices and the delivery hint, one
 * line each.
 *
 * SESSION_VIEW_UX_PLAN L4. A lifecycle notice (unreachable, disconnected,
 * ended) is the strip's line; otherwise, while a turn runs, the delivery hint
 * is — the two never compete for space. Each line is one line: the sentence
 * truncates visually with its whole text in the `title` and in the DOM, so a
 * screen reader hears all of it, and the state word leads every sentence so
 * the truth survives the cut. An error is never folded into that one line: a
 * failure gets its own row, in full.
 */
function ComposerLifecycleNotice({
  authorityReason,
  canControl,
  canSessionStop,
  deliveryHint,
  error,
  errorAction,
  isDisconnected,
  isEnded,
  accessCopy,
  isResuming,
  isSending,
  onAddProvider,
  onReconnect,
  onSessionStop,
  providerAuthorityPubkey,
  recipientLabel,
  unreachable,
  unreachableDetail,
}: {
  /** The seat this notice is about, or null to keep the unnamed sentence. */
  recipientLabel: string | null;
  authorityReason: string | null;
  canControl: boolean;
  canSessionStop: boolean;
  /** What the mid-turn controls will do, or null when idle. */
  deliveryHint: string | null;
  error: string | null;
  errorAction: React.ReactNode;
  isDisconnected: boolean;
  isEnded: boolean;
  /** Why channel write is refused, or null when it is allowed. */
  accessCopy: CodingSessionChannelAccessCopy | null;
  isResuming: boolean;
  isSending: boolean;
  onAddProvider?: () => void;
  onReconnect: () => void;
  onSessionStop: () => void;
  providerAuthorityPubkey: string | null;
  unreachable: boolean;
  unreachableDetail: string | null;
}) {
  if (
    !error &&
    !errorAction &&
    !unreachable &&
    !isDisconnected &&
    !isEnded &&
    deliveryHint === null
  ) {
    return null;
  }
  const reconnectDisabledReason = accessCopy
    ? accessCopy.reconnect
    : !canControl
      ? (authorityReason ??
        "You do not have permission to reconnect this execution.")
      : !providerAuthorityPubkey
        ? "Reconnect is unavailable until provider authority is available."
        : null;
  const stopDisabledReason = canSessionStop
    ? null
    : accessCopy
      ? accessCopy.stop
      : !providerAuthorityPubkey
        ? "Stop is unavailable until provider authority is available."
        : "Only the session founder can stop this execution.";
  return (
    <div className="relative z-0 mx-3 -mb-5 space-y-1 rounded-t-2xl border border-b-0 border-border/70 bg-muted/35 px-3 pt-1.5 pb-6">
      {error ? (
        <p
          className="text-sm text-destructive"
          data-testid="coding-session-composer-error"
        >
          {error}
        </p>
      ) : null}
      {errorAction}
      {unreachable ? (
        <LifecycleNoticeRow
          action={
            onAddProvider ? (
              <Button
                data-testid="coding-session-composer-add-provider"
                onClick={onAddProvider}
                size="xs"
                type="button"
              >
                Add provider
              </Button>
            ) : null
          }
          testId="coding-session-composer-unreachable"
        >
          {`No provider is answering for ${recipientLabel ?? "this execution"}${unreachableDetail ? ` — ${unreachableDetail}` : ""}. Add a provider to the session to continue the work.`}
        </LifecycleNoticeRow>
      ) : isDisconnected ? (
        <LifecycleNoticeRow
          action={
            <div className="flex items-center gap-2">
              <Button
                data-testid="coding-session-composer-resume"
                disabled={
                  !canControl ||
                  accessCopy !== null ||
                  !providerAuthorityPubkey ||
                  isSending
                }
                onClick={onReconnect}
                size="xs"
                type="button"
              >
                {isResuming ? "Reconnecting…" : "Reconnect"}
              </Button>
              <Button
                data-testid="coding-session-composer-session-stop"
                disabled={!canSessionStop || isSending}
                onClick={onSessionStop}
                size="xs"
                title={
                  stopDisabledReason ??
                  "Stop this provider execution; the session stays open."
                }
                type="button"
                variant="outline"
              >
                Stop execution
              </Button>
            </div>
          }
          // Why Reconnect or Stop is disabled is said on screen, each on its
          // own line — never only in the disabled button's tooltip, which is
          // invisible on touch and unannounced by most screen readers.
          reasons={[reconnectDisabledReason, stopDisabledReason]}
        >
          This provider execution is disconnected.
        </LifecycleNoticeRow>
      ) : isEnded ? (
        <LifecycleNoticeRow
          action={
            onAddProvider ? (
              <Button
                data-testid="coding-session-composer-add-provider"
                onClick={onAddProvider}
                size="xs"
                type="button"
              >
                Add provider
              </Button>
            ) : null
          }
          testId="coding-session-composer-ended"
        >
          This provider execution has ended and cannot be resumed.
        </LifecycleNoticeRow>
      ) : deliveryHint !== null ? (
        // Wraps rather than truncating: the delivery class is the most
        // consequential thing about a message sent into a running turn, and a
        // cut sentence would leave its second half to a tooltip.
        <p
          className="line-clamp-2 text-2xs leading-5 text-muted-foreground"
          data-testid="coding-session-composer-delivery-hint"
          title={deliveryHint}
        >
          {deliveryHint}
        </p>
      ) : null}
    </div>
  );
}

/**
 * One notice: the state sentence (at most two lines, state word first, whole
 * text in the DOM and the `title`), then any reason a control beside it is
 * disabled, each on its own unclamped line.
 */
function LifecycleNoticeRow({
  action,
  children,
  reasons = [],
  testId,
}: {
  action: React.ReactNode;
  children: React.ReactNode;
  /** Why an action here is unavailable; nulls are skipped. */
  reasons?: ReadonlyArray<string | null>;
  testId?: string;
}) {
  const shown = reasons.filter((reason): reason is string => Boolean(reason));
  return (
    <div
      className="flex min-h-6 items-start justify-between gap-3"
      data-testid={testId}
    >
      <div className="min-w-0 py-0.5">
        <p
          className="line-clamp-2 text-xs text-muted-foreground"
          title={typeof children === "string" ? children : undefined}
        >
          {children}
        </p>
        {shown.map((reason) => (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="coding-session-composer-notice-reason"
            key={reason}
          >
            {reason}
          </p>
        ))}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

function CompactComposerNotices({
  accessCopy,
  authorityReason,
  canControl,
  error,
  errorAction,
  isUngovernedSession,
  showAuthorityFailure,
}: {
  accessCopy: CodingSessionChannelAccessCopy | null;
  authorityReason: string | null;
  canControl: boolean;
  error: string | null;
  errorAction: React.ReactNode;
  isUngovernedSession: boolean;
  showAuthorityFailure: boolean;
}) {
  return (
    <>
      {showAuthorityFailure && accessCopy ? (
        <p
          className="mb-2 text-sm text-muted-foreground"
          data-testid="coding-session-composer-membership-failure"
        >
          {accessCopy.notice}
        </p>
      ) : null}
      {isUngovernedSession ? (
        <p
          className="mb-2 text-xs text-muted-foreground"
          data-testid="coding-session-ungoverned-hint"
        >
          ungoverned — adopt to govern.
        </p>
      ) : null}
      {!canControl ? (
        <p
          className="mb-2 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-foreground"
          data-testid="coding-session-composer-authority-gated"
        >
          {authorityReason ??
            "Only the session founder can control this session."}
        </p>
      ) : null}
      {error ? (
        <p
          className="mb-2 text-sm text-destructive"
          data-testid="coding-session-composer-error"
        >
          {error}
        </p>
      ) : null}
      {errorAction ? <div className="mb-2">{errorAction}</div> : null}
    </>
  );
}

function composerPlaceholder({
  authorityReason,
  authorityUnresolved,
  canControl,
  canSteer,
  isDisconnected,
  isEnded,
  accessCopy,
  isWorking,
}: {
  authorityReason: string | null;
  authorityUnresolved: boolean;
  canControl: boolean;
  canSteer: boolean;
  isDisconnected: boolean;
  isEnded: boolean;
  accessCopy: CodingSessionChannelAccessCopy | null;
  isWorking: boolean;
}): string {
  if (isEnded) return "This execution has ended.";
  if (isDisconnected) return "Reconnect this execution to continue…";
  if (accessCopy) return accessCopy.placeholder;
  if (!canControl && authorityUnresolved) {
    return authorityReason ?? "Session access is unresolved.";
  }
  if (!canControl) return "View only — ask for collaborator access.";
  if (isWorking && !canSteer) return "Send the next turn…";
  if (isWorking) return "Steer this coding session…";
  return "Send a message…";
}
