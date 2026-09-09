/**
 * The writes behind "Continue this session's work" (§5), in one testable place.
 *
 * The flow is four irreversible-ish steps in a fixed order, and the order is
 * the honesty: **claim first**. The takeover is what fences the absent
 * machine, so it must be accepted by the relay before this host fetches
 * anything, creates anything, or tells anybody the work moved. If the claim is
 * refused — somebody else won the race, the chain moved — nothing else runs.
 *
 * 1. publish `takeover` (44228) and wait, bounded, for the relay's own
 *    acceptance to appear as the chain head;
 * 2. `handover_prepare_checkout` — fetch the wip ref, verify the sha, apply
 *    the patch, and report what was and was not recovered;
 * 3. `session.create` on this host's provider, joining the same
 *    `sessionRef`/`genesisRef`, seeded with the rendered checkpoint;
 * 4. publish the 44247 `continuation` naming the claim, the checkpoint, the
 *    new target and the exact `recovered`/`missing` lines step 2 measured.
 *
 * Nothing is retried silently: every failure returns the step it failed at and
 * the reason, and the surface renders it. A step that already succeeded is
 * reported even when a later one fails — a claim that landed is a fact about
 * the session whether or not the reconstruction finished.
 */
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";

import { KIND_GIT_PATCH } from "@/shared/constants/kinds";
import {
  clearCodingSessionCreateHint,
  stageCodingSessionCreateHint,
} from "@/shared/api/tauriCodingSessionWorkdirs";

import {
  awaitCodingSessionCreateSettlement,
  confirmRecoveredCheckout,
  defaultWait,
  isProvenRelayRefusal,
  SETTLE_ATTEMPTS,
  SETTLE_DELAY_MS,
  type CodingSessionHandoverCheckoutReport,
} from "./codingSessionHandoverSettlement";

// Re-exported so the flow stays one import for its callers and its tests: the
// split below is about file size, not about two vocabularies.
export {
  awaitCodingSessionCreated,
  awaitCodingSessionCreateSettlement,
  isProvenRelayRefusal,
  CODING_SESSION_HANDOVER_UNKNOWN_CREATE,
  type CodingSessionHandoverCheckoutReport,
} from "./codingSessionHandoverSettlement";
import {
  buildCodingSessionHandoverContent,
  buildCodingSessionHandoverTags,
  KIND_CODING_SESSION_HANDOVER,
  type CodingSessionCheckpointBody,
  type CodingSessionContinuationBody,
  type CodingSessionHandoverArtifact,
  type CodingSessionHandoverTarget,
} from "./codingSessionHandoverWire";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionCreate,
} from "./codingSessionLifecycleCommand";
import {
  fetchCodingSessionRosterFold,
  publishCodingSessionAuthorityTransition,
} from "./codingSessionRoster";

/** Everything the continue flow needs, so a test can drive all of it. */
export type CodingSessionHandoverPublishDependencies = {
  publishTransition?: typeof publishCodingSessionAuthorityTransition;
  fetchFold?: typeof fetchCodingSessionRosterFold;
  signer?: (input: {
    kind: number;
    content: string;
    tags: string[][];
  }) => Promise<RelayEvent>;
  publisher?: {
    publishEvent(
      event: RelayEvent,
      timeoutMessage: string,
      failureMessage: string,
    ): Promise<RelayEvent>;
  };
  fetchEvents?: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>;
  prepareCheckout?: (
    request: CodingSessionHandoverCheckoutRequest,
  ) => Promise<CodingSessionHandoverCheckoutReport>;
  publishCreate?: typeof publishCodingSessionCreate;
  /** Stage the directory this exact create must run in. */
  stageCreateHint?: (input: {
    commandId: string;
    path: string;
    projectRef?: string | null;
  }) => Promise<unknown>;
  /** Drop a staged hint that no longer has a create to steer. */
  clearCreateHint?: (commandId: string) => Promise<unknown>;
  wait?: (milliseconds: number) => Promise<void>;
  settleAttempts?: number;
};

/** The argument shape of the `handover_prepare_checkout` Tauri command. */
export type CodingSessionHandoverCheckoutRequest = {
  cwd: string;
  /** An explicit remote name; absent means "resolve it from the checkout". */
  repoRemote?: string | null;
  /** The repository coordinate the wip ref lives in. */
  repoRef?: string | null;
  /** This community's relay origin, so the expected clone URL can be built. */
  relayOrigin?: string | null;
  wipRef: string;
  sha: string;
  sessionRef: string;
  patchText?: string | null;
  baseSha?: string | null;
};

/** Sign and publish one 44247 record. */
export async function publishCodingSessionHandoverRecord(
  input: {
    channelId: string;
    sessionRef: string;
    genesisRef: string;
  } & (
    | { type: "checkpoint"; body: CodingSessionCheckpointBody }
    | { type: "continuation"; body: CodingSessionContinuationBody }
  ),
  dependencies: CodingSessionHandoverPublishDependencies = {},
): Promise<RelayEvent> {
  const signer = dependencies.signer ?? signRelayEvent;
  const publisher = dependencies.publisher ?? relayClient;
  const event = await signer({
    kind: KIND_CODING_SESSION_HANDOVER,
    content: buildCodingSessionHandoverContent({
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
      type: input.type,
      body: input.body,
    }),
    tags: buildCodingSessionHandoverTags({
      channelId: input.channelId,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
      type: input.type,
    }),
  });
  return publisher.publishEvent(
    event,
    "Timed out publishing the handover record.",
    "Failed to publish the handover record.",
  );
}

/**
 * Publish a `takeover` and wait for the relay to accept it.
 *
 * Acceptance is read as the chain **head** being this very event: the relay
 * serializes claims, so a head that is somebody else's link is a lost race,
 * and this reports that rather than retrying into a second claim.
 */
export async function claimCodingSessionHandover(
  input: {
    channelId: string;
    genesisRef: string;
    claimantPubkey: string;
    bodyPubkey: string;
  },
  dependencies: CodingSessionHandoverPublishDependencies = {},
): Promise<{ acceptedEventId: string }> {
  const publishTransition =
    dependencies.publishTransition ?? publishCodingSessionAuthorityTransition;
  const fetchFold = dependencies.fetchFold ?? fetchCodingSessionRosterFold;
  const wait = dependencies.wait ?? defaultWait;
  const attempts = Math.max(1, dependencies.settleAttempts ?? SETTLE_ATTEMPTS);

  const event = await publishTransition({
    channelId: input.channelId,
    genesisRef: input.genesisRef,
    type: "takeover",
    granteePubkey: input.claimantPubkey,
    bodyPubkey: input.bodyPubkey,
  });
  let head: { eventId: string; seq: number } | null = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const fold = await fetchFold(input.channelId, input.genesisRef);
    head = fold.acceptedHead ?? null;
    if (head?.eventId === event.id) {
      return { acceptedEventId: event.id };
    }
    if (attempt + 1 < attempts) await wait(SETTLE_DELAY_MS);
  }
  throw new Error(lostClaimMessage(head, event.id));
}

/**
 * Why a claim did not land, told apart.
 *
 * A head that is **somebody else's** link is a lost race — the relay
 * serialized another claim first — and saying "no receipt" over it would send
 * a person looking for a relay problem that is not there. A head that never
 * moved is the other fact, and gets the other sentence.
 */
function lostClaimMessage(
  head: { eventId: string; seq: number } | null,
  publishedEventId: string,
): string {
  if (head === null || head.eventId === publishedEventId) {
    return "The relay accepted no receipt for this takeover, so the session was not claimed. Nothing was reconstructed.";
  }
  return `Another link won this chain first: the relay's accepted head is ${head.eventId} at seq ${head.seq}, not this takeover. The session is held by whoever that link names — the panel above now says who. Nothing was reconstructed.`;
}

/** How far the continue flow got, whatever happened next. */
export type CodingSessionHandoverContinueProgress = {
  claimEventId: string | null;
  checkout: CodingSessionHandoverCheckoutReport | null;
  createCommandId: string | null;
  target: CodingSessionHandoverTarget | null;
  /**
   * Whether the new execution's own metadata proved it is running in the
   * recovered checkout. `null` when there was nothing to confirm.
   */
  checkoutConfirmed: boolean | null;
  /**
   * Whether the staged folder hint was deliberately left in place because the
   * create's outcome is unknown.
   */
  hintRetained: boolean;
  continuationEventId: string | null;
};

export type CodingSessionHandoverContinueResult =
  | { ok: true; progress: CodingSessionHandoverContinueProgress }
  | {
      ok: false;
      /** The step that refused, named so a reader can act on it. */
      step: "claim" | "checkout" | "create" | "created" | "continuation";
      /**
       * Whether this failure **proves** nothing was accepted.
       *
       * `refused` is a relay saying no in machine-readable words. `unknown`
       * is everything else at or after the publish attempt: a timeout, a
       * dropped socket, a read that failed. The difference decides whether
       * the staged folder hint may be dropped, and whether a caller should
       * keep watching for a late receipt.
       */
      outcome?: "refused" | "unknown";
      reason: string;
      progress: CodingSessionHandoverContinueProgress;
    };

/** The patch this reconstruction will apply, and what it could not get. */
export type CodingSessionCheckpointPatch = {
  /** The NIP-34 patch text, or `null` when there is none to apply. */
  patchText: string | null;
  /** The base that patch was cut against, for `--3way`. */
  baseSha: string | null;
  /** Every artifact this host could not bring across, with the reason. */
  missing: string[];
};

/**
 * Resolve the checkpoint's uncommitted bytes into a patch this host can apply.
 *
 * Three refusals, each disclosed rather than silently dropped:
 *
 * * the patch event is not on the relay (or the read failed) — the checkout
 *   still happens at the wip sha, and the continuation says the patch was not
 *   applied;
 * * it was signed by somebody other than the checkpoint's author — a patch is
 *   the author's own uncommitted work, and applying a stranger's bytes because
 *   a checkpoint pointed at them is exactly the substitution this product
 *   treats as a bug;
 * * it names a different `parent-commit` than the artifact's `baseSha` — the
 *   two disagree about what this diff applies to, so neither is trusted.
 *
 * `blob` artifacts (a patch above the event bound, on Blossom) are **not**
 * fetched by the desktop this increment; the hash is named in `missing` so a
 * person can recover it with the CLI rather than believe it came across.
 */
export async function resolveCheckpointPatch(
  input: {
    artifacts: readonly CodingSessionHandoverArtifact[];
    /** The checkpoint's author — the only key whose patch this will apply. */
    checkpointAuthor: string;
  },
  dependencies: CodingSessionHandoverPublishDependencies = {},
): Promise<CodingSessionCheckpointPatch> {
  const fetchEvents =
    dependencies.fetchEvents ??
    ((filter: RelaySubscriptionFilter) => relayClient.fetchEvents(filter));
  const missing: string[] = [];
  let resolved: { patchText: string; baseSha: string } | null = null;

  for (const artifact of input.artifacts) {
    if (artifact.kind === "blob") {
      missing.push(
        `blob ${artifact.hash} (${artifact.bytes} bytes) is not fetched by this app; recover it with \`bee sessions handover continue\``,
      );
      continue;
    }
    if (artifact.kind !== "patch") continue;
    if (resolved !== null) {
      missing.push(
        `a second patch ${artifact.eventId} was not applied: one checkpoint patch is applied per reconstruction`,
      );
      continue;
    }
    let events: RelayEvent[];
    try {
      events = await fetchEvents({
        ids: [artifact.eventId],
        kinds: [KIND_GIT_PATCH],
        limit: 1,
      });
    } catch (error) {
      missing.push(
        `patch ${artifact.eventId} could not be read from the relay (${reasonOf(error)}), so it was not applied`,
      );
      continue;
    }
    const event = events.find((candidate) => candidate.id === artifact.eventId);
    if (!event) {
      missing.push(
        `patch ${artifact.eventId} was not on the relay, so it was not applied`,
      );
      continue;
    }
    if (event.pubkey !== input.checkpointAuthor) {
      missing.push(
        `patch ${artifact.eventId} was signed by ${event.pubkey}, not by the checkpoint's author ${input.checkpointAuthor}, so it was not applied`,
      );
      continue;
    }
    const parent = patchBaseSha(event);
    if (parent !== null && parent !== artifact.baseSha) {
      missing.push(
        `patch ${artifact.eventId} names base ${parent}, but the checkpoint says ${artifact.baseSha}, so it was not applied`,
      );
      continue;
    }
    if (event.content.trim().length === 0) {
      missing.push(
        `patch ${artifact.eventId} carried no diff, so there was nothing to apply`,
      );
      continue;
    }
    resolved = { patchText: event.content, baseSha: artifact.baseSha };
  }

  return {
    patchText: resolved?.patchText ?? null,
    baseSha: resolved?.baseSha ?? null,
    missing,
  };
}

/** The base a NIP-34 patch event names, when it names one. */
function patchBaseSha(event: RelayEvent): string | null {
  for (const tag of event.tags) {
    if (
      tag.length >= 2 &&
      (tag[0] === "parent-commit" || tag[0] === "base-commit") &&
      typeof tag[1] === "string" &&
      tag[1].length > 0
    ) {
      return tag[1].toLowerCase();
    }
  }
  return null;
}

/**
 * Claim the session, put the work on disk, join it with a new execution, and
 * say what came across.
 *
 * `checkout` is optional: a checkpoint whose artifacts this host cannot fetch
 * — or one that carried none — still reconstructs, and the continuation says
 * plainly that nothing was recovered rather than implying it was.
 */
export async function continueCodingSessionHandover(
  input: {
    channelId: string;
    sessionRef: string;
    genesisRef: string;
    viewerPubkey: string;
    /** This host's provider authority pubkey — the body being claimed. */
    bodyPubkey: string;
    providerInstanceRef: string;
    repoRef: string | null;
    projectRef: string | null;
    title: string | null;
    model: string | null;
    /**
     * The checkpoint that seeds the new execution (§4 step 5).
     *
     * The **body**, not a rendered string: the prompt is written after the
     * recovery runs, so it can carry the branch and sha that actually landed
     * and every line this host could not recover. A prompt built before the
     * fetch describes an intention; this one describes a checkout.
     */
    checkpoint: {
      body: CodingSessionCheckpointBody;
      authorLabel: string;
    };
    /** The checkpoint this reconstruction is from, when there is one. */
    checkpointRef: string | null;
    /** What to fetch and apply, when the checkpoint named artifacts. */
    checkout: CodingSessionHandoverCheckoutRequest | null;
    /** The checkpoint's own artifact list, so its patch can be resolved. */
    artifacts?: readonly CodingSessionHandoverArtifact[];
    /** The checkpoint's author — the only key whose patch this applies. */
    checkpointAuthor?: string;
    /** Lines the checkpoint's author already said were not preserved. */
    declaredMissing: readonly string[];
    note?: string | null;
  },
  dependencies: CodingSessionHandoverPublishDependencies = {},
): Promise<CodingSessionHandoverContinueResult> {
  const progress: CodingSessionHandoverContinueProgress = {
    claimEventId: null,
    checkout: null,
    createCommandId: null,
    target: null,
    checkoutConfirmed: null,
    hintRetained: false,
    continuationEventId: null,
  };

  let claim: { acceptedEventId: string };
  try {
    claim = await claimCodingSessionHandover(
      {
        channelId: input.channelId,
        genesisRef: input.genesisRef,
        claimantPubkey: input.viewerPubkey,
        bodyPubkey: input.bodyPubkey,
      },
      dependencies,
    );
  } catch (error) {
    return { ok: false, step: "claim", reason: reasonOf(error), progress };
  }
  progress.claimEventId = claim.acceptedEventId;

  const recovered: string[] = [];
  const missing: string[] = [...input.declaredMissing];
  const artifacts = input.artifacts ?? [];
  // The author's uncommitted bytes are resolved whether or not there is a wip
  // ref to check out: a checkpoint can carry a patch and no branch, and a
  // patch this host could have read is not "no artifact".
  const patch =
    artifacts.length > 0 && input.checkpointAuthor
      ? await resolveCheckpointPatch(
          { artifacts, checkpointAuthor: input.checkpointAuthor },
          dependencies,
        )
      : { patchText: null, baseSha: null, missing: [] };
  missing.push(...patch.missing);
  if (input.checkout) {
    const prepareCheckout = dependencies.prepareCheckout;
    if (!prepareCheckout) {
      return {
        ok: false,
        step: "checkout",
        reason:
          "this host has no way to prepare a checkout, so nothing was fetched",
        progress,
      };
    }
    try {
      const report = await prepareCheckout({
        ...input.checkout,
        patchText: patch.patchText,
        baseSha: patch.baseSha,
      });
      progress.checkout = report;
      recovered.push(...report.recovered);
      missing.push(...report.missing);
    } catch (error) {
      return { ok: false, step: "checkout", reason: reasonOf(error), progress };
    }
  } else if (artifacts.length === 0) {
    // Two different facts, two different sentences. A checkpoint that named
    // nothing is not the same as one whose artifacts this host never fetched,
    // and signing the first over the second would be a false statement about
    // somebody else's work.
    missing.push("the checkpoint named no artifact to recover");
  } else {
    missing.push(
      `no checkout was prepared on this computer, so ${artifacts.length} artifact(s) named by the checkpoint were not fetched`,
    );
    if (patch.patchText !== null) {
      missing.push(
        "the patch was read but not applied: no checkout was prepared",
      );
    }
  }

  const commandId = createCodingSessionLifecycleCommandId();
  progress.createCommandId = commandId;
  /** A publish that threw without proving non-acceptance, carried forward. */
  let publishError: string | null = null;
  const publishCreate =
    dependencies.publishCreate ?? publishCodingSessionCreate;
  const stageCreateHint =
    dependencies.stageCreateHint ?? stageCodingSessionCreateHint;
  const clearCreateHint =
    dependencies.clearCreateHint ?? clearCodingSessionCreateHint;
  // Bind the create to the directory the work actually landed in. Without it
  // the provider resolves a working directory from its own host file — the
  // command hint first, then project and channel defaults — so a person who
  // chose a different folder could get the model running in the old mapped
  // one while this flow published "recovered". The path never goes on the
  // wire; it is staged on this machine, for this command id only.
  if (input.checkout) {
    try {
      await stageCreateHint({
        commandId,
        path: input.checkout.cwd,
        projectRef: input.projectRef,
      });
    } catch (error) {
      return { ok: false, step: "create", reason: reasonOf(error), progress };
    }
  }
  // The prompt is written **now**: after the fetch, after the patch, with the
  // branch and sha that exist on disk and every line this host could not
  // bring across. The `missing` list here is the same list the continuation
  // signs — one statement, in two places.
  const initialTurn = renderCodingSessionCheckpointTurn({
    body: input.checkpoint.body,
    authorLabel: input.checkpoint.authorLabel,
    checkout: progress.checkout,
    recovered,
    missing,
  });
  try {
    await publishCreate({
      channelId: input.channelId,
      commandId,
      projectRef: input.projectRef,
      repoRef: input.repoRef,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
      providerInstanceRef: input.providerInstanceRef,
      providerAuthorityPubkey: input.bodyPubkey,
      model: input.model,
      title: input.title,
      initialTurn,
    });
  } catch (error) {
    // **An exception is not proof that nothing was accepted.** A durable
    // create can be signed, reach the relay, and be delivered to a provider
    // minutes later; a timeout or a dropped socket says only that this
    // window stopped hearing. The hint is therefore dropped **only** when the
    // relay refused the event in words that prove it was never stored (the
    // NIP-01 machine-readable prefixes, which only ever ride an `OK: false`).
    // Anything else keeps the hint staged, so a late delivery still lands in
    // the recovered checkout.
    if (isProvenRelayRefusal(error)) {
      await clearCreateHint(commandId);
      return {
        ok: false,
        step: "create",
        outcome: "refused",
        reason: reasonOf(error),
        progress,
      };
    }
    publishError = reasonOf(error);
  }

  const settlement = await awaitCodingSessionCreateSettlement(
    { channelId: input.channelId, commandId },
    dependencies,
  );
  if (settlement.settled === "failed") {
    // A provider answered and will not carry this work: settled, so the hint
    // has nothing left to steer.
    await clearCreateHint(commandId).catch(() => {});
    return {
      ok: false,
      step: "created",
      outcome: "refused",
      reason: `the provider answered this create with ${settlement.status}`,
      progress,
    };
  }
  if (settlement.settled === "unknown") {
    // Same rule, one step later: no receipt inside this window's budget, or a
    // read that failed, leaves the create's outcome **unknown**. The hint
    // stays staged for it and the caller keeps watching rather than calling
    // the flow failed.
    progress.hintRetained = true;
    return {
      ok: false,
      step: "created",
      outcome: "unknown",
      reason:
        publishError === null
          ? settlement.reason
          : `${publishError} (and no receipt followed: ${settlement.reason})`,
      progress,
    };
  }
  const target = settlement.target;
  progress.target = target;
  // The hint has done its job the moment a provider reports a session.
  await clearCreateHint(commandId).catch(() => {});

  // Proof, not assumption: the execution's own first metadata must report the
  // branch this host checked out, at the sha the checkpoint named. Anything
  // else — another folder, another commit, or no metadata at all — is said
  // plainly rather than published as "recovered".
  if (input.checkout) {
    const confirmation = await confirmRecoveredCheckout(
      {
        channelId: input.channelId,
        target,
        branch: progress.checkout?.branch ?? null,
        headSha: input.checkout.sha,
      },
      dependencies,
    );
    progress.checkoutConfirmed = confirmation.confirmed;
    if (confirmation.missing !== null) missing.push(confirmation.missing);
  }

  try {
    const event = await publishCodingSessionHandoverRecord(
      {
        channelId: input.channelId,
        sessionRef: input.sessionRef,
        genesisRef: input.genesisRef,
        type: "continuation",
        body: {
          claimRef: claim.acceptedEventId,
          // Always `reconstructed` from the desktop this increment: native
          // continuation is the CLI's, and labelling a new execution as a
          // resumed one would conflate the two outcomes §0 keeps apart.
          mode: "reconstructed",
          checkpointRef: input.checkpointRef,
          target,
          recovered,
          missing,
          note: input.note ?? null,
        },
      },
      dependencies,
    );
    progress.continuationEventId = event.id;
  } catch (error) {
    return {
      ok: false,
      step: "continuation",
      reason: reasonOf(error),
      progress,
    };
  }
  return { ok: true, progress };
}

/**
 * Pick a create back up when its receipt arrives after this window gave up.
 *
 * The unknown outcome is not an ending: the relay may have stored the create
 * and a provider may answer it minutes later. This waits again (a longer,
 * still bounded budget), and when the receipt lands it does exactly what the
 * flow would have done — prove the checkout, publish the continuation, and
 * only then drop the staged hint, because only now is the create's outcome
 * settled.
 */
export async function resumeCodingSessionHandoverContinuation(
  input: {
    channelId: string;
    sessionRef: string;
    genesisRef: string;
    claimEventId: string;
    commandId: string;
    checkpointRef: string | null;
    checkout: CodingSessionHandoverCheckoutRequest | null;
    checkoutReport: CodingSessionHandoverCheckoutReport | null;
    recovered: readonly string[];
    missing: readonly string[];
    note?: string | null;
  },
  dependencies: CodingSessionHandoverPublishDependencies = {},
): Promise<CodingSessionHandoverContinueResult> {
  const clearCreateHint =
    dependencies.clearCreateHint ?? clearCodingSessionCreateHint;
  const progress: CodingSessionHandoverContinueProgress = {
    claimEventId: input.claimEventId,
    checkout: input.checkoutReport,
    createCommandId: input.commandId,
    target: null,
    checkoutConfirmed: null,
    hintRetained: true,
    continuationEventId: null,
  };
  const settlement = await awaitCodingSessionCreateSettlement(
    { channelId: input.channelId, commandId: input.commandId },
    dependencies,
  );
  if (settlement.settled !== "created") {
    if (settlement.settled === "failed") {
      await clearCreateHint(input.commandId).catch(() => {});
      progress.hintRetained = false;
      return {
        ok: false,
        step: "created",
        outcome: "refused",
        reason: `the provider answered this create with ${settlement.status}`,
        progress,
      };
    }
    // Still unknown, and still staged: nothing here proves the create was
    // refused, so nothing here may drop the hint.
    return {
      ok: false,
      step: "created",
      outcome: "unknown",
      reason: settlement.reason,
      progress,
    };
  }
  const target = settlement.target;
  progress.target = target;
  // Settled: a provider answered this exact command. Now the hint has done
  // its job.
  await clearCreateHint(input.commandId).catch(() => {});
  progress.hintRetained = false;

  const missing = [...input.missing];
  if (input.checkout) {
    const confirmation = await confirmRecoveredCheckout(
      {
        channelId: input.channelId,
        target,
        branch: input.checkoutReport?.branch ?? null,
        headSha: input.checkout.sha,
      },
      dependencies,
    );
    progress.checkoutConfirmed = confirmation.confirmed;
    if (confirmation.missing !== null) missing.push(confirmation.missing);
  }
  try {
    const event = await publishCodingSessionHandoverRecord(
      {
        channelId: input.channelId,
        sessionRef: input.sessionRef,
        genesisRef: input.genesisRef,
        type: "continuation",
        body: {
          claimRef: input.claimEventId,
          mode: "reconstructed",
          checkpointRef: input.checkpointRef,
          target,
          recovered: [...input.recovered],
          missing,
          note: input.note ?? null,
        },
      },
      dependencies,
    );
    progress.continuationEventId = event.id;
  } catch (error) {
    return {
      ok: false,
      step: "continuation",
      reason: reasonOf(error),
      progress,
    };
  }
  return { ok: true, progress };
}

function reasonOf(error: unknown): string {
  const reason = error instanceof Error ? error.message.trim() : String(error);
  return reason.length > 0 ? reason : "the step gave no reason";
}

/**
 * Render one checkpoint as the initial turn of the reconstructed execution.
 *
 * The author's own words, in the order §4 step 5 lists them, with the
 * disclosures kept: a `partial` or `none` preservation is stated here too, so
 * the agent picking the work up is told what it does not have rather than
 * discovering it in a diff.
 */
export function renderCodingSessionCheckpointTurn(input: {
  body: CodingSessionCheckpointBody;
  authorLabel: string;
  /** What actually landed on disk, when a checkout ran. */
  checkout?: CodingSessionHandoverCheckoutReport | null;
  recovered?: readonly string[];
  missing?: readonly string[];
}): string {
  const { body } = input;
  const lines: string[] = [
    `You are continuing work handed over by ${input.authorLabel}.`,
    "",
    `Task: ${body.task}`,
  ];
  if (body.decisions.length > 0) {
    lines.push("", "Decisions already made:");
    for (const decision of body.decisions) {
      lines.push(`- ${decision.summary} (${decision.eventId})`);
    }
  }
  lines.push(
    "",
    `Revision: ${body.revision.headSha ?? "no head sha"} on ${
      body.revision.branch ?? "no branch"
    }${body.revision.dirty ? " (the tree was dirty)" : ""}`,
    `Uncommitted work preserved: ${body.revision.preserved}`,
  );
  if (body.tests.length > 0) {
    lines.push("", "Tests as the author last ran them:");
    for (const test of body.tests) {
      lines.push(`- ${test.name}: ${test.outcome} (${test.command})`);
    }
  }
  if (body.unresolved.length > 0) {
    lines.push("", "Unresolved:");
    for (const question of body.unresolved) lines.push(`- ${question}`);
  }
  if (body.artifacts.length > 0) {
    lines.push("", "Artifacts named by the checkpoint:");
    for (const artifact of body.artifacts) {
      lines.push(
        artifact.kind === "wip-ref"
          ? `- wip-ref ${artifact.ref} at ${artifact.sha}`
          : artifact.kind === "patch"
            ? `- patch ${artifact.eventId} (${artifact.bytes} bytes) against ${artifact.baseSha}`
            : `- blob ${artifact.hash} (${artifact.bytes} bytes) against ${artifact.baseSha}`,
      );
    }
  }
  if (input.checkout) {
    lines.push(
      "",
      `Checked out here: ${input.checkout.branch} at ${input.checkout.checkedOutSha}`,
    );
  }
  const recovered = input.recovered ?? [];
  if (recovered.length > 0) {
    lines.push("", "Recovered onto this checkout:");
    for (const line of recovered) lines.push(`- ${line}`);
  }
  const missing = [...(input.missing ?? []), ...body.missing];
  if (missing.length > 0 || body.revision.preserved !== "all") {
    lines.push("", "NOT recovered:");
    if (body.revision.preserved !== "all") {
      lines.push("- Not all uncommitted work was preserved");
    }
    for (const line of missing) lines.push(`- ${line}`);
  }
  lines.push("", `Next action: ${body.nextAction}`);
  return boundedTurn(lines.join("\n"));
}

/**
 * The prompt, bounded to 12 KiB, with the truncation said out loud.
 *
 * The same ceiling `bee sessions send` enforces. A silently clipped prompt
 * would drop the `missing` lines at the end — the very ones a continuing
 * agent most needs.
 */
const MAX_TURN_BYTES = 12 * 1024;

function boundedTurn(turn: string): string {
  const encoder = new TextEncoder();
  if (encoder.encode(turn).byteLength <= MAX_TURN_BYTES) return turn;
  const notice = "\n\n[truncated: this handover prompt exceeded 12 KiB]";
  const room = MAX_TURN_BYTES - encoder.encode(notice).byteLength;
  const bytes = encoder.encode(turn).slice(0, room);
  return `${new TextDecoder().decode(bytes)}${notice}`;
}
