/**
 * Which persona pack a seat actually staged, as the host observed it.
 *
 * Packs live in a git repository on hive (LANE-L23), announced by a project's
 * kind:30624 source record and pinned by role. The host resolves one pack —
 * the *seat's* role picks the tree, never the actor's home role — and records
 * the result on the seat's kind:44223 metadata as the additive optional
 * `packRef` key. This module is the reader for that key and the frozen copy
 * for the surface that discloses it, mirroring `codingSessionSeatBee.ts`'s
 * `beeStamp` pattern exactly (same finding-31 discipline: additive, optional,
 * an explicit `null` refused).
 *
 * One rule this differs from `beeStamp` on, deliberately: a `beeStamp`'s
 * absence renders nothing, because an older host is not "unknown". A
 * `packRef`'s absence is disclosed as "no pack staged" — a fact a founder
 * needs to see, not a silence to render past — because the staging rule
 * itself says "no 30624 → today's behaviour (checkout packs only)", and that
 * is real information about *this* seat, not merely an older build.
 *
 * **Setup-lives-inside-the-app addendum (2026-09-03).** A project with no
 * 30624 source is no longer "no pack": the app bundles the seven role packs
 * at build time and stages them as the fallback, and the host discloses that
 * fallback the same way it discloses a real git-sourced pack — a `packRef`
 * with `repo: "app:shipped"` and `sha` carrying the app's own version instead
 * of a commit. `readPackRef`/`codingSessionSeatPackLine` below read and word
 * that shape exactly like a real one; nothing downstream needs to special-
 * case it beyond this module.
 */

/** The literal `repo` value a shipped-defaults `packRef` carries. */
export const PACK_REF_SHIPPED_REPO = "app:shipped";

/** One seat's staged pack, exactly as the wire carries it. */
export type PackRef = {
  /**
   * The packs repository announcement coordinate, `30617:<owner-hex>:<id>`
   * — or the literal {@link PACK_REF_SHIPPED_REPO} when the host fell back
   * to the app's own bundled packs (no 30624 source for the project).
   */
  repo: string;
  /**
   * The exact 40-hex commit the pack was staged from — or, when `repo` is
   * {@link PACK_REF_SHIPPED_REPO}, the app's own version string.
   */
  sha: string;
  /** The seat's own role slug — the pack that was staged, never the actor's home role. */
  role: string;
  /** The path staged within the repo, typically `<base path>/<role>`. */
  path: string;
};

const PACK_REF_KEYS = ["repo", "sha", "role", "path"] as const;

/**
 * How the staged pack named by `packRef` was composed (spec § 4.6): the app
 * version whose template catalog resolved its includes, and the digest of
 * the bytes that ran. Present only beside a `packRef`; never on its own.
 */
export type ComposeRef = {
  /** The app version whose template catalog composed the pack. */
  appVersion: string;
  /** `sha256:<64 lowercase hex>` over the staged persona and skill files. */
  digest: string;
};

const COMPOSE_REF_KEYS = ["appVersion", "digest"] as const;
const COMPOSE_DIGEST = /^sha256:[0-9a-f]{64}$/;

const EXACT_SHA = /^[0-9a-f]{40}$/;
const ROLE_SLUG = /^[a-z0-9-]{1,64}$/;
/** `30617:<64-hex>:<dtag>` — a git repository announcement coordinate. */
const REPO_COORD = /^30617:[0-9a-f]{64}:[a-zA-Z0-9._-]{1,200}$/;
/** A loose semver-ish app version — digits/dots, optional `-`/`+` suffix. */
const APP_VERSION = /^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/;

/** Whether a `packRef`'s `repo` is the app's own bundled fallback. */
export function isShippedPackRef(packRef: Pick<PackRef, "repo">): boolean {
  return packRef.repo === PACK_REF_SHIPPED_REPO;
}

/**
 * Read a `packRef` from an untrusted wire value, or `null` for "no stamp".
 *
 * Strict on purpose, same discipline as `readSeatBeeStamp`: exactly the four
 * known keys, each on its own terms. A shape this reader does not fully
 * recognise yields `null` — never a partially guessed pack that would let a
 * chip claim a tree the host never staged. `repo`/`sha` accept exactly two
 * shapes: a real `30617:...` coordinate paired with a 40-hex commit, or the
 * literal `app:shipped` paired with a version string — never a coordinate
 * with a version, or `app:shipped` with a commit sha.
 */
export function readPackRef(value: unknown): PackRef | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== PACK_REF_KEYS.length) return null;
  for (const key of PACK_REF_KEYS) {
    if (!Object.hasOwn(record, key)) return null;
  }
  const { repo, sha, role, path } = record;
  if (typeof repo !== "string" || typeof sha !== "string") return null;
  const shipped = repo === PACK_REF_SHIPPED_REPO;
  if (!shipped && !REPO_COORD.test(repo)) return null;
  if (shipped ? !APP_VERSION.test(sha) : !EXACT_SHA.test(sha)) return null;
  if (typeof role !== "string" || !ROLE_SLUG.test(role)) return null;
  if (typeof path !== "string" || path.length === 0 || path.length > 512) {
    return null;
  }
  return { repo, sha, role, path };
}

/**
 * Read a `composeRef` exactly as the Rust decoder would, or `null` for
 * anything else — a missing key, an extra key, a digest that is not
 * `sha256:` plus sixty-four lowercase hex characters, or a blank version.
 */
export function readComposeRef(value: unknown): ComposeRef | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== COMPOSE_REF_KEYS.length) return null;
  for (const key of COMPOSE_REF_KEYS) {
    if (!Object.hasOwn(record, key)) return null;
  }
  const { appVersion, digest } = record;
  if (typeof appVersion !== "string" || typeof digest !== "string") return null;
  const version = appVersion.trim();
  if (version.length === 0 || version.length > 64) return null;
  if (!COMPOSE_DIGEST.test(digest)) return null;
  return { appVersion, digest };
}

/**
 * The frozen seat-card pack line — never `null`, unlike the bee line.
 *
 * `pack builder@a1b2c3d4` (role, then the sha's first 8 hex) for a real
 * git-sourced pack; `pack builder (shipped defaults v0.1.0)` for the app's
 * own bundled fallback; `no pack staged` when the wire carried none at all
 * (an older host, before this key existed). Words, never a colour.
 */
export function codingSessionSeatPackLine(packRef: PackRef | null): string {
  if (packRef === null) return "no pack staged";
  if (isShippedPackRef(packRef)) {
    return `pack ${packRef.role} (shipped defaults v${packRef.sha})`;
  }
  return `pack ${packRef.role}@${packRef.sha.slice(0, 8)}`;
}

/**
 * Where a just-staged seat's pack came from, for the screen the create was
 * made on (finding 84): `staged from 30617:3d3b7169…:agiterra-packs@dd935f43`
 * names the project's repository and commit; the shipped fallback is named
 * as such; `null` when staging reported no repository — a pack this computer
 * alone vouches for, a packless seat, or a backend that did not say — because
 * a provenance line with nothing behind it would be the guess this exists to
 * replace.
 */
export function codingSessionSeatStagedFromLine(
  packRef: PackRef | null,
): string | null {
  if (packRef === null) return null;
  if (isShippedPackRef(packRef)) {
    return `staged from shipped defaults v${packRef.sha}`;
  }
  const [kind, owner, ...id] = packRef.repo.split(":");
  const repo =
    kind === "30617" && owner !== undefined && owner.length === 64
      ? `${kind}:${owner.slice(0, 8)}…:${id.join(":")}`
      : packRef.repo;
  return `staged from ${repo}@${packRef.sha.slice(0, 8)}`;
}

/**
 * The one generation shape {@link derivePackRefs} needs — deliberately not
 * `CodingSessionCatalogRecord` itself, so this selector stays testable with
 * plain fixtures, matching `SeatBeeGenerationSource`'s own reasoning.
 */
export type PackRefGenerationSource = {
  packRef: PackRef | null;
};

/** The one execution shape {@link derivePackRefs} needs. */
export type PackRefExecutionSource = {
  executionKey: string;
  activeGeneration: PackRefGenerationSource;
  /** Earlier generations, ascending — same order `CodingSessionExecution` keeps. */
  priorGenerations: readonly PackRefGenerationSource[];
};

/**
 * One seat's pack, keyed by `executionKey` — the map
 * `CodingSessionParticipantBar`'s `seatPackRefs` prop takes directly.
 *
 * Same newest-stamp-wins fold as `deriveSeatBeeStamps`: the active generation
 * is checked first, and prior generations are checked newest first, so a
 * fresh resume that has not yet republished metadata still shows the most
 * recent pack this execution ever staged rather than a stale null shadowing
 * it.
 *
 * A seat none of whose generations ever carried a `packRef` maps to `null` —
 * `codingSessionSeatPackLine` renders that as `no pack staged`, the honest
 * default for an older host. A host on this build always resolves *some*
 * `packRef` (real or shipped), so `null` here specifically means "an older
 * host" once every provider on this build carries the addendum.
 */
export function derivePackRefs(
  executions: readonly PackRefExecutionSource[],
): Map<string, PackRef | null> {
  const packs = new Map<string, PackRef | null>();
  for (const execution of executions) {
    if (execution.activeGeneration.packRef !== null) {
      packs.set(execution.executionKey, execution.activeGeneration.packRef);
      continue;
    }
    let newest: PackRef | null = null;
    for (let i = execution.priorGenerations.length - 1; i >= 0; i -= 1) {
      const packRef = execution.priorGenerations[i].packRef;
      if (packRef !== null) {
        newest = packRef;
        break;
      }
    }
    packs.set(execution.executionKey, newest);
  }
  return packs;
}
