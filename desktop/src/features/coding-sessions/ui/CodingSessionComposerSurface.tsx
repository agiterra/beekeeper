import type * as React from "react";

import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import {
  matchCodingSessionHistoryKey,
  shouldSubmitCodingSessionComposerKey,
} from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import type { CodingSessionAttachmentController } from "@/features/coding-sessions/lib/useCodingSessionImageAttachments";
import { CodingSessionComposerAttachments } from "./CodingSessionComposerAttachments";
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
  isMember: boolean;
  isResuming: boolean;
  isSending: boolean;
  isUnavailable: boolean;
  isUngovernedSession: boolean;
  isWorking: boolean;
  layout: "inline" | "stacked";
  onAddProvider?: () => void;
  onInterrupt: () => void;
  onPrimary: () => void;
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
  isMember,
  isResuming,
  isSending,
  isUnavailable,
  isUngovernedSession,
  isWorking,
  layout,
  onAddProvider,
  onInterrupt,
  runtimeLabel,
  onPrimary,
  onRecallHistory,
  onReconnect,
  onSessionStop,
  onTextChange,
  pendingAction,
  providerAuthorityPubkey,
  recipientControl,
  sendLabel,
  showAuthorityFailure,
  showStopAction,
  text,
  unreachable,
  unreachableDetail,
  variant,
}: ComposerSurfaceProps) {
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
          canControl={canControl}
          canSessionStop={canSessionStop}
          error={error}
          errorAction={errorAction}
          isDisconnected={isDisconnected}
          isEnded={isEnded}
          isMember={isMember}
          isResuming={isResuming}
          isSending={isSending}
          onAddProvider={onAddProvider}
          onReconnect={onReconnect}
          onSessionStop={onSessionStop}
          providerAuthorityPubkey={providerAuthorityPubkey}
          unreachable={unreachable}
          unreachableDetail={unreachableDetail}
        />
      ) : (
        <CompactComposerNotices
          authorityReason={authorityReason}
          canControl={canControl}
          error={error}
          errorAction={errorAction}
          isUngovernedSession={isUngovernedSession}
          showAuthorityFailure={showAuthorityFailure}
        />
      )}
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
            immersive &&
              "block min-h-24 w-full resize-none rounded-none border-0 bg-transparent px-4 pt-4 pb-1 shadow-none focus-visible:ring-0",
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
            canControl,
            canSteer,
            isDisconnected,
            isEnded,
            isMember,
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
            isDisconnected={isDisconnected}
            isMember={isMember}
            isSending={isSending}
            layout={layout}
            onInterrupt={onInterrupt}
            onPrimary={onPrimary}
            onSessionStop={onSessionStop}
            sendLabel={sendLabel}
            showStopAction={showStopAction}
          />
        ) : (
          <CodingSessionComposerDeck
            authorityReason={authorityReason}
            canControl={canControl}
            canInterrupt={canInterrupt}
            canSessionStop={canSessionStop}
            canSteer={canSteer}
            context={context}
            contextWindow={contextWindow}
            isMember={isMember}
            isSending={isSending}
            isUnavailable={isUnavailable}
            isUngovernedSession={isUngovernedSession}
            isWorking={isWorking}
            onInterrupt={onInterrupt}
            onPrimary={onPrimary}
            onSessionStop={onSessionStop}
            pendingAction={pendingAction}
            primaryDisabled={!canSubmitText || isSending}
            recipientControl={recipientControl}
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
  isDisconnected,
  isMember,
  isSending,
  layout,
  onInterrupt,
  onPrimary,
  onSessionStop,
  sendLabel,
  showStopAction,
}: {
  canControl: boolean;
  canInterrupt: boolean;
  canSessionStop: boolean;
  canSubmitText: boolean;
  isDisconnected: boolean;
  isMember: boolean;
  isSending: boolean;
  layout: "inline" | "stacked";
  onInterrupt: () => void;
  onPrimary: () => void;
  onSessionStop: () => void;
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
      <Button
        data-testid="coding-session-composer-primary"
        disabled={!canSubmitText || isSending}
        onClick={onPrimary}
        type="button"
      >
        {sendLabel}
      </Button>
      {showStopAction ? (
        <Button
          data-testid="coding-session-composer-stop"
          disabled={!canControl || !isMember || !canInterrupt || isSending}
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

function ComposerLifecycleNotice({
  canControl,
  canSessionStop,
  error,
  errorAction,
  isDisconnected,
  isEnded,
  isMember,
  isResuming,
  isSending,
  onAddProvider,
  onReconnect,
  onSessionStop,
  providerAuthorityPubkey,
  unreachable,
  unreachableDetail,
}: {
  canControl: boolean;
  canSessionStop: boolean;
  error: string | null;
  errorAction: React.ReactNode;
  isDisconnected: boolean;
  isEnded: boolean;
  isMember: boolean;
  isResuming: boolean;
  isSending: boolean;
  onAddProvider?: () => void;
  onReconnect: () => void;
  onSessionStop: () => void;
  providerAuthorityPubkey: string | null;
  unreachable: boolean;
  unreachableDetail: string | null;
}) {
  if (!error && !errorAction && !unreachable && !isDisconnected && !isEnded) {
    return null;
  }
  return (
    <div className="relative z-0 mx-3 -mb-5 space-y-2 rounded-t-2xl border border-b-0 border-border/70 bg-muted/35 px-3 pt-3 pb-7">
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
                size="sm"
                type="button"
              >
                Add provider
              </Button>
            ) : null
          }
          testId="coding-session-composer-unreachable"
        >
          {`No provider is answering for this execution${unreachableDetail ? ` — ${unreachableDetail}` : ""}. Add a provider to the session to continue the work.`}
        </LifecycleNoticeRow>
      ) : isDisconnected ? (
        <LifecycleNoticeRow
          action={
            <div className="flex items-center gap-2">
              <Button
                data-testid="coding-session-composer-resume"
                disabled={
                  !canControl ||
                  !isMember ||
                  !providerAuthorityPubkey ||
                  isSending
                }
                onClick={onReconnect}
                size="sm"
                type="button"
              >
                {isResuming ? "Reconnecting…" : "Reconnect"}
              </Button>
              <Button
                data-testid="coding-session-composer-session-stop"
                disabled={!canSessionStop || isSending}
                onClick={onSessionStop}
                size="sm"
                type="button"
                variant="outline"
              >
                Stop execution
              </Button>
            </div>
          }
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
                size="sm"
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
      ) : null}
    </div>
  );
}

function LifecycleNoticeRow({
  action,
  children,
  testId,
}: {
  action: React.ReactNode;
  children: React.ReactNode;
  testId?: string;
}) {
  return (
    <div
      className="flex items-center justify-between gap-3"
      data-testid={testId}
    >
      <p className="text-sm text-muted-foreground">{children}</p>
      {action}
    </div>
  );
}

function CompactComposerNotices({
  authorityReason,
  canControl,
  error,
  errorAction,
  isUngovernedSession,
  showAuthorityFailure,
}: {
  authorityReason: string | null;
  canControl: boolean;
  error: string | null;
  errorAction: React.ReactNode;
  isUngovernedSession: boolean;
  showAuthorityFailure: boolean;
}) {
  return (
    <>
      {showAuthorityFailure ? (
        <p
          className="mb-2 text-sm text-muted-foreground"
          data-testid="coding-session-composer-membership-failure"
        >
          Join this channel for native control. Compatibility control also
          requires an allowlisted operator.
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
  canControl,
  canSteer,
  isDisconnected,
  isEnded,
  isMember,
  isWorking,
}: {
  canControl: boolean;
  canSteer: boolean;
  isDisconnected: boolean;
  isEnded: boolean;
  isMember: boolean;
  isWorking: boolean;
}): string {
  if (isEnded) return "This execution has ended.";
  if (isDisconnected) return "Reconnect this execution to continue…";
  if (!isMember) return "Join this channel to send a message.";
  if (!canControl) return "View only — ask for collaborator access.";
  if (isWorking && !canSteer) return "Send the next turn…";
  if (isWorking) return "Steer this coding session…";
  return "Send a message…";
}
