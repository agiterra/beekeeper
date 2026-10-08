// Pure parsing for the preview broadcast pump: which kind 24320 surface
// watches are addressed to this desktop's preview (WIRE-C5 § 3). The relay
// already validates them strictly; this is the client-side backstop, and it
// is exact: tags in wire order (h, surface, d, p), nothing else.

export type PreviewWatchAction = "watch" | "stop" | "resync" | "snapshot";

export type PreviewWatchRequest = {
  channelId: string;
  sessionRef: string;
  watcherPubkey: string;
  action: PreviewWatchAction;
};

type WatchEventLike = {
  pubkey: string;
  content: string;
  tags: string[][];
};

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const HEX64_RE = /^[0-9a-f]{64}$/;
const ACTIONS: readonly PreviewWatchAction[] = [
  "watch",
  "stop",
  "resync",
  "snapshot",
];

/**
 * The watch request a 24320 carries for a preview addressed to `myPubkey`,
 * or null (device surface, wrong producer, malformed tags or content).
 */
export function parsePreviewWatchEvent(
  event: WatchEventLike,
  myPubkey: string,
): PreviewWatchRequest | null {
  const { tags } = event;
  if (!Array.isArray(tags) || tags.length !== 4) return null;
  const [h, surface, d, p] = tags;
  const pair = (tag: string[] | undefined, name: string) =>
    Array.isArray(tag) && tag.length === 2 && tag[0] === name
      ? tag[1]
      : undefined;
  const channelId = pair(h, "h");
  const surfaceValue = pair(surface, "surface");
  const sessionRef = pair(d, "d");
  const producer = pair(p, "p");
  if (surfaceValue !== "preview") return null;
  if (!channelId || !UUID_RE.test(channelId)) return null;
  if (!sessionRef || !UUID_RE.test(sessionRef)) return null;
  if (!producer || producer !== myPubkey) return null;
  if (!HEX64_RE.test(event.pubkey) || event.pubkey === myPubkey) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }
  const keys = Object.keys(parsed);
  const action = (parsed as { action?: unknown }).action;
  if (keys.length !== 1 || keys[0] !== "action") return null;
  if (!ACTIONS.includes(action as PreviewWatchAction)) return null;
  return {
    channelId,
    sessionRef,
    watcherPubkey: event.pubkey,
    action: action as PreviewWatchAction,
  };
}
