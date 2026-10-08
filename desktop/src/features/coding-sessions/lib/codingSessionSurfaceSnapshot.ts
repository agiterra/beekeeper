/**
 * Shared surface records, read strictly (WIRE-C5 §§ 2, 5, 6): the 44253
 * surface snapshot both the Device and the Browser surfaces show as a card,
 * the 30626 preview announce and its owner fold (a port of
 * `session_preview::resolve_preview_owner`), and the remote preview's view
 * model. Pure: no React, no relay, no clock — `now` is always passed in.
 *
 * Every parser follows the wire's house rule: tags in the listed order,
 * optional tags skipped and never reordered, no unknown or duplicate tag,
 * canonical decimals. A record that breaks any of it is dropped, never
 * half-read. Nothing here looks for a UDID, a path or a hostname: none is on
 * the wire, and a value shaped like one is grounds to drop the record.
 */
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_SNAPSHOT,
} from "@/shared/constants/kinds";
import { truncatePubkey } from "@/shared/lib/pubkey";

// ---------------------------------------------------------------------------
// Strict tag helpers (shared with codingSessionDevice.ts)
// ---------------------------------------------------------------------------

/** One position in a tag layout. */
export type SurfaceTagSlot = {
  name: string;
  /** Total fields, the name included. */
  arity: number;
  required: boolean;
};

/** A required two-field slot. */
export function surfaceRequired(name: string, arity = 2): SurfaceTagSlot {
  return { name, arity, required: true };
}

/** An optional slot; skipped when absent, never reordered. */
export function surfaceOptional(name: string, arity = 2): SurfaceTagSlot {
  return { name, arity, required: false };
}

/**
 * Match `tags` against `layout` in order (`match_ordered_tags`): each slot is
 * the next tag or, if optional, absent. `null` on a missing required tag, a
 * wrong arity, or any tag left over (unknown, duplicate, out of order).
 */
export function matchSurfaceTags(
  tags: readonly (readonly string[])[],
  layout: readonly SurfaceTagSlot[],
): (readonly string[] | null)[] | null {
  const matched: (readonly string[] | null)[] = [];
  let index = 0;
  for (const slot of layout) {
    const tag = tags[index];
    if (tag && tag[0] === slot.name) {
      if (tag.length !== slot.arity) return null;
      if (tag.some((field) => typeof field !== "string")) return null;
      matched.push(tag);
      index += 1;
    } else if (slot.required) {
      return null;
    } else {
      matched.push(null);
    }
  }
  return index === tags.length ? matched : null;
}

const MAX_SAFE = Number.MAX_SAFE_INTEGER;

/** A canonical decimal (no sign, no leading zero but `0`, ≤ 2^53−1), or null. */
export function parseSurfaceDecimal(value: string): number | null {
  if (!/^(0|[1-9][0-9]*)$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed <= MAX_SAFE ? parsed : null;
}

/** `WxH`, each side 1..=8192, lowercase `x`. */
export function parseSurfaceDim(
  value: string,
): { width: number; height: number } | null {
  const parts = value.split("x");
  if (parts.length !== 2) return null;
  const width = parseSurfaceDecimal(parts[0]);
  const height = parseSurfaceDecimal(parts[1]);
  if (width === null || height === null) return null;
  if (width < 1 || height < 1 || width > 8192 || height > 8192) return null;
  return { width, height };
}

/** Lowercase hex of exactly `length` characters. */
export function isSurfaceHex(value: string, length: number): boolean {
  return value.length === length && /^[0-9a-f]+$/.test(value);
}

/** A lowercase canonical 8-4-4-4-12 UUID. */
export function isCanonicalUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    value,
  );
}

const UUID_RUN =
  /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/;
const HOST_LOCAL_PATHS = [
  "/Users/",
  "/home/",
  "/var/folders/",
  "/private/var/",
  "DerivedData",
  "CoreSimulator",
  "file://",
];

/** The path half of WIRE-C5 § 2: a home/temp/simulator path fragment. */
export function hasHostLocalPath(value: string): boolean {
  return HOST_LOCAL_PATHS.some((fragment) => value.includes(fragment));
}

/** WIRE-C5 § 2 in full: a UUID-shaped run or a host-local path fragment. */
export function isHostLocalValue(value: string): boolean {
  return UUID_RUN.test(value) || hasHostLocalPath(value);
}

/**
 * Every tag value passes § 2; tags named in `uuidExempt` skip only the UUID
 * half (the `h` channel, a sessionRef `d`, Beekeeper/runtime identifiers).
 */
export function surfaceTagsPassHostLocal(
  tags: readonly (readonly string[])[],
  uuidExempt: readonly string[],
): boolean {
  for (const tag of tags) {
    const exempt = uuidExempt.includes(tag[0]);
    for (const value of tag.slice(1)) {
      if (exempt ? hasHostLocalPath(value) : isHostLocalValue(value)) {
        return false;
      }
    }
  }
  return true;
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).length;
}

/** A `coding-session/v1|…` key, by its prefix only (the relay parses it). */
export function isCodingSessionTargetKey(value: string): boolean {
  return value.startsWith("coding-session/v1|") && value.length <= 512;
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/** "4 s", "2 min", "3 h": an age with no "ago". Negative ages read 0 s. */
export function formatSurfaceAge(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1_000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h`;
}

/** "every 3 s" from a cadence in ms ("every 0.5 s" under a second). */
export function formatSurfaceCadence(ms: number): string {
  const seconds = ms / 1_000;
  const text = Number.isInteger(seconds)
    ? String(seconds)
    : String(Math.round(seconds * 10) / 10);
  return `every ${text} s`;
}

/** Local `HH:MM`. */
export function formatSurfaceClock(ms: number): string {
  const date = new Date(ms);
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** Names a pubkey: the client's name for it, else its canonical short form. */
export type SurfaceNameOf = (pubkey: string) => string | null | undefined;

/** A person's (or seat's) name, else the short pubkey. Never a hostname. */
export function surfacePersonName(pubkey: string, nameOf?: SurfaceNameOf) {
  return nameOf?.(pubkey)?.trim() || truncatePubkey(pubkey);
}

/**
 * The machine a provider key stands for, as the Agents surface says it:
 * "this computer" for this machine's own provider, else the name this client
 * gives the key, else its short form. Never a hostname.
 */
export function surfaceMachineName(input: {
  providerPubkey: string | null;
  localProviderPubkey?: string | null;
  nameOf?: SurfaceNameOf;
}): string {
  const provider = input.providerPubkey?.trim().toLowerCase() ?? "";
  if (provider === "") return "the session's provider";
  const local = input.localProviderPubkey?.trim().toLowerCase() ?? "";
  if (local !== "" && local === provider) return "this computer";
  return surfacePersonName(provider, input.nameOf);
}

// ---------------------------------------------------------------------------
// kind 44253 — surface snapshot
// ---------------------------------------------------------------------------

export type SurfaceKind = "preview" | "device";

/** One snapshot as a card shows it (`CodingSessionSurfaceSnapshotCard`). */
export type SurfaceSnapshotCard = {
  id: string;
  signer: string;
  createdAt: number;
  channelId: string;
  type: "snapshot" | "annotation";
  surface: SurfaceKind;
  /** preview: sessionRef; device: slot. */
  key: string;
  sha256: string;
  url: string;
  mime: "image/png" | "image/jpeg" | "image/webp";
  dim: { width: number; height: number };
  /** Unix ms. */
  takenAt: number;
  /** The machine's provider key (`provider` tag). */
  machine: string;
  requestedBy: string | null;
  /** `null` → "commit not recorded". */
  commit: { sha: string; dirty: boolean } | null;
  reference: { eventId: string; marker: "command" | "annotation" } | null;
  /** preview only, e.g. `local:/settings`. */
  page: string | null;
  title: string | null;
  alt: string;
};

const SNAPSHOT_LAYOUT: readonly SurfaceTagSlot[] = [
  surfaceRequired("h"),
  surfaceRequired("ssn-v"),
  surfaceRequired("ssn-type"),
  surfaceRequired("surface"),
  surfaceRequired("d"),
  surfaceRequired("x"),
  surfaceRequired("url"),
  surfaceRequired("m"),
  surfaceRequired("dim"),
  surfaceRequired("taken-at"),
  surfaceRequired("provider"),
  surfaceOptional("p", 4),
  surfaceOptional("commit", 3),
  surfaceOptional("e", 4),
  surfaceOptional("page"),
  surfaceOptional("title"),
];

const SNAPSHOT_MIMES = new Set(["image/png", "image/jpeg", "image/webp"]);

/** Validate a `page` value (WIRE-C5 § 5 grammar). */
export function isValidPreviewPage(page: string): boolean {
  if (page === "" || utf8Bytes(page) > 1024) return false;
  if (/[\s\p{Cc}]/u.test(page)) return false;
  if (page.includes("?") || page.includes("#")) return false;
  if (isHostLocalValue(page)) return false;
  if (page.startsWith("local:")) {
    const path = page.slice("local:".length);
    return path.startsWith("/") && !path.startsWith("//");
  }
  const rest = page.startsWith("https://")
    ? page.slice(8)
    : page.startsWith("http://")
      ? page.slice(7)
      : null;
  if (rest === null) return false;
  const host = rest.split("/")[0].toLowerCase();
  if (host === "" || host.includes("@") || host.includes(":")) return false;
  return !isPrivateHost(host);
}

function isPrivateHost(host: string): boolean {
  const bare = host.replace(/^\[/, "").replace(/\]$/, "");
  if (
    bare === "localhost" ||
    bare.endsWith(".localhost") ||
    bare.endsWith(".local") ||
    bare === "::1" ||
    bare === "0.0.0.0"
  ) {
    return true;
  }
  const octets = bare.split(".");
  if (octets.length === 4 && octets.every((o) => /^\d{1,3}$/.test(o))) {
    const [a, b] = octets.map(Number);
    return (
      a === 127 ||
      a === 10 ||
      a === 0 ||
      (a === 192 && b === 168) ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 169 && b === 254)
    );
  }
  return false;
}

function isValidTitle(title: string): boolean {
  return (
    utf8Bytes(title) <= 200 &&
    !/\p{Cc}/u.test(title) &&
    !isHostLocalValue(title)
  );
}

/** Parse one 44253 strictly, or `null`. */
export function parseSurfaceSnapshot(
  event: RelayEvent,
): SurfaceSnapshotCard | null {
  if (event.kind !== KIND_SURFACE_SNAPSHOT) return null;
  const slots = matchSurfaceTags(event.tags, SNAPSHOT_LAYOUT);
  if (!slots) return null;
  const value = (index: number) => slots[index]?.[1] ?? "";
  const channelId = value(0);
  if (!isCanonicalUuid(channelId)) return null;
  if (value(1) !== "1") return null;
  const type = value(2);
  if (type !== "snapshot" && type !== "annotation") return null;
  const surface = value(3);
  if (surface !== "preview" && surface !== "device") return null;
  const key = value(4);
  if (surface === "preview" ? !isCanonicalUuid(key) : !isSurfaceHex(key, 16)) {
    return null;
  }
  const exempt = surface === "preview" ? ["h", "d"] : ["h"];
  if (!surfaceTagsPassHostLocal(event.tags, exempt)) return null;
  const sha256 = value(5);
  if (!isSurfaceHex(sha256, 64)) return null;
  const url = value(6);
  if (!/^https?:\/\/[^\s?#]+$/.test(url) || !url.includes(sha256)) return null;
  const mime = value(7);
  if (!SNAPSHOT_MIMES.has(mime)) return null;
  const dim = parseSurfaceDim(value(8));
  const takenAt = parseSurfaceDecimal(value(9));
  const machine = value(10);
  if (!dim || takenAt === null || !isSurfaceHex(machine, 64)) return null;

  const p = slots[11];
  if (
    p &&
    (!isSurfaceHex(p[1], 64) || p[2] !== "" || p[3] !== "requested-by")
  ) {
    return null;
  }
  const commitTag = slots[12];
  if (
    commitTag &&
    (!isSurfaceHex(commitTag[1], 40) ||
      (commitTag[2] !== "clean" && commitTag[2] !== "dirty"))
  ) {
    return null;
  }
  const e = slots[13];
  if (
    e &&
    (!isSurfaceHex(e[1], 64) ||
      e[2] !== "" ||
      (e[3] !== "command" && e[3] !== "annotation"))
  ) {
    return null;
  }
  if (type === "annotation" && e?.[3] !== "annotation") return null;
  const page = slots[14]?.[1] ?? null;
  const title = slots[15]?.[1] ?? null;
  if (surface === "preview") {
    if (page === null || !isValidPreviewPage(page)) return null;
    if (title !== null && !isValidTitle(title)) return null;
  } else if (page !== null || title !== null) {
    return null;
  }
  if (utf8Bytes(event.content) > 1024 || isHostLocalValue(event.content)) {
    return null;
  }
  return {
    id: event.id,
    signer: event.pubkey,
    createdAt: event.created_at,
    channelId,
    type,
    surface,
    key,
    sha256,
    url,
    mime: mime as SurfaceSnapshotCard["mime"],
    dim,
    takenAt,
    machine,
    requestedBy: p ? p[1] : null,
    commit: commitTag
      ? { sha: commitTag[1], dirty: commitTag[2] === "dirty" }
      : null,
    reference: e
      ? { eventId: e[1], marker: e[3] as "command" | "annotation" }
      : null,
    page,
    title,
    alt: event.content,
  };
}

/** Newest first: `takenAt`, then `created_at`, then lower id. */
export function compareSurfaceSnapshotsNewestFirst(
  left: SurfaceSnapshotCard,
  right: SurfaceSnapshotCard,
): number {
  return (
    right.takenAt - left.takenAt ||
    right.createdAt - left.createdAt ||
    left.id.localeCompare(right.id)
  );
}

/** "commit 1a2b3c4", "commit 1a2b3c4 · dirty", or "commit not recorded". */
export function surfaceSnapshotCommitLabel(
  commit: SurfaceSnapshotCard["commit"],
): string {
  if (!commit) return "commit not recorded";
  return `commit ${commit.sha.slice(0, 7)}${commit.dirty ? " · dirty" : ""}`;
}

/**
 * The 44253 id a verdict token names: `snapshot:<id>` or the `preview:<id>`
 * alias (`parse_snapshot_evidence_token`), else `null`.
 */
export function parseSurfaceSnapshotToken(token: string): string | null {
  const text = token.trim();
  for (const prefix of ["snapshot:", "preview:"]) {
    if (text.startsWith(prefix)) {
      const id = text.slice(prefix.length);
      return isSurfaceHex(id, 64) ? id : null;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// kind 30626 — preview announce and the owner fold
// ---------------------------------------------------------------------------

export type SessionPreviewAnnounce = {
  eventId: string;
  signer: string;
  createdAt: number;
  channelId: string;
  sessionRef: string;
  status: "open" | "closed";
  targetKey: string | null;
  /** The hosting machine's provider key; `null` → "machine not recorded". */
  provider: string | null;
  /** Present on `open`; a close carries no page (NIP-SP). */
  page: string | null;
  /** Present on `open` (may be empty); a close carries no title. */
  title: string | null;
  viewport: { width: number; height: number };
  stream: "frames" | "snapshots";
};

const ANNOUNCE_LAYOUT: readonly SurfaceTagSlot[] = [
  surfaceRequired("h"),
  surfaceRequired("d"),
  surfaceRequired("spa-v"),
  surfaceRequired("status"),
  surfaceOptional("cs-target"),
  surfaceOptional("provider"),
  // Required on `open`, forbidden on `closed` (checked after matching).
  surfaceOptional("page"),
  surfaceOptional("title"),
  surfaceRequired("viewport"),
  surfaceRequired("stream"),
  surfaceRequired("input"),
];

/** Parse one 30626 strictly, or `null`. */
export function parseSessionPreviewAnnounce(
  event: RelayEvent,
): SessionPreviewAnnounce | null {
  if (event.kind !== KIND_SESSION_PREVIEW_ANNOUNCE || event.content !== "") {
    return null;
  }
  const slots = matchSurfaceTags(event.tags, ANNOUNCE_LAYOUT);
  if (!slots) return null;
  if (!surfaceTagsPassHostLocal(event.tags, ["h", "d", "cs-target"])) {
    return null;
  }
  const value = (index: number) => slots[index]?.[1] ?? "";
  const channelId = value(0);
  const sessionRef = value(1);
  if (!isCanonicalUuid(channelId) || !isCanonicalUuid(sessionRef)) return null;
  if (value(2) !== "1") return null;
  const status = value(3);
  if (status !== "open" && status !== "closed") return null;
  const targetKey = slots[4]?.[1] ?? null;
  if (targetKey !== null && !isCodingSessionTargetKey(targetKey)) return null;
  const provider = slots[5]?.[1] ?? null;
  if (provider !== null && !isSurfaceHex(provider, 64)) return null;
  const page = slots[6]?.[1] ?? null;
  const title = slots[7]?.[1] ?? null;
  if (status === "open") {
    if (page === null || !isValidPreviewPage(page)) return null;
    if (title === null || !isValidTitle(title)) return null;
  } else if (page !== null || title !== null) {
    return null;
  }
  const viewport = parseSurfaceDim(value(8));
  if (!viewport) return null;
  const stream = value(9);
  if (stream !== "frames" && stream !== "snapshots") return null;
  if (value(10) !== "synthetic") return null;
  return {
    eventId: event.id,
    signer: event.pubkey,
    createdAt: event.created_at,
    channelId,
    sessionRef,
    status,
    targetKey,
    provider,
    page,
    title,
    viewport,
    stream,
  };
}

/**
 * `resolve_preview_owner`: each signer's newest valid 30626 for (h, d) —
 * newest `created_at`, tie the lower id — then among those that are `open`,
 * the earliest `created_at` (tie: lower id). Returns that announce, or null.
 */
export function resolveSessionPreviewOwner(
  events: readonly RelayEvent[],
  channelId: string,
  sessionRef: string,
): SessionPreviewAnnounce | null {
  const newest = new Map<string, SessionPreviewAnnounce>();
  for (const event of events) {
    const announce = parseSessionPreviewAnnounce(event);
    if (!announce) continue;
    if (
      announce.channelId !== channelId ||
      announce.sessionRef !== sessionRef
    ) {
      continue;
    }
    const held = newest.get(announce.signer);
    if (
      !held ||
      announce.createdAt > held.createdAt ||
      (announce.createdAt === held.createdAt && announce.eventId < held.eventId)
    ) {
      newest.set(announce.signer, announce);
    }
  }
  let owner: SessionPreviewAnnounce | null = null;
  for (const announce of newest.values()) {
    if (announce.status !== "open") continue;
    if (
      !owner ||
      announce.createdAt < owner.createdAt ||
      (announce.createdAt === owner.createdAt &&
        announce.eventId < owner.eventId)
    ) {
      owner = announce;
    }
  }
  return owner;
}

// ---------------------------------------------------------------------------
// Remote preview view model (`SessionPreviewRemoteView`)
// ---------------------------------------------------------------------------

/** The observer facts the remote view reads (`useSurfaceObserver`). */
export type SurfaceObserverView = {
  status:
    | "connecting"
    | "live"
    | "not-streaming"
    | "stalled"
    | "paused"
    | "ended";
  /** Unix ms of the frame shown (its captured time, never later than receipt). */
  frameAt: number | null;
  cadenceMs: number | null;
  /** Who sent input in the last 5 s, from the frame's `actor` tag. */
  actor: string | null;
};

export type RemotePreviewState =
  | "live"
  | "paused"
  | "snapshot"
  | "stalled"
  | "not-streaming"
  | "connecting"
  | "none";

export const REMOTE_PREVIEW_NONE_TEXT =
  "No preview is shared for this session.";
export const REMOTE_PREVIEW_NONE_HINT =
  "The Browser runs in the Beekeeper app on the machine running the agent.";
export const SURFACE_HOST_DID_NOT_ANSWER = "The host did not answer.";

/**
 * One status for the remote Browser, per the lane contract:
 * - no open 30626 → `none`;
 * - frames arriving → `live` ("Live from <name>'s computer · every 2 s · 4 s ago");
 * - `t=paused` → `paused` ("Paused by <name>");
 * - last frame over 30 s old → `stalled`, never "Live";
 * - the host shares snapshots only (`stream=snapshots`), or stopped (`t=end`),
 *   with a snapshot to show → `snapshot` ("Snapshot · 14:02 · not live");
 * - no frame within 10 s of watching → `not-streaming` ("Host not streaming");
 * - before that, `connecting` (with the newest snapshot, `snapshot`).
 */
export function deriveRemotePreviewView(input: {
  owner: SessionPreviewAnnounce | null;
  observer: SurfaceObserverView | null;
  newestSnapshot: SurfaceSnapshotCard | null;
  now: number;
  nameOf?: SurfaceNameOf;
}): { state: RemotePreviewState; text: string; hint: string | null } {
  const { owner, observer, newestSnapshot, now } = input;
  if (!owner) {
    return {
      state: "none",
      text: REMOTE_PREVIEW_NONE_TEXT,
      hint: REMOTE_PREVIEW_NONE_HINT,
    };
  }
  const name = surfacePersonName(owner.signer, input.nameOf);
  const snapshotView = newestSnapshot
    ? {
        state: "snapshot" as const,
        text: `Snapshot · ${formatSurfaceClock(newestSnapshot.takenAt)} · not live`,
        hint: null,
      }
    : null;
  const status = observer?.status ?? "connecting";
  if (status === "live" && observer?.frameAt != null) {
    const cadence =
      observer.cadenceMs !== null
        ? ` · ${formatSurfaceCadence(observer.cadenceMs)}`
        : "";
    return {
      state: "live",
      text: `Live from ${name}'s computer${cadence} · ${formatSurfaceAge(now - observer.frameAt)} ago`,
      hint: null,
    };
  }
  if (status === "paused") {
    return { state: "paused", text: `Paused by ${name}`, hint: null };
  }
  if (status === "stalled" && observer?.frameAt != null) {
    return {
      state: "stalled",
      text: `Stalled · last frame ${formatSurfaceAge(now - observer.frameAt)} ago`,
      hint: null,
    };
  }
  if (owner.stream === "snapshots" || status === "ended") {
    if (snapshotView) return snapshotView;
  }
  if (status === "connecting") {
    return (
      snapshotView ?? {
        state: "connecting",
        text: `Connecting to ${name}'s computer…`,
        hint: null,
      }
    );
  }
  return { state: "not-streaming", text: "Host not streaming", hint: null };
}
