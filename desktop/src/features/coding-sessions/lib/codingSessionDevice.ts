/**
 * The Device surface's records, folded (WIRE-C5 § 7; SV-34): one channel's
 * 44255 records, 44254 commands and device 44253 snapshots, read strictly,
 * and the one status the surface shows for them.
 *
 * Trust: the relay checks 44254/44255 structure, not standing. So a record
 * counts only when its signer is the provider that runs the generation it
 * names (`cs-target`) — the `authorityFor` lookup the hook builds from the
 * session's executions. A record for a generation this client cannot place
 * is dropped, never guessed at. The slot's frame authority is then the
 * signer of its newest `state` (WIRE-C5 § 4).
 *
 * No UDID, path or hostname is read: none is on the wire, and any value
 * shaped like one drops the record (§ 2).
 */
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_SESSION_DEVICE_COMMAND,
  KIND_SESSION_DEVICE_RECORD,
  KIND_SURFACE_SNAPSHOT,
} from "@/shared/constants/kinds";

import {
  compareSurfaceSnapshotsNewestFirst,
  formatSurfaceAge,
  formatSurfaceCadence,
  formatSurfaceClock,
  isCanonicalUuid,
  isCodingSessionTargetKey,
  isHostLocalValue,
  isSurfaceHex,
  matchSurfaceTags,
  parseSurfaceSnapshot,
  type SurfaceNameOf,
  type SurfaceObserverView,
  type SurfaceSnapshotCard,
  surfaceOptional,
  surfacePersonName,
  surfaceRequired,
  surfaceTagsPassHostLocal,
} from "./codingSessionSurfaceSnapshot";

const COMMAND_ID = /^[A-Za-z0-9._-]{1,64}$/;
const UUID_EXEMPT = ["h", "cs-target", "csl-command", "sdv-cmd"];

export type DevicePlatformAvailability = {
  available: boolean;
  reason: string | null;
};

export type DeviceAvailability = {
  eventId: string;
  signer: string;
  createdAt: number;
  targetKey: string;
  ios: DevicePlatformAvailability;
  android: DevicePlatformAvailability;
  agentDevice: {
    installed: boolean;
    version: string | null;
    reason: string | null;
  };
};

export type DeviceSlotState = {
  eventId: string;
  signer: string;
  createdAt: number;
  targetKey: string;
  slot: string;
  commandId: string | null;
  state: "booting" | "open" | "closed" | "failed";
  platform: "ios" | "android";
  model: string;
  osVersion: string;
  drivers: ("agent" | "host-owner")[];
  capture: { mode: string; maxIntervalMs: number };
  reason: string | null;
};

export type DeviceShot = {
  eventId: string;
  createdAt: number;
  targetKey: string;
  slot: string;
  commandId: string;
  snapshotId: string;
};

export type DeviceCommand = {
  eventId: string;
  author: string;
  createdAt: number;
  targetKey: string;
  commandId: string;
  slot: string | null;
  op: "open" | "close" | "screenshot" | "action";
};

/** One parsed 44255, before trust. */
type DeviceRecord =
  | { type: "availability"; value: DeviceAvailability }
  | { type: "state"; value: DeviceSlotState }
  | { type: "shot"; value: DeviceShot }
  | {
      type: "refused";
      eventId: string;
      createdAt: number;
      targetKey: string;
      commandId: string;
    };

const RECORD_LAYOUT = [
  surfaceRequired("h"),
  surfaceRequired("sdv-v"),
  surfaceRequired("cs-target"),
  surfaceRequired("csl-command"),
  surfaceRequired("sdv-type"),
  surfaceOptional("sdv-slot"),
  surfaceOptional("sdv-cmd"),
  surfaceOptional("e", 4),
];

const COMMAND_LAYOUT = [
  surfaceRequired("h"),
  surfaceRequired("sdv-v"),
  surfaceRequired("cs-target"),
  surfaceRequired("sdv-cmd"),
  surfaceOptional("sdv-slot"),
];

function readJson(content: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(content);
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function onlyKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = [],
): boolean {
  const keys = Object.keys(value);
  return (
    required.every((key) => key in value) &&
    keys.every((key) => required.includes(key) || optional.includes(key))
  );
}

function optionalString(value: unknown): string | null | undefined {
  if (value === undefined) return null;
  return typeof value === "string" ? value : undefined;
}

/** Every JSON string in `value` passes § 2. */
function stringsPassHostLocal(value: unknown): boolean {
  if (typeof value === "string") return !isHostLocalValue(value);
  if (Array.isArray(value)) return value.every(stringsPassHostLocal);
  if (value && typeof value === "object") {
    return Object.values(value).every(stringsPassHostLocal);
  }
  return true;
}

function parsePlatform(value: unknown): DevicePlatformAvailability | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const entry = value as Record<string, unknown>;
  if (!onlyKeys(entry, ["available"], ["reason"])) return null;
  const reason = optionalString(entry.reason);
  if (typeof entry.available !== "boolean" || reason === undefined) return null;
  return { available: entry.available, reason };
}

/** Parse one 44255 strictly, or `null`. */
function parseDeviceRecord(event: RelayEvent): DeviceRecord | null {
  if (event.kind !== KIND_SESSION_DEVICE_RECORD) return null;
  if (new TextEncoder().encode(event.content).length > 4 * 1024) return null;
  const slots = matchSurfaceTags(event.tags, RECORD_LAYOUT);
  if (!slots || !surfaceTagsPassHostLocal(event.tags, UUID_EXEMPT)) return null;
  const value = (index: number) => slots[index]?.[1] ?? "";
  if (!isCanonicalUuid(value(0)) || value(1) !== "sdv1") return null;
  const targetKey = value(2);
  if (!isCodingSessionTargetKey(targetKey) || !COMMAND_ID.test(value(3))) {
    return null;
  }
  const type = value(4);
  const slot = slots[5]?.[1] ?? null;
  const commandId = slots[6]?.[1] ?? null;
  const e = slots[7];
  if (slot !== null && !isSurfaceHex(slot, 16)) return null;
  if (commandId !== null && !COMMAND_ID.test(commandId)) return null;
  if (e && (!isSurfaceHex(e[1], 64) || e[2] !== "" || e[3] !== "snapshot")) {
    return null;
  }
  const content = readJson(event.content);
  if (!content || content.type !== type || !stringsPassHostLocal(content)) {
    return null;
  }
  const base = {
    eventId: event.id,
    signer: event.pubkey,
    createdAt: event.created_at,
    targetKey,
  };
  if (type === "availability") {
    if (slot !== null || commandId !== null || e) return null;
    if (!onlyKeys(content, ["type", "platforms", "agentDevice"])) return null;
    const platforms = content.platforms as Record<string, unknown> | null;
    if (!platforms || typeof platforms !== "object") return null;
    if (!onlyKeys(platforms, ["ios", "android"])) return null;
    const ios = parsePlatform(platforms.ios);
    const android = parsePlatform(platforms.android);
    const agent = content.agentDevice as Record<string, unknown> | null;
    if (!ios || !android || !agent || typeof agent !== "object") return null;
    if (!onlyKeys(agent, ["installed"], ["version", "reason"])) return null;
    const version = optionalString(agent.version);
    const reason = optionalString(agent.reason);
    if (
      typeof agent.installed !== "boolean" ||
      version === undefined ||
      reason === undefined
    ) {
      return null;
    }
    return {
      type,
      value: {
        ...base,
        ios,
        android,
        agentDevice: { installed: agent.installed, version, reason },
      },
    };
  }
  if (type === "state") {
    if (slot === null || e) return null;
    const required = [
      "type",
      "state",
      "platform",
      "model",
      "osVersion",
      "drivers",
      "capture",
    ];
    if (!onlyKeys(content, required, ["reason"])) return null;
    const state = content.state;
    const platform = content.platform;
    const reason = optionalString(content.reason);
    const drivers = content.drivers;
    const capture = content.capture as Record<string, unknown> | null;
    if (
      (state !== "booting" &&
        state !== "open" &&
        state !== "closed" &&
        state !== "failed") ||
      (platform !== "ios" && platform !== "android") ||
      typeof content.model !== "string" ||
      typeof content.osVersion !== "string" ||
      reason === undefined ||
      !Array.isArray(drivers) ||
      !drivers.every((d) => d === "agent" || d === "host-owner") ||
      !capture ||
      typeof capture !== "object" ||
      !onlyKeys(capture, ["mode", "maxIntervalMs"]) ||
      typeof capture.mode !== "string" ||
      typeof capture.maxIntervalMs !== "number" ||
      !Number.isSafeInteger(capture.maxIntervalMs) ||
      capture.maxIntervalMs < 0
    ) {
      return null;
    }
    return {
      type,
      value: {
        ...base,
        slot,
        commandId,
        state,
        platform,
        model: content.model,
        osVersion: content.osVersion,
        drivers: drivers as DeviceSlotState["drivers"],
        capture: { mode: capture.mode, maxIntervalMs: capture.maxIntervalMs },
        reason,
      },
    };
  }
  if (type === "shot") {
    if (slot === null || commandId === null || !e) return null;
    if (!onlyKeys(content, ["type"])) return null;
    return {
      type,
      value: {
        eventId: event.id,
        createdAt: event.created_at,
        targetKey,
        slot,
        commandId,
        snapshotId: e[1],
      },
    };
  }
  if (type === "refused") {
    if (commandId === null || e) return null;
    if (!onlyKeys(content, ["type", "code", "reason"])) return null;
    if (
      typeof content.code !== "string" ||
      typeof content.reason !== "string"
    ) {
      return null;
    }
    return { type, ...base, commandId };
  }
  return null;
}

const COMMAND_OPS = new Set(["open", "close", "screenshot", "action"]);

/** Parse one 44254 strictly enough to know which command it is, or `null`. */
function parseDeviceCommand(event: RelayEvent): DeviceCommand | null {
  if (event.kind !== KIND_SESSION_DEVICE_COMMAND) return null;
  if (new TextEncoder().encode(event.content).length > 8 * 1024) return null;
  const slots = matchSurfaceTags(event.tags, COMMAND_LAYOUT);
  if (!slots || !surfaceTagsPassHostLocal(event.tags, UUID_EXEMPT)) return null;
  const value = (index: number) => slots[index]?.[1] ?? "";
  if (!isCanonicalUuid(value(0)) || value(1) !== "sdv1") return null;
  const targetKey = value(2);
  const commandId = value(3);
  if (!isCodingSessionTargetKey(targetKey) || !COMMAND_ID.test(commandId)) {
    return null;
  }
  const slot = slots[4]?.[1] ?? null;
  if (slot !== null && !isSurfaceHex(slot, 16)) return null;
  const content = readJson(event.content);
  const op = content?.op;
  if (typeof op !== "string" || !COMMAND_OPS.has(op)) return null;
  if ((op === "open") !== (slot === null)) return null;
  return {
    eventId: event.id,
    author: event.pubkey,
    createdAt: event.created_at,
    targetKey,
    commandId,
    slot,
    op: op as DeviceCommand["op"],
  };
}

/** The channel's device records, trusted and folded. */
export type CodingSessionDeviceFold = {
  /** Per cs-target, the newest valid availability. */
  availabilityByTarget: ReadonlyMap<string, DeviceAvailability>;
  /** Per slot, the newest state (its signer is the slot's frame authority). */
  slots: ReadonlyMap<string, DeviceSlotState>;
  shots: readonly DeviceShot[];
  /** Device 44253s whose signer is a known slot's authority, newest first. */
  snapshots: readonly SurfaceSnapshotCard[];
  /** 44254s with no terminal 44255 carrying their sdv-cmd, newest first. */
  unanswered: readonly DeviceCommand[];
};

export const EMPTY_CODING_SESSION_DEVICE_FOLD: CodingSessionDeviceFold = {
  availabilityByTarget: new Map(),
  slots: new Map(),
  shots: [],
  snapshots: [],
  unanswered: [],
};

function newer(
  left: { createdAt: number; eventId: string },
  right: { createdAt: number; eventId: string } | undefined,
): boolean {
  return (
    !right ||
    left.createdAt > right.createdAt ||
    (left.createdAt === right.createdAt && left.eventId < right.eventId)
  );
}

/**
 * Fold one channel's 44255/44254/44253 events. `authorityFor(targetKey)` is
 * the provider key that runs that generation, or null when unknown; records
 * from any other signer (or for an unknown generation) are dropped.
 */
export function foldCodingSessionDevice(
  events: readonly RelayEvent[],
  input: {
    channelId: string;
    authorityFor: (targetKey: string) => string | null;
  },
): CodingSessionDeviceFold {
  const availability = new Map<string, DeviceAvailability>();
  const slots = new Map<string, DeviceSlotState>();
  const shots: DeviceShot[] = [];
  const answered = new Set<string>();
  const commands: DeviceCommand[] = [];
  const snapshotEvents: RelayEvent[] = [];
  const inChannel = (event: RelayEvent) =>
    event.tags[0]?.[0] === "h" && event.tags[0]?.[1] === input.channelId;
  for (const event of events) {
    if (!inChannel(event)) continue;
    if (event.kind === KIND_SURFACE_SNAPSHOT) {
      snapshotEvents.push(event);
      continue;
    }
    if (event.kind === KIND_SESSION_DEVICE_COMMAND) {
      const command = parseDeviceCommand(event);
      if (command) commands.push(command);
      continue;
    }
    const record = parseDeviceRecord(event);
    if (!record) continue;
    const targetKey =
      record.type === "refused" ? record.targetKey : record.value.targetKey;
    const authority = input.authorityFor(targetKey);
    if (!authority || authority !== event.pubkey) continue;
    if (record.type === "availability") {
      if (newer(record.value, availability.get(targetKey))) {
        availability.set(targetKey, record.value);
      }
    } else if (record.type === "state") {
      const value = record.value;
      if (newer(value, slots.get(value.slot))) slots.set(value.slot, value);
      if (value.commandId && value.state !== "booting") {
        answered.add(`${targetKey}\n${value.commandId}`);
      }
    } else if (record.type === "shot") {
      shots.push(record.value);
      answered.add(`${targetKey}\n${record.value.commandId}`);
    } else {
      answered.add(`${targetKey}\n${record.commandId}`);
    }
  }
  const snapshots = snapshotEvents
    .map(parseSurfaceSnapshot)
    .filter((card): card is SurfaceSnapshotCard => {
      if (card?.surface !== "device") return false;
      const slot = slots.get(card.key);
      return slot !== undefined && slot.signer === card.signer;
    })
    .sort(compareSurfaceSnapshotsNewestFirst);
  const seen = new Set<string>();
  const unanswered = commands
    .filter((command) => {
      const key = `${command.targetKey}\n${command.commandId}`;
      if (answered.has(key) || seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => b.createdAt - a.createdAt);
  shots.sort((a, b) => b.createdAt - a.createdAt);
  return {
    availabilityByTarget: availability,
    slots,
    shots,
    snapshots,
    unanswered,
  };
}

/** Open slots across the session: the badge's count. */
export function codingSessionDeviceOpenCount(
  fold: CodingSessionDeviceFold,
): number {
  let count = 0;
  for (const slot of fold.slots.values()) if (slot.state === "open") count += 1;
  return count;
}

// ---------------------------------------------------------------------------
// Surface view model
// ---------------------------------------------------------------------------

export type CodingSessionDeviceStatus =
  | "live"
  | "stalled"
  | "snapshot"
  | "closed"
  | "booting"
  | "failed"
  | "no-device"
  | "not-offered"
  | "unavailable"
  | "waiting"
  | "connecting"
  | "not-streaming";

/** A command unanswered this long reads "No answer from <machine>". */
export const DEVICE_COMMAND_ANSWER_GRACE_MS = 15_000;

export const DEVICE_NOT_OFFERED_REASON =
  "This machine's provider does not offer devices.";
export const DEVICE_NO_DEVICE_HINT =
  "The agent opens one with bee device open.";

export type CodingSessionDeviceView = {
  status: CodingSessionDeviceStatus;
  /** `<model> · iOS <osVersion> · on <machine>`, or `Device · on <machine>`. */
  header: string;
  statusText: string;
  /** The sentence under the status (not-offered reason, no-device hint). */
  detail: string | null;
  /** `agent-device unavailable: <reason>. Snapshots still work.` */
  agentDeviceNote: string | null;
  /** The slot shown, when one exists. */
  slot: DeviceSlotState | null;
  /** Watch this slot's frames: it is open. */
  watch: { slot: string; producerPubkey: string } | null;
  /** Show the frame (live or stalled); else the newest snapshot. */
  showFrame: boolean;
  newestSnapshot: SurfaceSnapshotCard | null;
  /** The shown slot's snapshots, newest first. */
  snapshots: readonly SurfaceSnapshotCard[];
};

function platformLabel(slot: DeviceSlotState): string {
  return slot.platform === "ios"
    ? `iOS ${slot.osVersion}`
    : `Android ${slot.osVersion}`;
}

/**
 * The one status the Device surface shows for one generation (`targetKey`),
 * given the fold, the frame observer, and `now` (ms). `machine` is the
 * provider's name as the Agents surface says it ("this computer", a name, or
 * a short key) — never a hostname.
 */
export function deriveCodingSessionDeviceView(input: {
  fold: CodingSessionDeviceFold;
  targetKey: string | null;
  machine: string;
  observer: SurfaceObserverView | null;
  now: number;
  nameOf?: SurfaceNameOf;
}): CodingSessionDeviceView {
  const { fold, targetKey, machine, observer, now } = input;
  const availability = targetKey
    ? (fold.availabilityByTarget.get(targetKey) ?? null)
    : null;
  let slot: DeviceSlotState | null = null;
  for (const candidate of fold.slots.values()) {
    if (candidate.targetKey !== targetKey) continue;
    // An open slot outranks a closed one; otherwise the newer state wins.
    const candidateOpen = candidate.state === "open";
    const better =
      !slot ||
      (candidateOpen && slot.state !== "open") ||
      (candidateOpen === (slot.state === "open") && newer(candidate, slot));
    if (better) slot = candidate;
  }
  const snapshots = slot
    ? fold.snapshots.filter((card) => card.key === slot.slot)
    : [];
  const newestSnapshot = snapshots[0] ?? null;
  const header = slot
    ? `${slot.model} · ${platformLabel(slot)} · on ${machine}`
    : `Device · on ${machine}`;
  const agentDeviceNote =
    availability && !availability.agentDevice.installed
      ? `agent-device unavailable: ${availability.agentDevice.reason ?? "not installed"}. Snapshots still work.`
      : null;
  const view = (
    status: CodingSessionDeviceStatus,
    statusText: string,
    extra: Partial<CodingSessionDeviceView> = {},
  ): CodingSessionDeviceView => ({
    status,
    header,
    statusText,
    detail: null,
    agentDeviceNote,
    slot,
    watch: null,
    showFrame: false,
    newestSnapshot,
    snapshots,
    ...extra,
  });

  if (!availability) {
    return view("not-offered", `Not offered by ${machine}`, {
      detail: DEVICE_NOT_OFFERED_REASON,
      agentDeviceNote: null,
    });
  }
  if (!availability.ios.available) {
    const reason = availability.ios.reason ?? "iOS Simulator not available";
    return view("unavailable", `Not offered by ${machine}: ${reason}`);
  }
  const waiting = fold.unanswered.find(
    (command) =>
      command.targetKey === targetKey &&
      now - command.createdAt * 1_000 >= DEVICE_COMMAND_ANSWER_GRACE_MS &&
      (!slot || command.createdAt >= slot.createdAt),
  );
  const booting = slot?.state === "booting";
  if (waiting && !booting) {
    const age = formatSurfaceAge(now - waiting.createdAt * 1_000);
    return view("waiting", `No answer from ${machine} · ${age}`);
  }
  if (!slot) {
    return view("no-device", "No device open", {
      detail: DEVICE_NO_DEVICE_HINT,
    });
  }
  if (slot.state === "booting") return view("booting", "Booting…");
  if (slot.state === "failed") {
    return view("failed", `Failed: ${slot.reason ?? "no reason given"}`);
  }
  if (slot.state === "closed") return view("closed", "Closed");

  const watch = { slot: slot.slot, producerPubkey: slot.signer };
  const status = observer?.status ?? "connecting";
  if (status === "live" && observer?.frameAt != null) {
    const cadence = observer.cadenceMs ?? slot.capture.maxIntervalMs;
    return view("live", `Live · ${formatSurfaceCadence(cadence)}`, {
      watch,
      showFrame: true,
    });
  }
  if (status === "stalled" && observer?.frameAt != null) {
    const age = formatSurfaceAge(now - observer.frameAt);
    return view("stalled", `Stalled · last frame ${age} ago`, {
      watch,
      showFrame: true,
    });
  }
  if (newestSnapshot) {
    const who = surfacePersonName(
      newestSnapshot.requestedBy ?? newestSnapshot.signer,
      input.nameOf,
    );
    const by =
      newestSnapshot.requestedBy === null &&
      newestSnapshot.signer === slot.signer
        ? machine
        : who;
    return view(
      "snapshot",
      `Snapshot · ${formatSurfaceClock(newestSnapshot.takenAt)} by ${by}`,
      { watch },
    );
  }
  if (status === "connecting") {
    return view("connecting", `Connecting to ${machine}…`, { watch });
  }
  return view("not-streaming", `Not streaming · no snapshot yet`, { watch });
}
