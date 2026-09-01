import { hasPrimaryShortcutModifier } from "@/shared/lib/platform";

/** UI model for the provider-neutral coding-session composer. */
export function getCodingSessionComposerState({
  canSteer,
  hasUnsettledAttachments = false,
  isMember,
  isWorking,
  text,
}: {
  /**
   * Whether this execution's runtime advertised native steering. The label is
   * the promise the button makes, so an execution that cannot steer must not
   * offer to: its mid-turn send reaches the provider now and runs at the next
   * turn boundary, which is "Send next", not "Steer".
   *
   * Required, and it used to default to `true`. A call site that forgot it got
   * a Steer button and a `deliver: "steer"` command aimed at a provider whose
   * `threadSteer` is false — a control that is downgraded every single time it
   * is pressed. The safe default for a capability is that it is absent.
   */
  canSteer: boolean;
  /**
   * True while an attached image is still uploading, or when one failed.
   *
   * Send is held closed in both cases. Publishing mid-upload would sign a turn
   * whose `attachments` list is short of what the composer is showing, and
   * publishing after a failure would sign a hash the relay has no blob for —
   * either way the operator would believe they sent a picture that never
   * arrived.
   */
  hasUnsettledAttachments?: boolean;
  isMember: boolean;
  isWorking: boolean;
  text: string;
}) {
  return {
    canSend: isMember && text.trim().length > 0 && !hasUnsettledAttachments,
    // "Interrupt", never "Stop": the composer also carries **Stop execution**,
    // which is terminal and cannot be undone. Two adjacent buttons both
    // reading Stop is how an operator ends an execution while meaning to end a
    // turn (asked about live, 2026-08-24).
    primaryLabel: isWorking ? "Interrupt" : "Send",
    sendLabel: isWorking ? (canSteer ? "Steer" : "Send next") : "Send",
    showStopAction: isWorking,
    showAuthorityFailure: !isMember,
  };
}

/** Enter steers/sends; Shift+Enter deliberately stays a newline. */
export function shouldSubmitCodingSessionComposerKey(event: {
  key: string;
  shiftKey: boolean;
}): boolean {
  return event.key === "Enter" && !event.shiftKey;
}

/**
 * Prompt-history recall: the primary modifier plus ↑/↓.
 *
 * On macOS ⌘↑/⌘↓ otherwise jump the caret to the start or end of the
 * textarea, which is why the composer claims the event; a bare ↑/↓ is left
 * alone so ordinary caret movement inside a multi-line draft still works.
 */
export function matchCodingSessionHistoryKey(event: {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): "older" | "newer" | null {
  if (event.shiftKey || event.altKey) return null;
  if (!hasPrimaryShortcutModifier(event)) return null;
  if (event.key === "ArrowUp") return "older";
  if (event.key === "ArrowDown") return "newer";
  return null;
}
