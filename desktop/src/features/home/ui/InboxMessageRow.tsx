import * as React from "react";

import { useKnownAgentPubkeys } from "@/features/agents/useKnownAgentPubkeys";
import type { InboxContextMessage } from "@/features/home/lib/inbox";
import { toTimelineMessage } from "@/features/home/lib/inboxViewHelpers";
import { formatTimeWithoutDayPeriod } from "@/features/messages/lib/dateFormatters";
import { formatItemTimestamp } from "@/shared/lib/datetime";
import type { TimelineMessage } from "@/features/messages/types";
import { getConfigNudgeAuthorPubkey } from "@/features/messages/ui/configNudgeAuthPubkey";
import {
  MessageActionBar,
  type MessageDeleteAuthority,
} from "@/features/messages/ui/MessageActionBar";
import { MessageAgentOwner } from "@/features/messages/ui/MessageAgentOwner";
import { MessageMetaSeparator } from "@/features/messages/ui/MessageHeader";
import { MessageReactions } from "@/features/messages/ui/MessageReactions";
import { UnreadDivider } from "@/features/messages/ui/UnreadDivider";
import { useReactionHandler } from "@/features/messages/ui/useReactionHandler";
import { useMessageEmoji } from "@/features/messages/lib/useMessageEmoji";
import { readHostStepApprovalRequest } from "@/features/project-actions/lib/hostStepApproval";
import { HostStepApprovalInboxCard } from "@/features/project-actions/ui/HostStepApprovalInboxCard";
import { readInboxDecisionRequest } from "../lib/decisionRequestInbox";
import { DecisionRequestInboxCard } from "./DecisionRequestInboxCard";
import { UserProfilePopover } from "@/features/profile/ui/UserProfilePopover";
import { cn } from "@/shared/lib/cn";
import { normalizePubkey } from "@/shared/lib/pubkey";
import { hasLinkPreviewSuppression } from "@/features/messages/lib/formatTimelineMessages";
import { UserAvatar } from "@/shared/ui/UserAvatar";
import type { VideoReviewContext } from "@/shared/ui/VideoPlayer";
import { VideoReviewCommentMarkdown } from "@/shared/ui/VideoReviewCommentMarkdown";
import { parseImetaTags } from "@/shared/ui/markdown/parseImeta";

export type InboxDisplayMessage = InboxContextMessage & {
  depth: number;
};

type InboxMessageRowProps = {
  agentPubkeys?: ReadonlySet<string>;
  canReply: boolean;
  /** Channel UUID for "Copy link" — passed straight through to MessageActionBar. */
  channelId?: string | null;
  /** Authority `onDelete` publishes under — relabels the control when it is a
   *  moderator delete rather than the viewer's own. */
  deleteAuthority?: MessageDeleteAuthority;
  isContinuation?: boolean;
  isFirst?: boolean;
  isFocusHighlightVisible: boolean;
  message: InboxDisplayMessage;
  onDelete?: (message: InboxDisplayMessage) => void;
  onEdit?: (message: InboxDisplayMessage) => void;
  onSelectReplyTarget: (message: InboxDisplayMessage) => void;
  onToggleReaction?: (
    message: TimelineMessage,
    emoji: string,
    remove: boolean,
  ) => Promise<void>;
  showUnreadBoundary?: boolean;
  videoReviewCommentRootId?: string;
  videoReviewContext?: VideoReviewContext;
};

export function InboxMessageRow({
  agentPubkeys,
  canReply,
  channelId = null,
  deleteAuthority = "self",
  isContinuation = false,
  isFocusHighlightVisible,
  message,
  onDelete,
  onEdit,
  onSelectReplyTarget,
  onToggleReaction,
  showUnreadBoundary = false,
  videoReviewCommentRootId,
  videoReviewContext,
}: InboxMessageRowProps) {
  const timelineMessage = React.useMemo(
    () => toTimelineMessage(message),
    [message],
  );
  // A kind:46010 this reader fully recognises renders as an approval card
  // instead of its JSON body (ledger 171(b)); one it does not keeps the
  // ordinary body, because a half-read request must carry no control.
  const approvalEvent = React.useMemo(() => {
    const candidate = {
      kind: message.kind ?? 0,
      tags: message.tags ?? [],
      content: message.content,
    };
    return readHostStepApprovalRequest(candidate) === null ? null : candidate;
  }, [message.content, message.kind, message.tags]);
  // Ledger 249(A): a `decision.request` held on the viewer renders its
  // question and options with an answer form, never its JSON body.
  const decisionRequest = React.useMemo(
    () =>
      readInboxDecisionRequest({
        id: message.id,
        kind: message.kind ?? 0,
        pubkey: message.authorPubkey,
        tags: message.tags ?? [],
        content: message.content,
      }),
    [
      message.authorPubkey,
      message.content,
      message.id,
      message.kind,
      message.tags,
    ],
  );
  const imetaByUrl = React.useMemo(
    () => (message.tags ? parseImetaTags(message.tags) : undefined),
    [message.tags],
  );
  const { customEmoji, emojiOnly } = useMessageEmoji(
    message.content,
    message.tags,
  );
  const [badgeBurstEmoji, setBadgeBurstEmoji] = React.useState<string | null>(
    null,
  );
  const {
    reactions,
    canToggle: canToggleReactions,
    pending: reactionPending,
    errorMessage: reactionErrorMessage,
    select: handleReactionSelect,
  } = useReactionHandler(timelineMessage, onToggleReaction);
  // "Is this pubkey an agent" = the community-scoped baseline every surface
  // shares plus this surface's extras passed via `agentPubkeys` (HomeView
  // folds feed-profile `isAgent` flags in). Mirrors MessageRow's predicate.
  const knownAgentPubkeys = useKnownAgentPubkeys();
  const isKnownAgentPubkey = React.useCallback(
    (pubkey: string) => {
      const normalized = normalizePubkey(pubkey);
      return (
        knownAgentPubkeys.has(normalized) ||
        agentPubkeys?.has(normalized) === true
      );
    },
    [agentPubkeys, knownAgentPubkeys],
  );
  const isAuthorAgent = isKnownAgentPubkey(message.authorPubkey);
  const profileRole = isAuthorAgent ? "bot" : undefined;
  const hoverTimestampLabel = formatTimeWithoutDayPeriod(
    message.timeLabel ?? message.fullTimestampLabel,
  );
  // Derived here rather than plumbed in with the message: the thread pane has no
  // day divider to supply the date, and deriving on render means a row does not
  // keep saying "Today" after midnight. `fullTimestampLabel` stays the absolute
  // value behind the hover title.
  const timestampLabel = formatItemTimestamp(message.createdAt, {
    withTime: true,
  });
  const timestampNode = (
    <p
      className="shrink-0 text-xs font-normal tabular-nums text-muted-foreground/55"
      title={message.fullTimestampLabel}
    >
      {timestampLabel}
    </p>
  );

  return (
    <div className="relative px-2">
      {showUnreadBoundary ? <UnreadDivider /> : null}
      {message.isSelected ? (
        <div
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute inset-x-3 inset-y-1 rounded-2xl transition-opacity duration-1000",
            isFocusHighlightVisible
              ? "bg-primary/[0.07] opacity-100"
              : "bg-primary/[0.07] opacity-0",
          )}
        />
      ) : null}
      <article
        className={cn(
          "group/message relative z-10 hover:z-20 focus-within:z-20 mx-1 flex gap-2.5 rounded-2xl px-2 py-1 transition-colors hover:bg-muted/50 focus-within:bg-muted/50",
          isContinuation ? "items-center" : "items-start",
        )}
        data-message-id={message.id}
        data-testid={
          message.isSelected
            ? "home-inbox-selected-message"
            : "home-inbox-context-message"
        }
      >
        {canReply || canToggleReactions || onDelete || onEdit ? (
          <div className="pointer-events-none absolute right-2 top-1 z-10">
            <MessageActionBar
              channelId={channelId}
              deleteAuthority={deleteAuthority}
              message={timelineMessage}
              onDelete={onDelete ? () => onDelete(message) : undefined}
              onEdit={onEdit ? () => onEdit(message) : undefined}
              onReactionSelect={
                canToggleReactions ? handleReactionSelect : undefined
              }
              onReactionBadgeBurstRequest={
                reactionPending ? undefined : setBadgeBurstEmoji
              }
              onReply={
                canReply ? () => onSelectReplyTarget(message) : undefined
              }
              reactionErrorMessage={reactionErrorMessage}
              reactions={reactions}
            />
          </div>
        ) : null}

        {isContinuation ? (
          <div
            aria-hidden="true"
            className="flex w-9 shrink-0 self-stretch items-start justify-end pt-0.5"
            title={message.fullTimestampLabel}
          >
            <p className="shrink-0 cursor-default whitespace-nowrap text-xs font-normal leading-4 tabular-nums text-muted-foreground/55 opacity-0 transition-opacity group-hover/message:opacity-100 group-focus-within/message:opacity-100">
              {hoverTimestampLabel}
            </p>
          </div>
        ) : (
          <div className="relative shrink-0">
            <UserProfilePopover
              botIdenticonValue={message.authorLabel}
              pubkey={message.authorPubkey}
              role={profileRole}
              triggerElement="span"
            >
              <span className="inline-flex shrink-0 rounded-full focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring">
                <UserAvatar
                  avatarUrl={message.avatarUrl}
                  className="h-9 w-9 shrink-0"
                  displayName={message.authorLabel}
                  size="md"
                />
              </span>
            </UserProfilePopover>
          </div>
        )}

        <div className="min-w-0 flex-1">
          {isContinuation ? null : (
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0">
              <UserProfilePopover
                botIdenticonValue={message.authorLabel}
                pubkey={message.authorPubkey}
                role={profileRole}
                triggerElement="span"
              >
                <span className="block max-w-full truncate rounded text-sm font-semibold text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring">
                  {message.authorLabel}
                </span>
              </UserProfilePopover>
              {message.isAgent ? (
                <>
                  <MessageAgentOwner
                    ownerLabel={message.ownerLabel}
                    ownerPubkey={message.ownerPubkey}
                  />
                  {/*
                    Grouped with the timestamp so the divider never wraps to the
                    start of a line on its own. Gap matches the container's, so
                    spacing reads the same either side of the divider.
                  */}
                  <span className="inline-flex min-w-0 items-center gap-x-2">
                    <MessageMetaSeparator />
                    {timestampNode}
                  </span>
                </>
              ) : (
                timestampNode
              )}
            </div>
          )}

          <div className={isContinuation ? "mt-0" : "mt-0.5"}>
            {/*
              Ledger 171(b): a kind:46010 host-step approval request is a
              command about to run on this computer, not a message. Rendering
              its JSON body under a reply composer left hand-signing a
              kind:46030 as the only way to answer. The card states the
              action, step, definition, commit and exact command, and offers
              the answer only to whoever may give it; an event it cannot
              fully read renders `null` and the ordinary body still shows.
            */}
            {approvalEvent ? (
              <HostStepApprovalInboxCard event={approvalEvent} />
            ) : null}
            {decisionRequest ? (
              <DecisionRequestInboxCard request={decisionRequest} />
            ) : null}
            {approvalEvent || decisionRequest ? null : (
              <VideoReviewCommentMarkdown
                className={cn(
                  "max-w-full text-left text-sm text-foreground",
                  emojiOnly &&
                    "text-4xl leading-tight [&_p]:leading-tight [&_img[data-custom-emoji]]:h-[1.45em] [&_img[data-custom-emoji]]:align-middle [&_button:has(img[data-custom-emoji])]:align-middle",
                )}
                // Only pass the author pubkey for agent-authored messages so
                // config-nudge cards can authenticate the sender. Uses the
                // raw event signer (signerPubkey), not a relay-delegated display
                // author, because the agent itself must have signed the card.
                configNudgeAuthorPubkey={getConfigNudgeAuthorPubkey(
                  timelineMessage,
                  isKnownAgentPubkey,
                )}
                content={message.content}
                messageId={message.id}
                linkPreviewsSuppressed={hasLinkPreviewSuppression(
                  timelineMessage.tags,
                )}
                customEmoji={customEmoji}
                imetaByUrl={imetaByUrl}
                mentionNames={message.mentionNames}
                mentionPubkeysByName={message.mentionPubkeysByName}
                videoReviewCommentRootId={videoReviewCommentRootId}
                videoReviewContext={videoReviewContext}
              />
            )}
            <MessageReactions
              canToggle={canToggleReactions}
              messageId={message.id}
              onSelect={(emoji) => {
                void handleReactionSelect(emoji);
              }}
              burstEmojiOnRender={badgeBurstEmoji}
              onBurstEmojiRendered={(emoji) => {
                setBadgeBurstEmoji((current) =>
                  current === emoji ? null : current,
                );
              }}
              pending={reactionPending}
              reactions={reactions}
            />
            {reactionErrorMessage ? (
              <p className="mt-1.5 text-xs text-destructive">
                {reactionErrorMessage}
              </p>
            ) : null}
          </div>
        </div>
      </article>
    </div>
  );
}
