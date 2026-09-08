import { resolveCodingSessionCreateModel } from "../lib/codingSessionCreateModel";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { toast } from "sonner";

import { refreshGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  createCodingSessionLifecycleCommandId,
  createCodingSessionSessionRef,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { publishCodingSessionGenesis } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { publishCodingSessionName } from "@/features/coding-sessions/lib/codingSessionName";
import { recordPendingCodingSessionLifecycle } from "@/features/coding-sessions/lib/codingSessionPendingLifecycle";
import { establishedCodingSessionTarget } from "@/features/coding-sessions/lib/codingSessionTrustedIngress";
import { useCodingSessionLifecycleResolution } from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { ensureProviderChannelMembership } from "@/features/coding-sessions/lib/providerChannelMembership";
import { publishSeatedCodingSessionCreate } from "@/features/coding-sessions/lib/codingSessionSeatedCreate";
import { codingSessionSeatDeps } from "@/features/coding-sessions/lib/codingSessionSeatDeps";
import type { CodingSessionActorSeat } from "@/features/coding-sessions/lib/codingSessionActorSeat";
import { ensureCodingSessionCreateOperatorGrants } from "@/features/coding-sessions/lib/codingSessionOperatorGrant";
import type { CodingSessionSeatPackRef } from "@/features/coding-sessions/lib/codingSessionActorSeatCustody";
import { useCodingSessionWorktreeRecorder } from "./useCodingSessionWorktreeRecorder";
import {
  clearCodingSessionCreateHint,
  recordCodingSessionWorkdirUse,
  stageCodingSessionCreateHint,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import {
  ensureCodingSessionProviderRunning,
  getCodingSessionProviderModels,
  getCodingSessionProviderRuntimes,
  getCodingSessionProviderStatus,
  provisionCodingSessionProvider,
  type CodingSessionProviderModels,
  type CodingSessionProviderRuntime,
  type CodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { useCodingSessionCatalog } from "../useCodingSessionCatalog";
import {
  clearDurableCodingSessionCreate,
  loadDurableCodingSessionCreate,
  prepareDurableCodingSessionCreate,
  publishDurableCodingSessionCreate,
  type DurableCodingSessionCreateTransaction,
} from "../lib/durableCodingSessionCreate";
import {
  CODING_SESSION_LOGIN_WATCH_INTERVAL_MS,
  codingSessionLoginWatchVerdict,
} from "../lib/codingSessionLoginWatch";
import {
  bootstrapClaudeCodingSessionRuntime,
  NEW_CODING_SESSION_STALL_MS,
  newCodingSessionWaitKey,
} from "../lib/newCodingSessionModel";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";
import type { NewCodingSessionHostPhase } from "../lib/newCodingSessionModel";

/**
 * What one press of Start came to: the create is on the relay, or it is not
 * and here is why. `publishError` carries the same words for the screen;
 * this is for the caller deciding what to do with the draft.
 */
export type NewCodingSessionSubmitOutcome =
  | { ok: true }
  | { ok: false; message: string };

/**
 * The create flow, in the one order that works.
 *
 * 0. An agent seat, when one was chosen, joins the channel and has its key
 *    material staged host-locally *before* anything is published — the relay
 *    only takes a 44220 from a member, and the provider refuses a create
 *    whose actor has no staged seat (`ACTOR_UNAVAILABLE`). Either failing
 *    leaves the create un-published with a named reason; nothing is signed.
 * 1. The provider must exist and be running, or there is nobody to receive the
 *    command. Provisioning happens lazily here rather than at app start so a
 *    person who never opens a coding session never gets a keychain prompt.
 * 2. The working directory is staged as a host-local hint keyed by the exact
 *    commandId, because it must never travel in the event itself.
 * 3. The provider must be an active member of the channel *before* the command
 *    is published — the relay enforces strict membership on 442xx, so a
 *    provider added afterwards would have silently missed the create.
 * 4. Only then is the pre-signed 44221 published.
 * 5. The 44224 receipt is the authority on what happened. Nothing about the
 *    outcome is inferred from the publish succeeding.
 */
export type NewCodingSessionCreateState = {
  hostPhase: NewCodingSessionHostPhase;
  isPublishing: boolean;
  publishError: string | null;
  durabilityError: string | null;
  transaction: DurableCodingSessionCreateTransaction | null;
  lifecycle: ReturnType<
    typeof useCodingSessionLifecycleResolution
  >["lifecycle"];
  lifecycleIsLoading: boolean;
  lifecycleErrorMessage: string | null;
  resolvedGenerationId: string | null;
  providerStatus: CodingSessionProviderStatus | null;
  providerRuntimes: CodingSessionProviderRuntime[];
  providerModelsByInstanceRef: Map<
    string,
    { defaultModel: string; allowedModels: string[] }
  >;
};

export function useNewCodingSessionCreate({
  scopeId,
  onCreated,
}: {
  scopeId: string;
  onCreated: (input: { channelId: string; generationId: string }) => void;
}) {
  const initial = React.useMemo(
    () => loadDurableCodingSessionCreate(scopeId),
    [scopeId],
  );
  const [transaction, setTransaction] =
    React.useState<DurableCodingSessionCreateTransaction | null>(
      initial.transaction,
    );
  const [durabilityError, setDurabilityError] = React.useState<string | null>(
    initial.errorMessage,
  );
  const [publishError, setPublishError] = React.useState<string | null>(null);
  const [isPublishing, setIsPublishing] = React.useState(false);
  // `null` is not `false`: it means no seat was staged *in this process* —
  // an unseated create, or a durable one rehydrated after a restart. Only a
  // staging call that actually answered turns this into a claim.
  const [seatPackStaged, setSeatPackStaged] = React.useState<boolean | null>(
    null,
  );
  // The repository commit the seat's pack was staged from, when one can
  // vouch for it (finding 84). Null is "no repository named", never a guess.
  const [seatPackRef, setSeatPackRef] =
    React.useState<CodingSessionSeatPackRef | null>(null);
  const [hostPhase, setHostPhase] =
    React.useState<NewCodingSessionHostPhase>("idle");
  const [providerStatus, setProviderStatus] =
    React.useState<CodingSessionProviderStatus | null>(null);
  const [providerRuntimes, setProviderRuntimes] = React.useState<
    CodingSessionProviderRuntime[]
  >(() => [bootstrapClaudeCodingSessionRuntime()]);
  const [providerModelsByInstanceRef, setProviderModelsByInstanceRef] =
    React.useState<
      Map<string, { defaultModel: string; allowedModels: string[] }>
    >(() => new Map());

  // Provisioning (and every provider start) seeds `allowed-bridge-pubkeys`
  // from Rust. The trusted ingress reads that list through the shared config
  // query, whose staleTime is Infinity — without this refetch, a first-run
  // ingress stays armed against the pre-provision (empty) trust set and the
  // create receipt is never admitted until the window reloads.
  const queryClient = useQueryClient();
  const onTrustMutated = React.useCallback(() => {
    refreshGlobalAgentConfig(queryClient);
  }, [queryClient]);

  React.useEffect(() => {
    const loaded = loadDurableCodingSessionCreate(scopeId);
    setTransaction(loaded.transaction);
    setDurabilityError(loaded.errorMessage);
    setPublishError(null);
    setIsPublishing(false);
    setHostPhase("idle");
    setSeatPackStaged(null);
    setSeatPackRef(null);
  }, [scopeId]);

  React.useEffect(() => {
    let cancelled = false;
    void loadOrProvisionCodingSessionProvider({
      onProvisioning: () => {
        if (!cancelled) setHostPhase("provisioning");
      },
      onTrustMutated,
    })
      .then((status) => {
        if (!cancelled) {
          setProviderStatus(status);
        }
        return loadCodingSessionProviderRuntimes({
          onRuntimes: (runtimes) => {
            if (!cancelled) setProviderRuntimes(runtimes);
          },
          onModels: (instanceRef, models) => {
            if (cancelled) return;
            setProviderModelsByInstanceRef((previous) => {
              const next = new Map(previous);
              next.set(instanceRef, models);
              return next;
            });
          },
        });
      })
      .then(() => {
        if (!cancelled) {
          setHostPhase("idle");
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setProviderStatus(null);
          setHostPhase("idle");
          setPublishError(
            typeof error === "string"
              ? error
              : error instanceof Error
                ? error.message
                : "Unable to set up this computer's coding-session provider.",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [onTrustMutated]);

  /**
   * Re-run the host runtimes probe and fold the answer into state.
   *
   * The backend re-runs each runtime's CLI auth check on every invoke, so a
   * refetch IS a re-probe — this is how a completed sign-in becomes visible
   * without restarting the app.
   */
  const refreshProviderState = React.useCallback(async () => {
    const status = await getCodingSessionProviderStatus();
    setProviderStatus(status);
    let latest: CodingSessionProviderRuntime[] = [];
    const latestModels = new Map<
      string,
      { defaultModel: string; allowedModels: string[] }
    >();
    await loadCodingSessionProviderRuntimes({
      onRuntimes: (runtimes) => {
        latest = runtimes;
        setProviderRuntimes(runtimes);
      },
      onModels: (instanceRef, models) => {
        latestModels.set(instanceRef, models);
      },
    });
    setProviderModelsByInstanceRef(latestModels);
    return { status, runtimes: latest, modelsByInstanceRef: latestModels };
  }, []);
  const refreshProviderRuntimes = React.useCallback(
    async () => (await refreshProviderState()).runtimes,
    [refreshProviderState],
  );

  // Post-Connect login watch: nothing signals when a person finishes the
  // vendor's sign-in, so poll the probe until the launched runtime reports
  // ready or the watch expires. A window refocus (coming back from the
  // browser or terminal) probes immediately instead of waiting out the tick.
  const loginWatchRef = React.useRef<{
    timer: ReturnType<typeof setInterval>;
    runtime: string;
    startedAt: number;
  } | null>(null);
  const stopLoginWatch = React.useCallback(() => {
    if (loginWatchRef.current) {
      clearInterval(loginWatchRef.current.timer);
      loginWatchRef.current = null;
    }
  }, []);
  const runLoginWatchTick = React.useCallback(async () => {
    const watch = loginWatchRef.current;
    if (!watch) return;
    const runtimes = await refreshProviderRuntimes().catch(
      () => [] as CodingSessionProviderRuntime[],
    );
    if (loginWatchRef.current !== watch) return;
    const verdict = codingSessionLoginWatchVerdict({
      runtime: watch.runtime,
      startedAt: watch.startedAt,
      now: Date.now(),
      runtimes,
    });
    if (verdict !== "continue") stopLoginWatch();
  }, [refreshProviderRuntimes, stopLoginWatch]);
  const beginLoginWatch = React.useCallback(
    (runtime: string) => {
      stopLoginWatch();
      loginWatchRef.current = {
        timer: setInterval(
          () => void runLoginWatchTick(),
          CODING_SESSION_LOGIN_WATCH_INTERVAL_MS,
        ),
        runtime,
        startedAt: Date.now(),
      };
    },
    [runLoginWatchTick, stopLoginWatch],
  );
  React.useEffect(() => {
    const onFocus = () => {
      if (loginWatchRef.current) void runLoginWatchTick();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      stopLoginWatch();
    };
  }, [runLoginWatchTick, stopLoginWatch]);

  const scoped = transaction?.scopeId === scopeId ? transaction : null;
  const lifecycleSnapshot = useCodingSessionLifecycleResolution(
    scoped?.input.channelId ?? null,
    scoped?.input.commandId ?? null,
    scoped?.input.providerAuthorityPubkey ?? null,
  );
  const lifecycle = lifecycleSnapshot.lifecycle;
  const exactSessionCatalog = useCodingSessionCatalog(
    scoped?.input.channelId ?? null,
  );
  const resolvedGenerationId = React.useMemo(() => {
    const established = establishedCodingSessionTarget(lifecycle);
    if (!established) return null;
    const targetKey = buildCodingSessionTargetKey(established);
    const providerAuthorityPubkey = scoped?.input.providerAuthorityPubkey;
    if (!providerAuthorityPubkey) return null;
    return (
      exactSessionCatalog.entries.find(
        (entry) =>
          entry.commandTarget &&
          buildCodingSessionTargetKey(entry.commandTarget) === targetKey &&
          entry.providerAuthorityPubkey === providerAuthorityPubkey &&
          entry.metadataAuthorityPubkey === providerAuthorityPubkey,
      )?.generationId ?? null
    );
  }, [exactSessionCatalog.entries, lifecycle, scoped]);

  // The signed-fact resolution is clock-free by design, so the deadline lives
  // here: any open-ended wait (provider accept, metadata, catalog join) that
  // outlives the stall window escalates from spinner to diagnosis. Keyed on
  // the durable createdAt so a wedge rehydrated after a restart surfaces
  // immediately instead of buying itself another quiet window.
  const waitKey = newCodingSessionWaitKey({
    lifecycleState: lifecycle?.state ?? null,
    resolvedGenerationId,
    hasTransaction: scoped !== null,
  });
  const [stalled, setStalled] = React.useState(false);
  const stallAnchor = scoped?.createdAt ?? null;
  React.useEffect(() => {
    setStalled(false);
    if (!waitKey || stallAnchor === null) return;
    const elapsed = Date.now() - stallAnchor;
    const remaining = Math.max(0, NEW_CODING_SESSION_STALL_MS - elapsed);
    const timer = window.setTimeout(() => setStalled(true), remaining);
    return () => window.clearTimeout(timer);
  }, [waitKey, stallAnchor]);

  const settledCommandRef = React.useRef<string | null>(null);
  const { remember: rememberTree, settle: settleTree } =
    useCodingSessionWorktreeRecorder();

  React.useEffect(() => {
    if (
      !scoped ||
      !resolvedGenerationId ||
      settledCommandRef.current === scoped.input.commandId
    ) {
      return;
    }
    if (establishedCodingSessionTarget(lifecycle) === null) return;
    settledCommandRef.current = scoped.input.commandId;
    // The hint has done its job the moment the provider reports a session; it
    // must not survive to steer some later command that happens to reuse the
    // id.
    void clearCodingSessionCreateHint(scoped.input.commandId).catch(() => {});
    settleTree(scoped.input.commandId, resolvedGenerationId, lifecycle);
    // The catalog join above is exact signed metadata from this create's
    // provider. Only now may the founder grant that provider durable wake
    // authority, followed by the actor's steering authority on the same
    // receipt-backed chain.
    const seatActor = scoped.input.actor;
    const seatGenesis = scoped.input.genesisRef;
    if (seatActor) {
      // The provider deletes its own copy of the staged seat at spawn, so
      // this is only the cleanup for the paths where it did not.
      void codingSessionSeatDeps
        .clearSeat(scoped.input.commandId)
        .catch(() => {});
    }
    void (async () => {
      if (!seatGenesis) {
        toast.error(
          "The session was created, but it has no genesis to grant against — provider wake and seat steering remain unavailable.",
        );
      } else {
        const grantResult = await ensureCodingSessionCreateOperatorGrants({
          channelId: scoped.input.channelId,
          genesisRef: seatGenesis,
          providerAuthorityPubkey: scoped.input.providerAuthorityPubkey,
          actorPubkey: seatActor ?? null,
        });
        if (!grantResult.ok) {
          const label =
            grantResult.failed === "provider"
              ? "provider wake authority"
              : "agent operator authority";
          toast.error(
            `The session was created but could not receive ${label}: ${grantResult.reason} Grant it in People before relying on provider wake or sibling steering.`,
          );
        }
      }
      clearDurableCodingSessionCreate(scopeId);
      onCreated({
        channelId: scoped.input.channelId,
        generationId: resolvedGenerationId,
      });
    })();
  }, [lifecycle, onCreated, resolvedGenerationId, scopeId, scoped, settleTree]);

  const publishTransaction = React.useCallback(
    async (exact: DurableCodingSessionCreateTransaction) => {
      setIsPublishing(true);
      setPublishError(null);
      setDurabilityError(null);
      const result = await publishDurableCodingSessionCreate(exact, {
        onTransaction: setTransaction,
      });
      setTransaction(result.transaction);
      setPublishError(result.publishError);
      setDurabilityError(result.persistenceError);
      setIsPublishing(false);
      setHostPhase("idle");
      if (result.accepted) {
        // The relay holds the signed create; show the session in the sidebar
        // immediately instead of waiting for the provider's 44223 facts.
        recordPendingCodingSessionLifecycle({
          kind: "create",
          channelId: exact.input.channelId,
          commandId: exact.input.commandId,
          sessionRef: exact.input.sessionRef ?? null,
          title: exact.input.title,
          projectRef: exact.input.projectRef,
          providerAuthorityPubkey: exact.input.providerAuthorityPubkey,
          hasInitialTurn: exact.input.initialTurn !== null,
          recordedAt: Date.now(),
        });
      }
    },
    [],
  );

  const submit = React.useCallback(
    async (input: {
      target: NewCodingSessionTarget;
      model: string | null;
      title: string | null;
      initialTurn: string | null;
      workdir: string | null;
      /**
       * Seat a managed agent on this execution (design D1/D6): its pubkey and
       * role are signed into the create, its key material is staged
       * host-locally, and it is added to the channel before the publish and
       * granted operator once the create's receipt lands. Omit for an
       * ordinary human-created execution, whose bytes are unchanged.
       */
      seat?: CodingSessionActorSeat | null;
      /** Display name for the seat, used only in failure copy. */
      seatLabel?: string | null;
      /**
       * The directory to *remember* for next time, when it differs from the
       * one the session runs in.
       *
       * A worktree create runs the session in a directory that did not exist
       * a moment ago. Promoting that to the head of the recent list would
       * prefill the next session with the last one's worktree — and then a
       * worktree of a worktree. What a person actually returns to is the
       * checkout the worktree came from, so that is what is stored.
       */
      rememberWorkdir?: string | null;
      /**
       * Join an existing umbrella instead of founding one (design §B): the
       * create carries the umbrella's ref, so the provider mints a new
       * execution inside the same session. Omit to mint a fresh umbrella.
       */
      sessionRef?: string | null;
      /** Existing genesis to carry when attaching to a founded umbrella. */
      genesisRef?: string | null;
      /**
       * Project coordinate to sign into the create. This is the session's
       * placement authority for the rest of its life — the projects sidebar
       * reads it back off the 44223 metadata — so it is written once, here,
       * and never inferred later. Standalone creation passes nothing.
       */
      projectRef?: string | null;
      repoRef?: string | null;
    }): Promise<NewCodingSessionSubmitOutcome> => {
      // `transaction` only exists once prepare has resolved, so on its own it
      // leaves the whole in-flight window unguarded — and this flow puts
      // provisioning and a membership round-trip inside that window. Without
      // `isPublishing` a second submit mints a fresh commandId and creates a
      // duplicate session.
      if (isPublishing || transaction) {
        return {
          ok: false,
          message: "A session request is already in flight.",
        };
      }
      setIsPublishing(true);
      setPublishError(null);
      setDurabilityError(null);
      const commandId = createCodingSessionLifecycleCommandId();
      rememberTree(commandId, input.workdir);
      // Set when the durable step refuses. It reports itself in its own
      // field, so the catch below must not repeat it as a publish error.
      let durabilityRefusal: string | null = null;
      try {
        const status = await ensureLocalProvider({
          isLocalProvider: (pubkey) =>
            pubkey === providerStatus?.providerPubkey,
          signerPubkey: input.target.signerPubkey,
          setHostPhase,
          onTrustMutated,
        });
        if (status) setProviderStatus(status);

        if (input.workdir) {
          await stageCodingSessionCreateHint({
            commandId,
            path: input.workdir,
            projectRef: input.projectRef ?? null,
          });
          await recordCodingSessionWorkdirUse(
            input.rememberWorkdir ?? input.workdir,
          );
        }

        // Strict membership on 442xx means a provider that joins after the
        // command is published never sees it. Joining first is the only order
        // in which the create can be observed at all — and the join is only
        // real once the roster shows it, so this verifies rather than trusts
        // the publish.
        setHostPhase("joining");
        await ensureProviderChannelMembership({
          channelId: input.target.channelId,
          providerPubkey: input.target.signerPubkey,
        });

        // The seat's membership and its host-local custody entry both come
        // before the publish, and either failing means nothing is published
        // (see `publishSeatedCodingSessionCreate`).
        await publishSeatedCodingSessionCreate({
          channelId: input.target.channelId,
          commandId,
          seat: input.seat ?? null,
          seatLabel: input.seatLabel ?? null,
          // The same coordinate the create is signed with: the seat's pack is
          // looked up on the project the session is filed under.
          projectRef: input.projectRef ?? null,
          deps: codingSessionSeatDeps,
          // What custody actually wrote, so the pending screen can say a seat
          // is running without its role pack instead of implying it has one —
          // and, when a repository vouches for the pack, which commit.
          onSeatStaged: ({ packStaged, packRef }) => {
            setSeatPackStaged(packStaged);
            setSeatPackRef(packRef);
          },
          publish: async () => {
            setHostPhase("publishing");
            // Genesis-publishing wrapper (founding publishes a genesis;
            // joining carries the umbrella's). Glue's project coordinate
            // rides the same input — the durable helper is never called
            // directly here.
            const prepared = await prepareNewCodingSessionCreate(scopeId, {
              channelId: input.target.channelId,
              commandId,
              providerInstanceRef: input.target.provider.providerInstanceRef,
              providerAuthorityPubkey: input.target.signerPubkey,
              // The catalog's id, never the picker's label (item 88(b)).
              model: resolveCodingSessionCreateModel({
                model: input.model,
                catalog: providerModelsByInstanceRef.get(
                  input.target.provider.providerInstanceRef,
                ) ?? {
                  defaultModel: input.target.provider.defaultModel,
                  allowedModels: input.target.provider.allowedModels,
                },
              }),
              title: input.title,
              initialTurn: input.initialTurn,
              projectRef: input.projectRef ?? null,
              repoRef: input.repoRef ?? null,
              ...(input.sessionRef ? { sessionRef: input.sessionRef } : {}),
              ...(input.genesisRef ? { genesisRef: input.genesisRef } : {}),
              ...(input.seat
                ? { actor: input.seat.actor, role: input.seat.role }
                : {}),
            });
            if (!prepared.ok) {
              durabilityRefusal = prepared.errorMessage;
              setDurabilityError(prepared.errorMessage);
              setHostPhase("idle");
              // A create that was never signed is a create that never went
              // out, so the staged seat must go with it.
              throw new Error(prepared.errorMessage);
            }
            setTransaction(prepared.transaction);
            await publishTransaction(prepared.transaction);
          },
        });
      } catch (error) {
        const message =
          error instanceof Error
            ? error.message
            : "Unable to prepare the signed session request.";
        // A durability refusal already reported itself in its own field; do
        // not say the same thing twice in two different tones.
        if (message !== durabilityRefusal) setPublishError(message);
        setHostPhase("idle");
        // Said to the caller as well as the screen: the form clears its
        // draft on success only, and a relay refusal such as
        // `rate-limited: quota exceeded` used to eat the goal (2026-09-07).
        return { ok: false, message };
      } finally {
        setIsPublishing(false);
      }
      return { ok: true };
    },
    [
      isPublishing,
      onTrustMutated,
      providerModelsByInstanceRef,
      providerStatus?.providerPubkey,
      publishTransaction,
      scopeId,
      rememberTree,
      transaction,
    ],
  );

  const retryExact = React.useCallback(() => {
    if (scoped) void publishTransaction(scoped);
  }, [publishTransaction, scoped]);

  const startFresh = React.useCallback(() => {
    if (scoped) {
      void clearAbandonedCodingSessionCreate({
        commandId: scoped.input.commandId,
        actor: scoped.input.actor ?? null,
        clearHint: clearCodingSessionCreateHint,
        clearSeat: codingSessionSeatDeps.clearSeat,
      });
    }
    const cleared = clearDurableCodingSessionCreate(scopeId);
    if (!cleared.ok) {
      setDurabilityError(cleared.errorMessage);
      return;
    }
    setTransaction(null);
    setPublishError(null);
    setDurabilityError(null);
    setHostPhase("idle");
    setSeatPackStaged(null);
    setSeatPackRef(null);
    settledCommandRef.current = null;
  }, [scoped, scopeId]);

  // The seat as the *signed* create carries it, not as the form holds it: a
  // rehydrated transaction has no form state left, and this is the pair that
  // actually went out.
  const seat = React.useMemo(() => {
    const actor = scoped?.input.actor ?? null;
    const role = scoped?.input.role ?? null;
    return actor && role ? { actor, role } : null;
  }, [scoped?.input.actor, scoped?.input.role]);

  return {
    beginLoginWatch,
    durabilityError,
    hostPhase,
    isPublishing,
    lifecycle,
    lifecycleErrorMessage: lifecycleSnapshot.errorMessage,
    lifecycleIsLoading: lifecycleSnapshot.isLoading,
    providerStatus,
    providerRuntimes,
    providerModelsByInstanceRef,
    publishError,
    refreshProviderRuntimes,
    refreshProviderState,
    resolvedGenerationId,
    retryExact,
    seat,
    /** What staging wrote for {@link seat}, or null when it never ran here. */
    seatPackStaged,
    /** The repository commit that pack came from, when one vouched for it. */
    seatPackRef,
    stalled,
    startFresh,
    submit,
    transaction: scoped,
  };
}

/**
 * Assemble the durable-create input for a brand-new session draft.
 *
 * Every new create mints a fresh umbrella `sessionRef` (design §A): a
 * single-execution session is an umbrella of one, so adding a second provider
 * later is a pure join with no migration step. The "Add provider" entry point
 * on a session workspace passes that umbrella's existing ref instead of
 * minting — that is the only difference between founding and joining.
 */
export function buildNewCodingSessionCreateInput(input: {
  channelId: string;
  commandId: string;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
  projectRef?: string | null;
  repoRef?: string | null;
  sessionRef?: string;
  genesisRef?: string;
  /** Agent seat, both halves or neither — the builder refuses a lone one. */
  actor?: string;
  role?: string;
}): Parameters<typeof prepareDurableCodingSessionCreate>[1] {
  return {
    channelId: input.channelId,
    commandId: input.commandId,
    projectRef: input.projectRef ?? null,
    repoRef: input.repoRef ?? null,
    sessionRef: input.sessionRef ?? createCodingSessionSessionRef(),
    ...(input.genesisRef ? { genesisRef: input.genesisRef } : {}),
    ...(input.actor !== undefined && input.role !== undefined
      ? { actor: input.actor, role: input.role }
      : {}),
    providerInstanceRef: input.providerInstanceRef,
    providerAuthorityPubkey: input.providerAuthorityPubkey,
    model: input.model,
    title: input.title,
    initialTurn: input.initialTurn,
  };
}

/**
 * Prepare the durable create, publishing a genesis first only for founding.
 *
 * The returned genesis event id is the exact value written into the signed
 * create. Joining an existing umbrella never publishes another genesis; it
 * carries the umbrella's already-resolved id when one exists, while legacy
 * umbrellas preserve the historical 9-key create form.
 */
export async function prepareNewCodingSessionCreate(
  scopeId: string,
  input: Parameters<typeof buildNewCodingSessionCreateInput>[0],
  dependencies: {
    publishGenesis?: typeof publishCodingSessionGenesis;
    publishName?: typeof publishCodingSessionName;
    prepareCreate?: typeof prepareDurableCodingSessionCreate;
  } = {},
): ReturnType<typeof prepareDurableCodingSessionCreate> {
  const createInput = buildNewCodingSessionCreateInput(input);
  const prepareCreate =
    dependencies.prepareCreate ?? prepareDurableCodingSessionCreate;
  if (input.sessionRef !== undefined) {
    return prepareCreate(scopeId, createInput);
  }
  const sessionRef = createInput.sessionRef;
  if (!sessionRef) {
    return {
      ok: false,
      errorMessage: "Unable to mint the coding session reference.",
    };
  }
  const genesis = await (
    dependencies.publishGenesis ?? publishCodingSessionGenesis
  )({
    channelId: input.channelId,
    sessionRef,
  });
  if (input.title?.trim()) {
    await (dependencies.publishName ?? publishCodingSessionName)({
      channelId: input.channelId,
      content: input.title,
      sessionRef,
    });
  }
  return prepareCreate(scopeId, {
    ...createInput,
    genesisRef: genesis.eventId,
  });
}

/**
 * Load the host runtime table, then each ready runtime's models.
 *
 * The runtimes command failing (an older desktop backend without it) degrades
 * to the bundled claude-only descriptor — exactly the pre-runtimes behavior.
 * Model lookups run per runtime and each failure is swallowed independently:
 * one broken adapter must not cost the others their model lists.
 */
/**
 * Drop every host-local trace of a create that will never be answered.
 *
 * Two files on this computer are keyed by a create's exact `commandId`: the
 * working-directory hint the provider resolves the session's `cwd` from, and —
 * for a seated create — the custody entry holding the agent's `nsec`. They are
 * written together and they have to be abandoned together. Clearing only the
 * hint leaves a secret at rest under a command nothing will ever consume,
 * which is the same defect as leaking it, just quieter.
 *
 * Best effort by contract: the create is already being abandoned, and a
 * cleanup write that cannot land must not keep the person on a dead screen.
 */
export async function clearAbandonedCodingSessionCreate(input: {
  commandId: string;
  actor: string | null;
  clearHint: (commandId: string) => Promise<unknown>;
  clearSeat: (commandId: string) => Promise<unknown>;
}): Promise<void> {
  await input.clearHint(input.commandId).catch(() => {});
  if (input.actor) await input.clearSeat(input.commandId).catch(() => {});
}

export {
  CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE,
  CODING_SESSION_CREATE_UNNAMED_MODEL_LABEL,
  type CodingSessionCreateModelCatalog,
  codingSessionCreateModelDisclosure,
  codingSessionCreateModelLabel,
  resolveCodingSessionCreateModel,
} from "../lib/codingSessionCreateModel";

export async function loadCodingSessionProviderRuntimes({
  getRuntimes = getCodingSessionProviderRuntimes,
  getModels = getCodingSessionProviderModels,
  onRuntimes,
  onModels,
}: {
  getRuntimes?: () => Promise<CodingSessionProviderRuntime[]>;
  getModels?: (instanceRef: string) => Promise<CodingSessionProviderModels>;
  onRuntimes: (runtimes: CodingSessionProviderRuntime[]) => void;
  onModels: (
    instanceRef: string,
    models: { defaultModel: string; allowedModels: string[] },
  ) => void;
}): Promise<void> {
  const runtimes = await getRuntimes()
    .then((listed) =>
      listed.length > 0 ? listed : [bootstrapClaudeCodingSessionRuntime()],
    )
    .catch(() => [bootstrapClaudeCodingSessionRuntime()]);
  onRuntimes(runtimes);
  await Promise.all(
    runtimes
      .filter((runtime) => runtime.authState === "ready")
      .map((runtime) =>
        getModels(runtime.instanceRef)
          .then((models) => {
            onModels(runtime.instanceRef, {
              defaultModel: models.defaultModel,
              allowedModels: models.allowedModels,
            });
          })
          .catch(() => {}),
      ),
  );
}

/**
 * Read this relay's local provider, provisioning it on the first visit to the
 * create screen. Keeping this out of app startup means people who never use
 * coding sessions never mint a provider identity.
 */
export async function loadOrProvisionCodingSessionProvider({
  getStatus = getCodingSessionProviderStatus,
  onProvisioning = () => {},
  onTrustMutated = () => {},
  provision = provisionCodingSessionProvider,
}: {
  getStatus?: () => Promise<CodingSessionProviderStatus>;
  onProvisioning?: () => void;
  /**
   * Fired after any backend call that seeds `allowed-bridge-pubkeys`
   * (`session_provider/trust.rs`), so config readers can refetch the trust
   * list this render is otherwise caching forever.
   */
  onTrustMutated?: () => void;
  provision?: () => Promise<CodingSessionProviderStatus>;
} = {}): Promise<CodingSessionProviderStatus> {
  const status = await getStatus();
  if (status.provisioned) return status;
  onProvisioning();
  const provisioned = await provision();
  onTrustMutated();
  return provisioned;
}

/**
 * Make sure this computer's provider exists and is running.
 *
 * Only for the local provider. A target signed by some other machine's
 * provider is that machine's business — provisioning here would mint a second
 * identity for no reason.
 */
async function ensureLocalProvider(input: {
  isLocalProvider: (pubkey: string) => boolean;
  signerPubkey: string;
  setHostPhase: (phase: NewCodingSessionHostPhase) => void;
  onTrustMutated: () => void;
}): Promise<CodingSessionProviderStatus | null> {
  const status = await getCodingSessionProviderStatus().catch(() => null);
  if (status && !status.provisioned) {
    input.setHostPhase("provisioning");
    const provisioned = await provisionCodingSessionProvider();
    input.onTrustMutated();
    return provisioned;
  }
  if (!status || !input.isLocalProvider(input.signerPubkey)) {
    return status;
  }
  if (!status.running) {
    input.setHostPhase("starting");
    // Starting also re-seeds trust (`supervisor.rs` re-asserts the entry on
    // every start), so readers must refetch here too.
    const running = await ensureCodingSessionProviderRunning();
    input.onTrustMutated();
    return running;
  }
  return status;
}
