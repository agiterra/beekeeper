import * as React from "react";

import { relayClient } from "@/shared/api/relayClient";
import { KIND_REPO_ANNOUNCEMENT } from "@/shared/constants/kinds";
import type { CodingSessionMissionLandEvidenceInput } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  codingSessionMissionLandModel,
  invokeCodingSessionLand,
  type CodingSessionMissionLandModel,
} from "@/features/coding-sessions/lib/codingSessionMissionLand";

/** The repository a mission would land on, once its announcement is read. */
export type CodingSessionMissionLandRepository = {
  /** Hex pubkey of the kind:30617 announcement's author. */
  ownerPubkey: string;
  /** That event's whole tag list, `buzz-protect` rows included. */
  protectionTags: readonly (readonly string[])[];
};

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
  return {
    ownerPubkey: newest.pubkey.toLowerCase(),
    protectionTags: newest.tags.map((tag) => [...tag]),
  };
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
  /** The `repoRef` this session's creates named, or null when none did. */
  repoRef: string | null;
  resolveWho: (pubkey: string) => string;
  /** Injected so a test drives the real model without the Tauri boundary. */
  invoke?: typeof invokeCodingSessionLand;
  /** Injected so a test drives the real read without a relay. */
  readRepository?: typeof readCodingSessionRepository;
}): CodingSessionMissionLandModel | null {
  const {
    founderPubkey,
    genesisRef,
    landEvidence,
    repoRef,
    sessionRef,
    viewerPubkey,
  } = input;
  const invoke = input.invoke ?? invokeCodingSessionLand;
  const readRepository = input.readRepository ?? readCodingSessionRepository;
  const [land, setLand] = React.useState<CodingSessionMissionLandModel | null>(
    null,
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
          landEvidence.includedEventIds.join(","),
        ].join(" ");
  const resolveRef = React.useRef(input.resolveWho);
  resolveRef.current = input.resolveWho;
  const request = React.useRef({
    founderPubkey,
    genesisRef,
    landEvidence,
    repoRef,
    sessionRef,
    viewerPubkey,
  });
  request.current = {
    founderPubkey,
    genesisRef,
    landEvidence,
    repoRef,
    sessionRef,
    viewerPubkey,
  };

  React.useEffect(() => {
    if (identity === null) {
      setLand(null);
      return;
    }
    let cancelled = false;
    const current = request.current;
    void (async () => {
      // A read that throws is "not read", never "no rule": the rule is asked
      // with no repository and the sentence says the announcement did not
      // reach this view.
      let repository: CodingSessionMissionLandRepository | null = null;
      if (current.repoRef !== null) {
        try {
          repository = await readRepository(current.repoRef);
        } catch {
          repository = null;
        }
      }
      if (cancelled) return;
      try {
        const result = await invoke({
          sessionRef: current.sessionRef as string,
          genesisRef: current.genesisRef as string,
          founderPubkey: current.founderPubkey as string,
          repoOwnerPubkey: repository?.ownerPubkey ?? null,
          pusherPubkey: current.viewerPubkey as string,
          protectionTags: repository?.protectionTags ?? null,
          includedEventIds: current.landEvidence?.includedEventIds ?? [],
          events: current.landEvidence?.wireEvents ?? [],
        });
        if (cancelled) return;
        setLand(
          codingSessionMissionLandModel({
            result,
            repositoryUnknownReason:
              current.repoRef === null ? "no-repo-ref" : "not-read",
            resolveWho: (pubkey) => resolveRef.current(pubkey),
          }),
        );
      } catch {
        // A boundary that failed answers nothing rather than guessing: the
        // control is absent, not "refused" and not "ready".
        if (!cancelled) setLand(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [identity, invoke, readRepository]);

  return land;
}
