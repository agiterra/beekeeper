import * as React from "react";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { createCodingSessionLifecycleCommandId } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { useCodingSessionLifecycleResolution } from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { addChannelMembers } from "@/shared/api/tauri";
import {
  clearCodingSessionCreateHint,
  recordCodingSessionWorkdirUse,
  stageCodingSessionCreateHint,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import {
  ensureCodingSessionProviderRunning,
  getCodingSessionProviderStatus,
  provisionCodingSessionProvider,
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
    void getCodingSessionProviderStatus()
      .then((status) => {
        if (!cancelled) setProviderStatus(status);
      })
      .catch(() => {
        if (!cancelled) setProviderStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

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
        const prepared = await prepareDurableCodingSessionCreate(scopeId, {
          channelId: input.target.channelId,
          commandId,
          projectRef: null,
          repoRef: null,
          providerInstanceRef: input.target.provider.providerInstanceRef,
          providerAuthorityPubkey: input.target.signerPubkey,
          model: input.model,
          title: input.title,
          initialTurn: input.initialTurn,
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
    publishError,
    resolvedGenerationId,
    retryExact,
    startFresh,
    submit,
    transaction: scoped,
  };
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
}): Promise<CodingSessionProviderStatus | null> {
  const status = await getCodingSessionProviderStatus().catch(() => null);
  if (status && !status.provisioned) {
    input.setHostPhase("provisioning");
    return provisionCodingSessionProvider();
  }
  if (!status || !input.isLocalProvider(input.signerPubkey)) {
    return status;
  }
  if (!status.running) {
    input.setHostPhase("starting");
    return ensureCodingSessionProviderRunning();
  }
  return status;
}
