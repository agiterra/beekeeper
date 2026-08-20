import { invokeTauri } from "@/shared/api/tauri";

/**
 * Delete a message.
 *
 * `moderator` selects the authority the delete is published under, and with it
 * the wire kind. `false` (the default) is the ordinary self-delete: NIP-09
 * kind:5, which the relay authorizes for the author and the author agent's
 * NIP-OA owner only, and which leaves no trace in the channel. `true` publishes
 * the Buzz-native kind:9005 that channel/community owners and admins are also
 * authorized for — the relay answers it with a `message_deleted` system
 * tombstone in the channel, so it must never be used for ordinary self-deletes
 * or for DMs (9005 is channel-scoped).
 *
 * Lives outside `tauri.ts` for the same reason `editMessage` does: that module
 * is over the file-size ceiling and may not grow.
 */
export async function deleteMessage(
  channelId: string,
  eventId: string,
  moderator = false,
): Promise<void> {
  await invokeTauri("delete_message", { channelId, eventId, moderator });
}
