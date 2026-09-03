import * as React from "react";

import { relayClient } from "@/shared/api/relayClient";
import {
  KIND_PROJECT,
  KIND_PROJECT_MEMBERS,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";
import type { CodingSessionMissionLandEvidenceInput } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  codingSessionMissionLandModel,
  invokeCodingSessionLand,
  type CodingSessionMissionLandModel,
  type CodingSessionMissionLandUnavailableReason,
} from "@/features/coding-sessions/lib/codingSessionMissionLand";
import { inferCodingSessionMissionLandRepo } from "@/features/coding-sessions/lib/codingSessionMissionLandRepoInference";

/** The repository a mission would land on, once its announcement is read. */
export type CodingSessionMissionLandRepository = {
  /** Hex pubkey of the kind:30617 announcement's author. */
  ownerPubkey: string;
  /** That event's whole tag list, `buzz-protect` rows included. */
  protectionTags: readonly (readonly string[])[];
  /**
   * Pubkeys the repository's project roster grants Owner, or null when the
   * roster could not be read (no `project` back-reference is *not* that case —
   * a repository in no project has no roster, and nothing is missing).
   *
   * Finding 33: a project Owner founds the repository under `a56ad5d01`'s
   * model, and the pre-L18 Land control read only the announcement's signer.
   */
  projectOwnerPubkeys: readonly string[] | null;
};

/** A NIP-MP project coordinate, `30621:<owner>:<d>`. */
const PROJECT_ADDRESS = /^30621:([0-9a-f]{64}):(.+)$/i;

/**
 * The Owners of the project at `coordinate`, from the wire.
 *
 * The relay-signed kind:39010 projection when one exists, and otherwise the
 * project head's own `p` tags — which is what the relay's ACL was built from
 * before any membership op was accepted. The creator holds no roster row (a
 * membership op naming them is refused outright) and is an Owner implicitly,
 * so they are always included.
 *
 * Throws rather than returning `[]` on a read it could not make: an empty
 * roster and an unread one are different facts and the sentence says which.
 */
export async function readProjectOwners(
  coordinate: string,
  fetchEvents: typeof relayClient.fetchEvents = (filter) =>
    relayClient.fetchEvents(filter),
): Promise<readonly string[]> {
  const match = PROJECT_ADDRESS.exec(coordinate.trim());
  if (match === null) return [];
  const creator = match[1].toLowerCase();
  const identifier = match[2];
  const projections = await fetchEvents({
    kinds: [KIND_PROJECT_MEMBERS],
    "#d": [coordinate],
    limit: 1,
  });
  let source = [...projections].sort((a, b) => b.created_at - a.created_at)[0];
  if (!source) {
    const heads = await fetchEvents({
      kinds: [KIND_PROJECT],
      authors: [creator],
      "#d": [identifier],
      limit: 1,
    });
    source = [...heads].sort((a, b) => b.created_at - a.created_at)[0];
  }
  const owners = [creator];
  for (const tag of source?.tags ?? []) {
    // `["p", <hex>, <relay hint>, <role>]` — a role-less invite is the legacy
    // collaborator, which is not an Owner.
    if (tag[0] !== "p" || tag[3] !== "owner") continue;
    const pubkey = (tag[1] ?? "").toLowerCase();
    if (pubkey.length === 64 && !owners.includes(pubkey)) owners.push(pubkey);
  }
  return owners;
}

/** A NIP-34 repository address, `30617:<owner>:<d>`. */
const REPO_ADDRESS = /^30617:([0-9a-f]{64}):(.+)$/i;

/**
 * Split a create's `repoRef` into the coordinates its announcement is at.
 *
 * Exported for its own test: the address is the only thing standing between
 * "this session names a repository" and "we did not look", and a parser that
 * quietly returned null for a valid address would make the second sentence a
 * lie about the first.
 */
export function codingSessionRepoAddress(
  repoRef: string | null,
): { ownerPubkey: string; identifier: string } | null {
  const match = repoRef === null ? null : REPO_ADDRESS.exec(repoRef.trim());
  return match === null
    ? null
    : { ownerPubkey: match[1].toLowerCase(), identifier: match[2] };
}

/** Read one repository announcement, or null when the relay holds none. */
export async function readCodingSessionRepository(
  repoRef: string,
  fetchEvents: typeof relayClient.fetchEvents = (filter) =>
    relayClient.fetchEvents(filter),
): Promise<CodingSessionMissionLandRepository | null> {
  const address = codingSessionRepoAddress(repoRef);
  if (address === null) return null;
  const events = await fetchEvents({
    kinds: [KIND_REPO_ANNOUNCEMENT],
    authors: [address.ownerPubkey],
    "#d": [address.identifier],
    limit: 1,
  });
  // Newest wins: kind:30617 is addressable, so the relay may hand back an
  // older copy alongside the live one.
  const newest = [...events].sort((a, b) => b.created_at - a.created_at)[0];
  if (!newest) return null;
  const coordinate = newest.tags.find((tag) => tag[0] === "project")?.[1];
  let projectOwnerPubkeys: readonly string[] | null = [];
  if (coordinate !== undefined) {
    try {
      projectOwnerPubkeys = await readProjectOwners(coordinate, fetchEvents);
    } catch {
      // Unread, and disclosed as unread. Guessing `[]` here would print a
      // founder set that silently omits a co-owner — finding 33's own shape.
      projectOwnerPubkeys = null;
    }
  }
  return {
    ownerPubkey: newest.pubkey.toLowerCase(),
    protectionTags: newest.tags.map((tag) => [...tag]),
    projectOwnerPubkeys,
  };
}

/**
 * The repository addresses (`30617:<owner>:<d>`, deduplicated) a project's
 * own repo announcements name — the read half of LANE-L20 item 2.
 *
 * Every repository that back-references the project through its own
 * `project` tag, the same tag `readCodingSessionRepository` reads to find a
 * repository's project. Newest-per-address only: kind:30617 is addressable,
 * so a stale copy alongside the live one must not double-count.
 */
export async function readProjectRepositoryAddresses(
  projectRef: string,
  fetchEvents: typeof relayClient.fetchEvents = (filter) =>
    relayClient.fetchEvents(filter),
): Promise<readonly string[]> {
  const events = await fetchEvents({
    kinds: [KIND_REPO_ANNOUNCEMENT],
    "#project": [projectRef],
    // A generous cap, not a real pagination boundary: the inference only
    // needs to tell "exactly one" from "more than one", and no project this
    // surface reads is expected to announce anywhere near this many repos.
    limit: 200,
  });
  const newestByAddress = new Map<string, number>();
  for (const event of events) {
    const dtag = event.tags.find((tag) => tag[0] === "d")?.[1];
    if (!dtag) continue;
    const address = `${KIND_REPO_ANNOUNCEMENT}:${event.pubkey.toLowerCase()}:${dtag}`;
    const seenAt = newestByAddress.get(address);
    if (seenAt === undefined || event.created_at > seenAt) {
      newestByAddress.set(address, event.created_at);
    }
  }
  return [...newestByAddress.keys()];
}

/**
 * Ask the push path's own rule whether this mission's commit may land.
 *
 * Two one-shot reads keyed on identity, never a poll and never a timer (I1):
 * the repository's kind:30617 (when the session's creates name one), then the
 * native rule over the fold this surface already holds. It re-runs when the
 * included set changes, because a new disposition is exactly the event that
 * turns "not ready" into "ready".
 *
 * With **no** repository record the rule still runs and answers
 * `repositoryKnown: false`, and the model says *which* fact that is — the
 * session names no repository, or the one it names was not read here. A
 * surface that hid the control there would leave a founder unable to tell
 * either from "nothing gates this push".
 */
export function useCodingSessionMissionLand(input: {
  sessionRef: string | null;
  genesisRef: string | null;
  founderPubkey: string | null;
  viewerPubkey: string | null;
  landEvidence: CodingSessionMissionLandEvidenceInput | undefined;
  /**
   * The mission's folded kind 44246 gate rows — arm (B)'s only evidence.
   *
   * Empty is "this view read no observations", never "the gates were red";
   * the rule then simply has no arm-(B) answer to give.
   */
  observedGates: readonly {
    authorPubkey: string;
    source: string;
    gate: string;
    outcome: string;
    headSha: string | null;
    dirty: boolean | null;
  }[];
  /** The gate half of the newest founder-signed policy, or null when unread. */
  gatePolicy: {
    verifierRequired: boolean | null;
    requiredGates: readonly string[] | null;
  } | null;
  /** The `repoRef` this session's creates named, or null when none did. */
  repoRef: string | null;
  /**
   * The `projectRef` this session's creates named, or null when none did.
   *
   * LANE-L20 item 2: read only when `repoRef` is null, to infer a repository
   * from the project's own repositories rather than leave every session a
   * pre-finding-38 create ever signed permanently unable to land.
   */
  projectRef: string | null;
  resolveWho: (pubkey: string) => string;
  /** Injected so a test drives the real model without the Tauri boundary. */
  invoke?: typeof invokeCodingSessionLand;
  /** Injected so a test drives the real read without a relay. */
  readRepository?: typeof readCodingSessionRepository;
  /** Injected so a test drives the real read without a relay. */
  readProjectRepos?: typeof readProjectRepositoryAddresses;
}): {
  land: CodingSessionMissionLandModel | null;
  /**
   * Why `land` is null, when it is — finding 37. An absent control used to
   * say nothing, leaving a founder unable to tell "nothing to land yet" from
   * "the read failed"; this is the fact the panel needs to say which.
   */
  unavailableReason: CodingSessionMissionLandUnavailableReason | null;
} {
  const {
    founderPubkey,
    gatePolicy,
    genesisRef,
    landEvidence,
    observedGates,
    projectRef,
    repoRef,
    sessionRef,
    viewerPubkey,
  } = input;
  const invoke = input.invoke ?? invokeCodingSessionLand;
  const readRepository = input.readRepository ?? readCodingSessionRepository;
  const readProjectRepos =
    input.readProjectRepos ?? readProjectRepositoryAddresses;
  const [land, setLand] = React.useState<CodingSessionMissionLandModel | null>(
    null,
  );
  const [unavailableReason, setUnavailableReason] =
    React.useState<CodingSessionMissionLandUnavailableReason | null>(
      "no-identity",
    );
  // The identity of the *question*, not of the objects: the fold hands fresh
  // arrays on every projection, and an effect keyed on those would re-ask the
  // rule on every render.
  const identity =
    sessionRef === null ||
    genesisRef === null ||
    founderPubkey === null ||
    viewerPubkey === null ||
    landEvidence === undefined
      ? null
      : [
          sessionRef,
          genesisRef,
          founderPubkey,
          viewerPubkey,
          repoRef ?? "no-repo-ref",
          projectRef ?? "no-project-ref",
          landEvidence.includedEventIds.join(","),
          // Arm (B)'s own inputs are part of the *question*: a gate row that
          // arrives after the first ask changes the answer, and an identity
          // that ignored them would leave the panel showing a refusal the
          // rule no longer gives. Content, not object identity — the folds
          // hand fresh arrays every render.
          observedGates
            .map(
              (row) =>
                `${row.source}:${row.gate}:${row.outcome}:${row.headSha ?? "-"}:${row.dirty ?? "-"}`,
            )
            .join(","),
          gatePolicy === null
            ? "no-policy"
            : `${gatePolicy.verifierRequired ?? "unset"}:${(gatePolicy.requiredGates ?? []).join("|")}`,
        ].join(" ");
  const resolveRef = React.useRef(input.resolveWho);
  resolveRef.current = input.resolveWho;
  const request = React.useRef({
    founderPubkey,
    gatePolicy,
    genesisRef,
    landEvidence,
    observedGates,
    projectRef,
    repoRef,
    sessionRef,
    viewerPubkey,
  });
  request.current = {
    founderPubkey,
    gatePolicy,
    genesisRef,
    landEvidence,
    observedGates,
    projectRef,
    repoRef,
    sessionRef,
    viewerPubkey,
  };

  React.useEffect(() => {
    if (identity === null) {
      setLand(null);
      setUnavailableReason("no-identity");
      return;
    }
    let cancelled = false;
    const current = request.current;
    void (async () => {
      // A read that throws is "not read", never "no rule": the rule is asked
      // with no repository and the sentence says the announcement did not
      // reach this view.
      let repository: CodingSessionMissionLandRepository | null = null;
      let unknownReason: "no-repo-ref" | "not-read" | "multiple-repos" =
        current.repoRef === null ? "no-repo-ref" : "not-read";
      let projectRepoCount: number | undefined;
      let repositoryInferred = false;
      if (current.repoRef !== null) {
        try {
          repository = await readRepository(current.repoRef);
        } catch {
          repository = null;
        }
      } else if (current.projectRef !== null) {
        // Item 2's read fallback: the session's own creates named nothing,
        // but its project might name exactly one repository.
        try {
          const addresses = await readProjectRepos(current.projectRef);
          const inference = inferCodingSessionMissionLandRepo(addresses);
          if (inference.kind === "inferred") {
            repositoryInferred = true;
            try {
              repository = await readRepository(inference.repoRef);
              if (repository === null) unknownReason = "not-read";
            } catch {
              repository = null;
              unknownReason = "not-read";
            }
          } else if (inference.kind === "multiple") {
            unknownReason = "multiple-repos";
            projectRepoCount = inference.count;
          }
        } catch {
          // Unread project repositories reads the same as naming none: there
          // was nothing here to infer from, so the plain "no repository"
          // sentence stands rather than a second, different "not read".
        }
      }
      if (cancelled) return;
      try {
        const result = await invoke({
          sessionRef: current.sessionRef as string,
          genesisRef: current.genesisRef as string,
          founderPubkey: current.founderPubkey as string,
          repoOwnerPubkey: repository?.ownerPubkey ?? null,
          projectOwnerPubkeys: repository?.projectOwnerPubkeys ?? null,
          pusherPubkey: current.viewerPubkey as string,
          protectionTags: repository?.protectionTags ?? null,
          includedEventIds: current.landEvidence?.includedEventIds ?? [],
          activeSeats: current.landEvidence?.activeSeats ?? [],
          observedGates: current.observedGates,
          gatePolicy: current.gatePolicy,
          events: current.landEvidence?.wireEvents ?? [],
        });
        if (cancelled) return;
        setLand(
          codingSessionMissionLandModel({
            result,
            repositoryUnknownReason: unknownReason,
            projectRepoCount,
            repositoryInferred: repositoryInferred && repository !== null,
            resolveWho: (pubkey) => resolveRef.current(pubkey),
          }),
        );
        setUnavailableReason(null);
      } catch {
        // A boundary that failed answers nothing rather than guessing: the
        // control is absent, not "refused" and not "ready" — and finding 37's
        // reason says so rather than leaving the panel to guess.
        if (!cancelled) {
          setLand(null);
          setUnavailableReason("boundary-failed");
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [identity, invoke, readRepository, readProjectRepos]);

  return { land, unavailableReason };
}
