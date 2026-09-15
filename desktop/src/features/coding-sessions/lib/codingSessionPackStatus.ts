/**
 * "What pack would this seat stage?" — the preview the hire/launch dialogs
 * show before anything is created (LANE-L23).
 *
 * The host answers it with `preview_coding_session_seat_pack`
 * (`desktop/src-tauri/src/managed_agents/actor_seats.rs`), which runs the very
 * function the create path stages with — `plan_seat_pack` — so the dialog
 * cannot promise a pack the create then fails to stage. It syncs the project's
 * packs cache and writes nothing.
 *
 * That command speaks the host's vocabulary: an `origin` word for which rung
 * of the staging ladder answered, the seat's `packRef` when a repository can
 * vouch for the pack, and a `refusal`/`reason` pair when the project names
 * packs this computer could not read. This module is the one place that maps
 * it into the shape the form renders, so every caller reads one vocabulary.
 *
 * Two facts the host cannot supply on its own, and which this module resolves
 * first:
 *
 * - **the pack source.** `plan_seat_pack` is given a decoded kind:30624; it
 *   does not query the relay. So this module reads the project's newest 30624
 *   ({@link fetchCodingSessionSeatPackSource} — the same reader the create
 *   stages with, finding 84) and hands it over. A project with none gets
 *   `hasSource: false` and the ladder's local rungs.
 * - **the seat.** The preview is about one managed agent seated at one role,
 *   so an `agentPubkey` is required. Without a chosen agent there is no seat
 *   to preview, and the form shows nothing rather than a generic answer.
 */
import { invokeTauri } from "@/shared/api/tauri";

import { fetchCodingSessionSeatPackSource } from "./codingSessionSeatPackSource";

/** The host command this module calls. */
export const CODING_SESSION_PACK_STATUS_COMMAND =
  "preview_coding_session_seat_pack";
/** Closed schema this module's own result carries, so a caller can assert it. */
export const CODING_SESSION_PACK_STATUS_SCHEMA =
  "buzz-coding-session-pack-status/v1";

/**
 * Which rung of the staging ladder answered, in the host's own words
 * (`SeatPackOrigin`, `actor_seats.rs`).
 */
export type CodingSessionPackOrigin =
  | "project"
  | "checkout"
  | "installed"
  | "shipped"
  | "none";

const ORIGINS: readonly CodingSessionPackOrigin[] = [
  "project",
  "checkout",
  "installed",
  "shipped",
  "none",
];

/**
 * What this machine would stage for one role, on the project's current 30624
 * source — a preview, never a claim about what any past or future seat
 * actually ran.
 */
export type CodingSessionPackStatusResult = {
  readonly schema: typeof CODING_SESSION_PACK_STATUS_SCHEMA;
  readonly implementation: "buzz-core";
  /** Whether the project has a 30624 pack-source record at all. */
  readonly hasSource: boolean;
  /** The packs repository coordinate the source names, or `null` without one. */
  readonly repo: string | null;
  /** The resolved commit — the pinned sha, or the ref's tip as last fetched — or `null`. */
  readonly sha: string | null;
  /** The base path within the repo (default `personas/roles`), or `null`. */
  readonly path: string | null;
  /** The role this preview was asked about — echoed, never re-derived. */
  readonly role: string;
  /** `<path>/<role>` — the exact tree that would be staged, or `null` without a source. */
  readonly rolePath: string | null;
  /** Whether that role's directory actually exists in the resolved packs checkout. */
  readonly roleFound: boolean;
  /**
   * Whether the *session's own* checkout carries an overlay for this role —
   * the staging rule's "checkout wins" layer, disclosed so a founder does not
   * mistake a local override for the shared source.
   */
  readonly overlayFromCheckout: boolean;
  /** A human sentence disclosing the state, in the host's own words. */
  readonly note: string;
};

/**
 * The host's answer, exactly as `SeatPackPreview` serializes it.
 *
 * Read-optional nothing: every key is present on the wire, and a response
 * missing one is a boundary this reader does not recognise.
 */
type SeatPackPreview = {
  packStaged: boolean;
  origin: CodingSessionPackOrigin;
  role: string | null;
  packDir: string | null;
  personaId: string | null;
  packRef: {
    repo: string;
    sha: string;
    role: string;
    path: string;
  } | null;
  refusal: string | null;
  reason: string | null;
};

function isOptionalString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isPackRef(value: unknown): value is SeatPackPreview["packRef"] {
  if (value === null) return true;
  if (typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.repo === "string" &&
    typeof record.sha === "string" &&
    typeof record.role === "string" &&
    typeof record.path === "string"
  );
}

function isSeatPackPreview(value: unknown): value is SeatPackPreview {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.packStaged === "boolean" &&
    ORIGINS.includes(record.origin as CodingSessionPackOrigin) &&
    isOptionalString(record.role) &&
    isOptionalString(record.packDir) &&
    isOptionalString(record.personaId) &&
    isPackRef(record.packRef) &&
    isOptionalString(record.refusal) &&
    isOptionalString(record.reason)
  );
}

/** Strip the trailing `/<role>` a `packRef.path` ends with, leaving the base. */
function basePath(rolePath: string, role: string): string | null {
  const suffix = `/${role}`;
  return rolePath.endsWith(suffix)
    ? rolePath.slice(0, rolePath.length - suffix.length)
    : null;
}

/**
 * The sentence the form prints.
 *
 * A refusal is the host's own words, unedited — it is the sentence the hire
 * would be refused with, and showing it before the operator commits is the
 * whole point of the preview. Otherwise the sentence names the rung that
 * answered, because "a pack from the project's repository at this commit" and
 * "the packs this app happens to ship" are different promises.
 */
function packStatusNote(preview: SeatPackPreview, role: string): string {
  if (preview.refusal !== null) {
    return preview.reason === null
      ? preview.refusal
      : `${preview.refusal} (${preview.reason})`;
  }
  switch (preview.origin) {
    case "project":
      return preview.packRef === null
        ? `Stages the ${role} pack from this project's packs repository.`
        : `Stages ${preview.packRef.path} from this project's packs repository, at ${preview.packRef.sha.slice(0, 8)}.`;
    case "checkout":
      return `Stages the ${role} pack from this session's own checkout, which overrides the project's packs repository.`;
    case "installed":
      return `Stages the ${role} pack installed on this computer. No repository vouches for it, so the seat's metadata will name none.`;
    case "shipped":
      return `Stages the ${role} pack this build of Beekeeper ships. Give the project a packs repository to version it.`;
    case "none":
      return `No ${role} pack — this seat would run on its persona prompt alone.`;
  }
}

/**
 * Read the host's answer and say what it means for this form.
 *
 * @throws when the boundary answered a shape this reader does not recognise —
 * the caller renders no preview rather than a partial reading.
 */
export function decodeCodingSessionPackStatusResult(
  value: unknown,
  role: string,
): CodingSessionPackStatusResult {
  if (!isSeatPackPreview(value)) {
    throw new Error("native pack-status adapter returned a malformed response");
  }
  const packRef = value.origin === "project" ? value.packRef : null;
  return {
    schema: CODING_SESSION_PACK_STATUS_SCHEMA,
    implementation: "buzz-core",
    hasSource: value.origin === "project",
    repo: packRef?.repo ?? null,
    sha: packRef?.sha ?? null,
    path: packRef === null ? null : basePath(packRef.path, packRef.role),
    // The role this preview was asked about. The host echoes it, but a host
    // that was asked for none answers `null`, and the caller always names one.
    role: value.role ?? role,
    rolePath: packRef?.path ?? null,
    roleFound: value.packStaged,
    overlayFromCheckout: value.origin === "checkout",
    note: packStatusNote(value, value.role ?? role),
  };
}

/**
 * Ask this machine what it would stage for one seat, without staging it.
 *
 * Throws on any failure — no host (a browser build), an agent this computer
 * does not manage, a packs repository that cannot be fetched, or a malformed
 * response. Every caller in this codebase catches that and renders nothing
 * that claims to preview a pack: a build (or a moment) that cannot answer
 * offers no preview, never a fabricated one.
 */
export async function codingSessionPackStatus(input: {
  projectRef: string;
  role: string;
  /** The managed agent being seated — there is no seat to preview without one. */
  agentPubkey: string;
  /** The session checkout that may overlay the base pack, when one is known. */
  checkout?: string | null;
}): Promise<CodingSessionPackStatusResult> {
  const packSource = await fetchCodingSessionSeatPackSource(input.projectRef);
  return decodeCodingSessionPackStatusResult(
    await invokeTauri(CODING_SESSION_PACK_STATUS_COMMAND, {
      agentPubkey: input.agentPubkey,
      role: input.role,
      packSource,
      checkout: input.checkout ?? null,
      // The preview answers for a new seat in this project, so it carries the
      // same requirement staging will: a non-project agent previews the
      // refusal it would get, never a pack it will not be seated with.
      requireProjectRef: input.projectRef,
      newSelection: true,
    }),
    input.role,
  );
}
