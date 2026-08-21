/**
 * Project Pulse's fact-only TypeScript fold. Shared vectors are the source of
 * truth and must remain byte-identical to buzz-core.
 *
 * What is left here is Pulse's own half — entries, supersession, and the
 * digest envelope. The session half moved to
 * `shared/coordination/sessionCoordinationFold.ts`, which Agent Progress reads
 * too, so "provider-reachable" is decided once for the whole app instead of
 * once per surface.
 *
 * Import constraint (load-bearing): every module reachable from here must use
 * relative `.ts` specifiers and erasable TypeScript only, because the
 * conformance binder imports this file under plain `node --test` with native
 * type stripping. The shared coordination modules obey the same rule.
 */

import {
  PULSE_ENTRY_KIND,
  normalizePulseProjectCoordinate,
  pulseEntryProjectCoordinate,
  pulseEntrySessionRef,
  validatePulseEntryEnvelope,
  type PulseEvent,
} from "./pulseEntry.ts";
import {
  PULSE_DIGEST_SCHEMA,
  PULSE_SESSIONS_SCOPE,
  type ProjectPulseDigest,
  type ProjectPulseFoldInput,
  type PulseDigestEntry,
  type PulseDigestError,
  type PulseSupersessionClaim,
} from "./pulseFoldTypes.ts";
import {
  foldSessionCoordination,
  sessionCommitConfirmation,
} from "../../../shared/coordination/sessionCoordinationFold.ts";

export * from "./pulseFoldTypes.ts";

/** The fixed tri-state string for one session's commit confirmation. */
export const pulseCommitConfirmation = sessionCommitConfirmation;

function tagValue(event: PulseEvent, key: string): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === key);
  return tag?.[1] ?? null;
}

/** Byte-wise order matching Rust `Ord`; never substitute ICU collation. */
function byteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/**
 * Fold every readable Pulse entry and coding-session fact for one project into
 * the digest envelope.
 *
 * Supersession is a **single-pass marking, never a traversal**, so cycles are
 * structurally impossible: an honored claim is recorded on its target, and a
 * refused claim (cross-author, unresolved, or out-of-order) is recorded on the
 * claimant so the attempt stays visible. Superseded entries are never dropped.
 */
export function foldProjectPulseDigest(
  input: ProjectPulseFoldInput,
): ProjectPulseDigest {
  const project =
    normalizePulseProjectCoordinate(input.project) ?? input.project;
  const sourceErrors = input.sourceErrors ?? [];
  // Session coordination — including the source-read completeness envelope —
  // is folded once for every consumer. Pulse adds entry-local observations to
  // the returned errors without letting those observations make a read partial.
  const foldedSessions = foldSessionCoordination({
    now: input.now,
    events: input.events,
    sourceErrors,
    acceptProjectRef: (projectRef) =>
      projectRef !== null &&
      normalizePulseProjectCoordinate(projectRef) === project,
  });
  const errors: PulseDigestError[] = [...foldedSessions.errors];

  const entries: PulseDigestEntry[] = [];
  const claimsById = new Map<string, PulseSupersessionClaim[]>();
  for (const event of input.events) {
    if (event.kind !== PULSE_ENTRY_KIND) continue;
    // Project scope is decided BEFORE envelope validation, matching the Rust
    // fold's order (`fold_pulse_digest`). A mixed result set is a caller's
    // shape, not a defect: another project's entry — well-formed or not — is
    // not this digest's business and must not appear in its `errors[]`. An
    // unresolvable coordinate *is* this digest's business, because the gate
    // closes rather than opening.
    const coordinate = pulseEntryProjectCoordinate(event);
    if (coordinate === null) {
      errors.push({
        scope: "invalid-entry",
        message: `entry ${event.id} failed validation and was excluded`,
      });
      continue;
    }
    if (coordinate !== project) continue;
    const decoded = validatePulseEntryEnvelope(event);
    if (!decoded.ok) {
      // Ingest rejects these shapes, so only a smuggled or legacy event gets
      // here. It is excluded from the claims but never silently dropped.
      errors.push({
        scope: "invalid-entry",
        message: `entry ${event.id} failed validation and was excluded`,
      });
      continue;
    }
    entries.push({
      eventId: event.id,
      pubkey: event.pubkey,
      createdAt: event.created_at,
      type: decoded.entry.type,
      text: decoded.entry.text,
      claimedAreas: [...decoded.entry.codeAreas],
      // The content field is the claim; the `branch` tag exists so a relay
      // filter can see it, and the validator already proved the two agree
      // whenever both are present. Falling back to the tag keeps this fold
      // byte-identical to the Rust one (`crates/buzz-cli/src/commands/pulse.rs`
      // `branch: entry.branch.or_else(tag "branch")`) for an entry that
      // carries only the tag.
      branch: decoded.entry.branch ?? tagValue(event, "branch"),
      sessionRef: pulseEntrySessionRef(event),
      supersedes: decoded.entry.supersedes,
      supersededBy: [],
      active: true,
    });
    claimsById.set(event.id, []);
  }

  const byId = new Map(entries.map((entry) => [entry.eventId, entry]));
  for (const claimant of entries) {
    const targetId = claimant.supersedes;
    if (targetId === null) continue;
    const target = byId.get(targetId);
    if (!target) {
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: null,
        honored: false,
        reason: "unresolved",
      });
      errors.push({
        scope: "unresolved-supersedes",
        message: `entry ${claimant.eventId} supersedes ${targetId}, which is not in the visible result set`,
      });
      continue;
    }
    if (target.pubkey !== claimant.pubkey) {
      // A peer's claim never removes your entry from the set that drives
      // wait|consult|proceed — both entries render, the claim stays visible.
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: target.pubkey,
        honored: false,
        reason: "cross-author",
      });
      continue;
    }
    const wins =
      claimant.createdAt > target.createdAt ||
      (claimant.createdAt === target.createdAt &&
        claimant.eventId > target.eventId);
    if (!wins) {
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: target.pubkey,
        honored: false,
        reason: "out-of-order",
      });
      continue;
    }
    claimsById.get(target.eventId)?.push({
      eventId: claimant.eventId,
      pubkey: claimant.pubkey,
      honored: true,
      reason: null,
    });
    target.active = false;
  }

  for (const entry of entries) {
    entry.supersededBy = (claimsById.get(entry.eventId) ?? []).sort(
      (left, right) => byteOrder(left.eventId, right.eventId),
    );
  }
  entries.sort((left, right) =>
    left.createdAt !== right.createdAt
      ? right.createdAt - left.createdAt
      : byteOrder(right.eventId, left.eventId),
  );

  errors.sort((left, right) =>
    left.scope !== right.scope
      ? byteOrder(left.scope, right.scope)
      : byteOrder(left.message, right.message),
  );

  return {
    schema: PULSE_DIGEST_SCHEMA,
    source: input.source ?? "client-composed",
    project,
    asOf: input.now,
    // `complete:false` plus a non-empty `errors[]` is the only representation
    // of a partial read: an invalid entry or a dangling supersession is a fact
    // about one event, not a failed query, and never flips this.
    complete: foldedSessions.complete,
    sessionsScope: PULSE_SESSIONS_SCOPE,
    sessions: foldedSessions.sessions,
    providerReachableSessions: foldedSessions.providerReachableSessions,
    openUnverifiedSessions: foldedSessions.openUnverifiedSessions,
    closedSessions: foldedSessions.closedSessions,
    entries,
    errors,
  };
}

/** The distinct branch values across a digest, `null` kept as its own group. */
export function pulseDigestBranches(
  digest: ProjectPulseDigest,
): Array<string | null> {
  const named = new Set<string>();
  let hasNull = false;
  for (const branch of [
    ...digest.entries.map((entry) => entry.branch),
    ...digest.sessions.flatMap((session) =>
      session.generations.map((generation) => generation.branch),
    ),
  ]) {
    if (branch === null) hasNull = true;
    else named.add(branch);
  }
  const groups: Array<string | null> = [...named].sort(byteOrder);
  // A null branch is its own group: it is never merged into a named branch and
  // never rendered as one. Desktop's "no branch" chip maps to `--branch -`.
  if (hasNull) groups.push(null);
  return groups;
}
