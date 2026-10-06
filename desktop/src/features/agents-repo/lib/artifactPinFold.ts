/**
 * The project artifact pin fold — the TypeScript twin of
 * `crates/beekeeper-core/src/project_artifact_pin_fold.rs`, bound to the same
 * vectors (`conformance/project-artifact-pin-fold/`).
 *
 * Rules, normatively, are that directory's `CONTRACT.md`. The same import
 * constraint as `artifactPinOp.ts` applies: relative paths with explicit
 * extensions only, erasable syntax only.
 */
import { decodePinOp, PROJECT_ARTIFACT_PIN_KIND } from "./artifactPinOp.ts";
import type { PinOp, PinTargetKind } from "./artifactPinOp.ts";
import {
  canonicalRepositoryCoordinate,
  normalizeProjectCoordinate,
} from "./agentsRepoDraftFold.ts";

export const PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA =
  "buzz-project-artifact-pin-digest/v1";

/** The subset of a relay event the fold reads. */
export type PinFoldEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** One target the digest reports. */
export type PinRow = {
  /** The file path or folder prefix. */
  target: string;
  targetKind: PinTargetKind;
  /** Whether it shows in every member's sidebar now. */
  pinned: boolean;
  rank: string;
  /** Who pinned it: the author of the winning `pin.set`. */
  by: string;
  updatedAt: number;
};

export type ProjectArtifactPinDigest = {
  schema: typeof PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA;
  project: string;
  repo: string;
  /** Events dropped as malformed or mis-scoped. */
  ignored: number;
  /** Well-formed ops for another repository. */
  otherRepo: number;
  /** `pin.rank` ops naming a target no `pin.set` ever introduced. */
  ranksWithoutPin: number;
  pins: PinRow[];
};

/** The pinned rows of a digest, in order — what a sidebar draws. */
export function pinnedOnly(digest: ProjectArtifactPinDigest): PinRow[] {
  return digest.pins.filter((row) => row.pinned);
}

type Key = { createdAt: number; id: string };

function compareKeys(a: Key, b: Key): number {
  if (a.createdAt !== b.createdAt) return a.createdAt - b.createdAt;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

type Decoded = { key: Key; pubkey: string; op: PinOp };

function singleTag(event: PinFoldEvent, key: string): string | null {
  let found: string | null = null;
  for (const tag of event.tags) {
    if (tag.length === 2 && tag[0] === key) {
      if (found !== null) return null;
      found = tag[1] ?? null;
    }
  }
  return found;
}

type Decode =
  | { kind: "op"; op: PinOp }
  | { kind: "other-repo" }
  | { kind: "ignored" };

function decode(project: string, repo: string, event: PinFoldEvent): Decode {
  if (event.kind !== PROJECT_ARTIFACT_PIN_KIND) return { kind: "ignored" };
  const coordinate = singleTag(event, "a");
  if (coordinate === null) return { kind: "ignored" };
  if (normalizeProjectCoordinate(coordinate) !== project) {
    return { kind: "ignored" };
  }
  const tagRepo = singleTag(event, "ar-repo");
  if (tagRepo === null) return { kind: "ignored" };
  // `canonicalRepositoryCoordinate` answers null unless the tag is already
  // canonical, which is the rule: ingest never stores a variant.
  const canonical = canonicalRepositoryCoordinate(tagRepo);
  if (canonical === null) return { kind: "ignored" };
  const op = decodePinOp(event.content, canonical);
  if (op === null) return { kind: "ignored" };
  if (canonical !== repo) return { kind: "other-repo" };
  return { kind: "op", op };
}

/** One field's winner: the value and the key that set it. */
type Slot<T> = { value: T | null; key: Key | null };

function offer<T>(slot: Slot<T>, key: Key, value: T): void {
  if (slot.key === null || compareKeys(key, slot.key) > 0) {
    slot.key = key;
    slot.value = value;
  }
}

type TargetState = {
  pinned: Slot<boolean>;
  targetKind: Slot<PinTargetKind>;
  rank: Slot<string>;
  /** The author of the winning `pin.set` — who pinned it. */
  by: Slot<string>;
  updatedAt: number;
};

function emptyState(): TargetState {
  return {
    pinned: { value: null, key: null },
    targetKind: { value: null, key: null },
    rank: { value: null, key: null },
    by: { value: null, key: null },
    updatedAt: 0,
  };
}

/** Fold `events` for `project` and its agents repository `repo`. */
export function foldProjectArtifactPins(
  project: string,
  repo: string,
  events: readonly PinFoldEvent[],
): ProjectArtifactPinDigest {
  let ignored = 0;
  let otherRepo = 0;
  let ranksWithoutPin = 0;
  const seen = new Set<string>();
  const ops: Decoded[] = [];
  for (const event of events) {
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    const decoded = decode(project, repo, event);
    if (decoded.kind === "op") {
      ops.push({
        key: { createdAt: event.created_at, id: event.id },
        pubkey: event.pubkey,
        op: decoded.op,
      });
    } else if (decoded.kind === "other-repo") otherRepo += 1;
    else ignored += 1;
  }
  ops.sort((a, b) => compareKeys(a.key, b.key));

  // Introduce every target first, so a `pin.rank` that arrived before its
  // `pin.set` in the log still counts — the order of the bag is not the order
  // of the clock, and a reader must not depend on it.
  const targets = new Map<string, TargetState>();
  for (const decoded of ops) {
    if (decoded.op.content.op === "pin.set") {
      if (!targets.has(decoded.op.content.target)) {
        targets.set(decoded.op.content.target, emptyState());
      }
    }
  }
  for (const decoded of ops) {
    const content = decoded.op.content;
    const state = targets.get(content.target);
    if (!state) {
      ranksWithoutPin += 1;
      continue;
    }
    if (content.op === "pin.set") {
      offer(state.pinned, decoded.key, content.pinned);
      offer(state.targetKind, decoded.key, content.targetKind);
      offer(state.rank, decoded.key, content.rank);
      offer(state.by, decoded.key, decoded.pubkey);
    } else {
      offer(state.rank, decoded.key, content.rank);
    }
    state.updatedAt = Math.max(state.updatedAt, decoded.key.createdAt);
  }

  const pins: PinRow[] = [];
  for (const [target, state] of targets) {
    if (
      state.targetKind.value === null ||
      state.pinned.value === null ||
      state.rank.value === null ||
      state.by.value === null
    ) {
      continue;
    }
    pins.push({
      target,
      targetKind: state.targetKind.value,
      pinned: state.pinned.value,
      rank: state.rank.value,
      by: state.by.value,
      updatedAt: state.updatedAt,
    });
  }
  pins.sort((a, b) => {
    if (a.rank !== b.rank) return a.rank < b.rank ? -1 : 1;
    return a.target < b.target ? -1 : a.target > b.target ? 1 : 0;
  });

  return {
    schema: PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA,
    project,
    repo,
    ignored,
    otherRepo,
    ranksWithoutPin,
    pins,
  };
}
