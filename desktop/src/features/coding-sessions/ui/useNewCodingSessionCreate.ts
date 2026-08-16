import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { refreshGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  createCodingSessionLifecycleCommandId,
  createCodingSessionSessionRef,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { publishCodingSessionGenesis } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { useCodingSessionLifecycleResolution } from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { addChannelMembers } from "@/shared/api/tauri";
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
import { bootstrapClaudeCodingSessionRuntime } from "../lib/newCodingSessionModel";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";
import type { NewCodingSessionHostPhase } from "../lib/newCodingSessionModel";

/**
 * The create flow, in the one order that works.
 *
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
    if (
      lifecycle?.state !== "created" &&
      lifecycle?.state !== "created-with-failed-initial-turn"
    ) {
      return null;
    }
    const targetKey = buildCodingSessionTargetKey(lifecycle.target);
    return (
      exactSessionCatalog.entries.find(
        (entry) =>
          entry.commandTarget &&
          buildCodingSessionTargetKey(entry.commandTarget) === targetKey,
      )?.generationId ?? null
    );
  }, [exactSessionCatalog.entries, lifecycle]);

  const settledCommandRef = React.useRef<string | null>(null);

  React.useEffect(() => {
    if (
      !scoped ||
      !resolvedGenerationId ||
      settledCommandRef.current === scoped.input.commandId
    ) {
      return;
    }
    if (
      lifecycle?.state !== "created" &&
      lifecycle?.state !== "created-with-failed-initial-turn"
    ) {
      return;
    }
    settledCommandRef.current = scoped.input.commandId;
    // The hint has done its job the moment the provider reports a session; it
    // must not survive to steer some later command that happens to reuse the
    // id.
    void clearCodingSessionCreateHint(scoped.input.commandId).catch(() => {});
    clearDurableCodingSessionCreate(scopeId);
    onCreated({
      channelId: scoped.input.channelId,
      generationId: resolvedGenerationId,
    });
  }, [lifecycle, onCreated, resolvedGenerationId, scopeId, scoped]);

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
       * Join an existing umbrella instead of founding one (design §B): the
       * create carries the umbrella's ref, so the provider mints a new
       * execution inside the same session. Omit to mint a fresh umbrella.
       */
      sessionRef?: string | null;
      /** Existing genesis to carry when attaching to a founded umbrella. */
      genesisRef?: string | null;
    }) => {
      // `transaction` only exists once prepare has resolved, so on its own it
      // leaves the whole in-flight window unguarded — and this flow puts
      // provisioning and a membership round-trip inside that window. Without
      // `isPublishing` a second submit mints a fresh commandId and creates a
      // duplicate session.
      if (isPublishing || transaction) return;
      setIsPublishing(true);
      setPublishError(null);
      setDurabilityError(null);
      const commandId = createCodingSessionLifecycleCommandId();
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
          });
          await recordCodingSessionWorkdirUse(input.workdir);
        }

        // Strict membership on 442xx means a provider that joins after the
        // command is published never sees it. Joining first is the only order
        // in which the create can be observed at all.
        setHostPhase("joining");
        await addChannelMembers({
          channelId: input.target.channelId,
          pubkeys: [input.target.signerPubkey],
          role: "bot",
        });

        setHostPhase("publishing");
        const prepared = await prepareNewCodingSessionCreate(scopeId, {
          channelId: input.target.channelId,
          commandId,
          providerInstanceRef: input.target.provider.providerInstanceRef,
          providerAuthorityPubkey: input.target.signerPubkey,
          model: input.model,
          title: input.title,
          initialTurn: input.initialTurn,
          ...(input.sessionRef ? { sessionRef: input.sessionRef } : {}),
          ...(input.genesisRef ? { genesisRef: input.genesisRef } : {}),
        });
        if (!prepared.ok) {
          setDurabilityError(prepared.errorMessage);
          setHostPhase("idle");
          return;
        }
        setTransaction(prepared.transaction);
        await publishTransaction(prepared.transaction);
      } catch (error) {
        setPublishError(
          error instanceof Error
            ? error.message
            : "Unable to prepare the signed session request.",
        );
        setHostPhase("idle");
      } finally {
        setIsPublishing(false);
      }
    },
    [
      isPublishing,
      onTrustMutated,
      providerStatus?.providerPubkey,
      publishTransaction,
      scopeId,
      transaction,
    ],
  );

  const retryExact = React.useCallback(() => {
    if (scoped) void publishTransaction(scoped);
  }, [publishTransaction, scoped]);

  const startFresh = React.useCallback(() => {
    if (scoped) {
      void clearCodingSessionCreateHint(scoped.input.commandId).catch(() => {});
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
    settledCommandRef.current = null;
  }, [scoped, scopeId]);

  return {
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
    resolvedGenerationId,
    retryExact,
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
  sessionRef?: string;
  genesisRef?: string;
}): Parameters<typeof prepareDurableCodingSessionCreate>[1] {
  return {
    channelId: input.channelId,
    commandId: input.commandId,
    projectRef: null,
    repoRef: null,
    sessionRef: input.sessionRef ?? createCodingSessionSessionRef(),
    ...(input.genesisRef ? { genesisRef: input.genesisRef } : {}),
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
