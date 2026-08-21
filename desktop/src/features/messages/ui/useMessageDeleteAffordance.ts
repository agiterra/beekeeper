import * as React from "react";

import { useMessageModeration } from "@/features/messages/hooks";
import { messageManageAuthority } from "@/features/messages/lib/canManageMessage";
import type { TimelineMessage } from "@/features/messages/types";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import type { ChannelType } from "@/shared/api/types";
import type { MessageDeleteAuthority } from "./MessageActionBar";

/** What a surface should render for one message's edit/delete controls. */
export type MessageDeleteAffordance = {
  /** Drives the control's label and confirmation copy. */
  deleteAuthority: MessageDeleteAuthority;
  /** Handler to render, or undefined when the viewer may not delete at all. */
  onDelete?: (message: TimelineMessage) => void;
  /** Moderators may delete but never edit someone else's message. */
  canEdit: boolean;
};

const NO_DELETE: MessageDeleteAffordance = {
  deleteAuthority: "self",
  onDelete: undefined,
  canEdit: false,
};

/**
 * Resolves, per message, which delete a surface should offer.
 *
 * Self-authored (and owned-agent) messages keep the surface's own kind:5
 * handler. Another user's message is deletable only under the kind:9005
 * moderator path, which is a *different event* published by this hook rather
 * than a flag on the caller's handler — that separation is what keeps ordinary
 * self-deletes off the tombstone-emitting kind.
 *
 * Call once per surface: it owns a query subscription and a mutation observer,
 * so it must not be called per row.
 */
export function useMessageDeleteAffordance(input: {
  channelId: string | null | undefined;
  channelType: ChannelType | null | undefined;
  currentPubkey?: string;
  profiles?: UserProfileLookup;
  /** The surface's kind:5 delete. Absent means deletes are off here (e.g. an
   *  archived channel) — moderator standing must not reopen them. */
  onDelete?: (message: TimelineMessage) => void;
}): (message: TimelineMessage | null) => MessageDeleteAffordance {
  const { channelId, channelType, currentPubkey, profiles, onDelete } = input;
  const { isModerator, deleteAsModerator } = useMessageModeration(
    channelId,
    channelType,
  );

  return React.useCallback(
    (message: TimelineMessage | null) => {
      if (!message) return NO_DELETE;
      const authority = messageManageAuthority(
        message,
        currentPubkey,
        profiles,
        { isModerator: isModerator && Boolean(onDelete) },
      );
      if (authority === "self") {
        return { deleteAuthority: "self", onDelete, canEdit: true } as const;
      }
      if (authority === "moderator") {
        return {
          deleteAuthority: "moderator",
          onDelete: deleteAsModerator,
          canEdit: false,
        } as const;
      }
      return NO_DELETE;
    },
    [currentPubkey, deleteAsModerator, isModerator, onDelete, profiles],
  );
}
