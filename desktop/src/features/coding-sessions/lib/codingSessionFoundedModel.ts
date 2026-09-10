/**
 * Founded umbrellas: a genesis with nothing running under it.
 *
 * The desktop founds every session with a 44226 before it creates anything
 * (`useNewCodingSessionCreate.ts`, `codingSessionCrewLaunch.ts`), and a team
 * session is now founded *first* — goal, name, genesis — with who leads, the
 * runtime, the bench and the policy decided afterwards, inside the session.
 * Between those two moments the umbrella has a genesis, a goal and a name and
 * no execution, and nothing in the catalog could show it: `groupCodingSessionCatalog`
 * folds provider-reported generations, so an umbrella with none was not a row.
 *
 * This is the projection for that gap. It is deliberately **not** an umbrella
 * record with an invented status and an empty execution list — nine modules
 * consume `executions` as non-empty — but its own small record, derived from
 * the geneses the create-observation store already keeps, minus those any
 * receipt-joined create names.
 */
import { umbrellaGenerations } from "./channelCodingSessionIngress";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionGenesisObservation,
} from "./codingSessionTypes";
import {
  type CodingSessionUmbrellaCreateObservation,
  groupCodingSessionCatalog,
} from "./codingSessionUmbrellaModel";

/** A founded umbrella with no execution: what the founded route renders. */
export type CodingSessionFoundedUmbrella = {
  channelId: string;
  sessionRef: string;
  genesisRef: string;
  /** The genesis signer — the one identity the umbrella has until it starts. */
  founderPubkey: string;
  /** Genesis `created_at`, in seconds. */
  foundedAt: number;
};

/**
 * The founded umbrellas of one channel, newest founding first.
 *
 * A genesis stays founded iff no receipt-joined create claims its
 * `sessionRef` (the primary signal — a receipt lands before metadata) **and**
 * no catalog entry echoes it (the backstop for a create that fell outside the
 * observation history window while its 44223 is still held).
 */
export function resolveFoundedCodingSessions(input: {
  channelId: string;
  geneses: readonly CodingSessionGenesisObservation[];
  entries: readonly CodingSessionCatalogRecord[];
  creates: readonly CodingSessionUmbrellaCreateObservation[];
}): CodingSessionFoundedUmbrella[] {
  const claimed = new Set<string>();
  for (const create of input.creates) {
    if (create.channelId === input.channelId && create.sessionRef) {
      claimed.add(create.sessionRef);
    }
  }
  for (const entry of input.entries) {
    if (entry.sessionRef) claimed.add(entry.sessionRef);
  }
  return input.geneses
    .filter(
      (genesis) =>
        genesis.channelId === input.channelId &&
        !claimed.has(genesis.sessionRef),
    )
    .map((genesis) => ({
      channelId: genesis.channelId,
      sessionRef: genesis.sessionRef,
      genesisRef: genesis.genesisRef,
      founderPubkey: genesis.founderPubkey,
      foundedAt: genesis.foundedAt,
    }))
    .sort(
      (left, right) =>
        right.foundedAt - left.foundedAt ||
        left.genesisRef.localeCompare(right.genesisRef),
    );
}

export type CodingSessionFoundedRouteResolution =
  | { kind: "loading" }
  /** A genesis and nothing else: the Team card's state. */
  | { kind: "founded"; founded: CodingSessionFoundedUmbrella }
  /**
   * A receipt-joined create claims the ref but no generation exists yet —
   * the receipt→metadata gap, and what a second viewer sees while somebody
   * else's Start is in flight.
   */
  | { kind: "starting"; founded: CodingSessionFoundedUmbrella }
  /** The umbrella has a generation; the founded route hands off to it. */
  | { kind: "started"; generationId: string }
  | { kind: "missing"; description: string };

/**
 * What the founded route should show for one `sessionRef`.
 *
 * `started` wins over everything: once a generation exists the founded screen
 * has nothing to add and replaces itself. `loading` is answered only after the
 * positive states, and reads `foundingIsLoading` as well as `isLoading`,
 * because the catalog deliberately does not wait on the observation read and
 * a cold start would otherwise say "missing" for a session that exists.
 */
export function resolveFoundedCodingSessionRoute(input: {
  catalog: CodingSessionCatalogSnapshot;
  channelId: string;
  sessionRef: string;
}): CodingSessionFoundedRouteResolution {
  const creates = input.catalog.creates ?? [];
  for (const umbrella of groupCodingSessionCatalog(
    input.catalog.entries,
    creates,
  )) {
    if (umbrella.sessionRef !== input.sessionRef) continue;
    const newest = umbrellaGenerations(umbrella)[0];
    if (newest) return { kind: "started", generationId: newest.generationId };
  }
  if (input.catalog.authorityErrorMessage) {
    return {
      kind: "missing",
      description: input.catalog.authorityErrorMessage,
    };
  }
  const geneses = (input.catalog.geneses ?? []).filter(
    (genesis) =>
      genesis.channelId === input.channelId &&
      genesis.sessionRef === input.sessionRef,
  );
  const genesis = geneses[0];
  if (genesis) {
    const founded: CodingSessionFoundedUmbrella = {
      channelId: genesis.channelId,
      sessionRef: genesis.sessionRef,
      genesisRef: genesis.genesisRef,
      founderPubkey: genesis.founderPubkey,
      foundedAt: genesis.foundedAt,
    };
    const claimed = creates.some(
      (create) =>
        create.channelId === input.channelId &&
        create.sessionRef === input.sessionRef,
    );
    return claimed
      ? { kind: "starting", founded }
      : { kind: "founded", founded };
  }
  if (input.catalog.isLoading || input.catalog.foundingIsLoading) {
    return { kind: "loading" };
  }
  return {
    kind: "missing",
    description:
      input.catalog.errorMessage ??
      "No genesis for this session is available in the relay catalog.",
  };
}

/**
 * Founder by genesis id for every umbrella the catalog knows — started ones
 * through their receipt-joined creates, founded ones through the genesis
 * itself. The closure fold drops any 44230 whose genesis is not in this map,
 * so a founded session could not be closed without the second source.
 *
 * Both sources name the genesis signer, so they can only agree; the create
 * join is consulted first because it is the resolution every started surface
 * already uses, and a genesis the store holds but no create names fills in
 * the founded remainder. `geneses` may span several channels — a genesis id
 * is unique across them — which is what the global shelf relies on.
 */
export function founderPubkeysByGenesisRef(
  catalog: Pick<
    CodingSessionCatalogSnapshot,
    "entries" | "creates" | "geneses"
  >,
): Map<string, string> {
  const founders = new Map<string, string>();
  for (const umbrella of groupCodingSessionCatalog(
    catalog.entries,
    catalog.creates ?? [],
  )) {
    if (umbrella.genesisRef && umbrella.founderPubkey) {
      founders.set(umbrella.genesisRef, umbrella.founderPubkey);
    }
  }
  for (const genesis of catalog.geneses ?? []) {
    if (!founders.has(genesis.genesisRef)) {
      founders.set(genesis.genesisRef, genesis.founderPubkey);
    }
  }
  return founders;
}
