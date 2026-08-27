import type { CodingSessionTurnBudget } from "./codingSessionIngressPayloads";
import type { CodingSessionProjectedTranscriptItem } from "./codingSessionTranscriptItems";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import type { CodingSessionLifecycleResolution } from "./codingSessionTrustedIngress";
import type { CodingSessionUmbrellaCreateObservation } from "./codingSessionUmbrellaModel";

export type CodingSessionStatus =
  | "starting"
  | "idle"
  | "running"
  | "waiting_for_input"
  | "completed"
  | "stopped"
  | "failed"
  | "interrupted"
  | "disconnected"
  | "unknown";

export type CodingSessionCapabilities = {
  threadTurnStart: boolean;
  threadTurnInterrupt: boolean;
  threadSteer: boolean;
  context: boolean;
  diff: boolean;
  plan: boolean;
};

/** Provider-neutral catalog record consumed by coding-session surfaces. */
export type CodingSessionCatalogRecord = {
  generationId: string;
  label: string;
  title: string;
  /** Exact trusted provider authority derived from this generation's transcripts. */
  providerAuthorityPubkey: string | null;
  /** Exact authority whose metadata enriched this record, or null when unenriched. */
  metadataAuthorityPubkey: string | null;
  lastEventAt: string;
  status: CodingSessionStatus;
  /**
   * When `status` was observed — the newest 44223 metadata event's
   * `created_at` in ms — or null when no metadata has arrived. Distinct from
   * `lastEventAt`, which is a max over metadata AND transcript streams and
   * says nothing about which stream is fresher.
   */
  statusAt: number | null;
  transcript: CodingSessionProjectedTranscriptItem[];
  conflictCount: number;
  commandTarget: CodingSessionCommandTarget | null;
  projectRef: string | null;
  repoRef: string | null;
  /**
   * Umbrella session reference echoed by the provider's 44223 metadata, or
   * null for a pre-umbrella session (an implicit umbrella of one).
   */
  sessionRef: string | null;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  /**
   * The agent seated on this execution — the `agent_ref` its provider signed
   * into 44223 — or null when a person created it and no agent identity was
   * injected.
   */
  agentRef: string | null;
  /** The seat's role slug. Non-null exactly when `agentRef` is. */
  role: string | null;
  /**
   * The crew turn allowance this execution's umbrella is running under (D9),
   * or null when the provider published none — no umbrella claimed, or a host
   * that set no budget. Never invented locally: absent means "not disclosed",
   * not "unlimited".
   */
  turnBudget: CodingSessionTurnBudget | null;
  capabilities: CodingSessionCapabilities | null;
};

/**
 * One provider runtime session across its generations: the `cs-target` minus
 * `generation`, per signer. The active generation is the highest one with
 * events; prior generations stay reachable as collapsed history.
 */
export type CodingSessionExecution = {
  executionKey: string;
  /** The fact-stream signer (provider authority) behind this execution. */
  signerPubkey: string;
  activeGeneration: CodingSessionCatalogRecord;
  /** Earlier generations, ascending. */
  priorGenerations: CodingSessionCatalogRecord[];
  /**
   * The human who signed this execution's 44221 create, when a create
   * observation resolved it; null when unknown.
   */
  operatorPubkey: string | null;
};

/**
 * All executions sharing a `sessionRef`, plus (for umbrella-claiming
 * sessions) the conversation lane. Records with no sessionRef form an
 * implicit umbrella of one.
 */
export type CodingSessionUmbrellaRecord = {
  /** `sessionRef`, or `implicit:<executionKey>` for an umbrella of one. */
  umbrellaKey: string;
  sessionRef: string | null;
  title: string;
  executions: CodingSessionExecution[];
  /** Genesis signer when linked, else the legacy founder projection. */
  founderPubkey: string | null;
  /** Exact genesis event id reached through a receipt-joined create. */
  genesisRef: string | null;
  /**
   * How the umbrella's authority anchor resolved. `genesisRef` alone cannot
   * distinguish "this session has no genesis" from "the creates that would
   * name one have not been observed (yet)" — and a join that guesses the
   * former mints an ungoverned execution inside a governed session.
   *
   * - `governed`: receipt-joined creates name exactly one genesis (in
   *   `genesisRef`).
   * - `legacy`: creates are known and none names a genesis, or the umbrella
   *   is implicit (no `sessionRef` — nothing can join it anyway).
   * - `unresolved`: the umbrella claims a `sessionRef` but no receipt-joined
   *   create has been observed — typically the observations have not loaded,
   *   so a genesis cannot be ruled out.
   * - `conflict`: creates name more than one genesis.
   */
  genesisResolution: "governed" | "legacy" | "unresolved" | "conflict";
  /**
   * Derived: `running` if any execution runs, else `waiting_for_input` if any
   * waits, else the most recently active execution's status.
   */
  status: CodingSessionStatus;
  lastEventAt: string;
  conflictCount: number;
  /** Executions whose known operator is not the founder — flagged, never merged. */
  foreignAttachmentCount: number;
};

export type CodingSessionCatalogSnapshot = {
  channelId: string | null;
  entries: CodingSessionCatalogRecord[];
  /**
   * Receipt-joined 44221 create observations for this channel, when the
   * consumer collected them. Optional: a caller that has none (a test fixture,
   * or a surface that never resolves authority) leaves founder and operator
   * unresolved, which is the permissive fallback rather than a gate.
   */
  creates?: readonly CodingSessionUmbrellaCreateObservation[];
  isLoading: boolean;
  errorMessage: string | null;
  authorityErrorMessage: string | null;
  rejectedAuthorCount: number;
  invalidSignatureCount: number;
};

export type GlobalCodingSessionCatalogRecord = {
  channelId: string;
  session: CodingSessionCatalogRecord;
};

export type GlobalCodingSessionCatalogSnapshot = {
  entries: GlobalCodingSessionCatalogRecord[];
  /** Human-signed creates used only to resolve each umbrella's founder. */
  creates?: readonly CodingSessionUmbrellaCreateObservation[];
  isLoading: boolean;
  errorMessage: string | null;
  authorityErrorMessage: string | null;
  /**
   * Resolve one create command's lifecycle from the catalog's verified
   * receipts, by (channel, commandId, provider authority). Optional: only the
   * live ingress-backed catalog provides it; fixtures and derived snapshots
   * may omit it.
   */
  lifecycleFor?: (
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ) => CodingSessionLifecycleResolution | null;
};

export type CodingSessionWorkspaceStatus =
  | { kind: "working"; label: "Working" }
  | { kind: "idle"; label: "Idle" }
  | { kind: "ended"; label: "Ended" }
  /**
   * Not a known-good state: either nothing legible has been read yet, or the
   * signed lifecycle says this execution is not usable right now.
   *
   * `attention` separates the two. It is absent when the status is merely
   * unread — the historical `Status unknown` — and set when a provider-signed
   * lifecycle status put the execution here, which every surface paints as
   * attention-worthy rather than idle-calm. Keeping one `kind` is deliberate:
   * surfaces already treat this bucket as "cannot vouch for it", and the
   * distinction they need is the label plus this flag, not a new branch each
   * of them would have to learn separately.
   */
  | {
      kind: "unknown";
      label:
        | "Status unknown"
        | "Disconnected"
        | "Needs attention"
        | "No provider answering";
      attention?: "disconnected" | "failed" | "unreachable";
      /**
       * The provider's newest signed report, kept as history when coordination
       * proves nobody is answering for this generation right now.
       *
       * A status is what a provider *said*; reachability is whether anything
       * can still answer. Printing the first as the second is how a header read
       * `Idle` for two hours over an app that had quit (§2 item 41), so when
       * the lease says unreachable the report is demoted to this field and
       * every surface renders it as "last reported X, N ago".
       */
      lastReported?: { label: string; ageSeconds: number | null };
    };
