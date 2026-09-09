import * as React from "react";

import { useCommunities } from "@/features/communities/useCommunities";
import { useIdentityQuery } from "@/shared/api/hooks";
import { invokeTauri } from "@/shared/api/tauri";
import {
  getCodingSessionProviderRuntimes,
  getCodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { useCodingSessionHandover } from "../hooks/useCodingSessionHandover";
import {
  continueCodingSessionHandover,
  claimCodingSessionHandover,
  type CodingSessionHandoverCheckoutReport,
  type CodingSessionHandoverCheckoutRequest,
} from "../lib/codingSessionHandoverPublish";
import {
  codingSessionHandoverGenerations,
  deriveCodingSessionHandoverModel,
} from "../lib/codingSessionHandoverModel";
import type { CodingSessionCommandTarget } from "../lib/codingSessionCommand";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "../lib/codingSessionTypes";
import { CodingSessionHandoverPanel } from "./CodingSessionHandoverPanel";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";

/** The Tauri command that puts an absent participant's work on this disk. */
export const HANDOVER_PREPARE_CHECKOUT_COMMAND = "handover_prepare_checkout";

/**
 * The handover surface's one mount: read, fold, model, render, act.
 *
 * Its own component rather than lines inside `CodingSessionWorkspace` for two
 * reasons. The workspace is close to this repo's 1,000-line ceiling, and — the
 * one that matters — the flow this panel runs publishes a claim that fences
 * another machine, so it belongs somewhere a test can drive end to end without
 * mounting a whole session.
 *
 * Nothing here retries. Every refusal is rendered with the step and the reason
 * the write gave, because "it didn't work" over a fenced session is exactly
 * the kind of comfortable silence this product treats as a bug.
 *
 * It owns its own spacing and renders **nothing at all** when the session has
 * no claim, no fence, no continuation and nothing this viewer could continue —
 * an ordinary session pays no chrome for a capability it is not using.
 */
export function CodingSessionHandoverHost({
  channelId,
  umbrella,
  focusedExecution,
  resolveReachability,
  providerInstanceRef = null,
  projectRef = null,
  title = null,
  workdir = null,
  retired = false,
  retiredAt = null,
  resolveName,
  onOpenEvidence,
}: {
  channelId: string;
  /** The umbrella this panel is about: its refs, founder and executions. */
  umbrella: CodingSessionUmbrellaRecord;
  /** The execution the viewer is looking at, for the fence question. */
  focusedExecution: CodingSessionExecution;
  /**
   * The workspace's own reachability resolver, threaded rather than re-made.
   *
   * One coordination subscription per open session: a second call to the
   * resolver hook would open a second live REQ for the same answer.
   */
  resolveReachability: (target: CodingSessionCommandTarget | null) => {
    known: boolean;
    reachable?: boolean;
  };
  /** This host's provider instance, when one is connected. */
  providerInstanceRef?: string | null;
  projectRef?: string | null;
  title?: string | null;
  /** The checkout a reconstruction prepares. Absent means nothing is fetched. */
  workdir?: string | null;
  retired?: boolean;
  retiredAt?: number | null;
  resolveName?: (pubkey: string) => string | null;
  onOpenEvidence?: (eventId: string) => void;
}) {
  const identity = useIdentityQuery();
  const { activeCommunity } = useCommunities();
  const relayOrigin = relayOriginOf(activeCommunity?.relayUrl ?? null);
  const viewerPubkey = identity.data?.pubkey ?? null;
  const { founderPubkey, genesisRef, sessionRef } = umbrella;
  const executions = umbrella.executions;
  const repoRef = focusedExecution.activeGeneration.repoRef;
  const thisExecutionProviderPubkey =
    focusedExecution.activeGeneration.providerAuthorityPubkey;
  const scope =
    sessionRef !== null && genesisRef !== null && founderPubkey !== null
      ? { channelRef: channelId, sessionRef, genesisRef, founderPubkey }
      : null;
  const read = useCodingSessionHandover(scope, { retired });
  const [busy, setBusy] = React.useState<"continue" | "take-back" | null>(null);
  // The app's own workdir picker holds this: one remembered directory per
  // channel and project, not a second answer to "where does my code live".
  const [checkoutPath, setCheckoutPath] = React.useState(workdir ?? "");
  // **This** machine's provider — the body a claim from here names. Never the
  // viewed execution's provider: claiming somebody else's body would fence an
  // execution this computer cannot run, which is the opposite of taking work
  // over. `null` means there is nothing here to continue on, and the panel
  // says so rather than offering an action that cannot work.
  const [localBody, setLocalBody] = React.useState<{
    pubkey: string;
    instanceRef: string;
  } | null>(null);
  React.useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const status = await getCodingSessionProviderStatus();
        if (!status.providerPubkey) return;
        const runtimes = await getCodingSessionProviderRuntimes();
        const instanceRef = runtimes[0]?.instanceRef ?? null;
        if (cancelled || instanceRef === null) return;
        setLocalBody({ pubkey: status.providerPubkey, instanceRef });
      } catch {
        // No provider on this computer, or no Tauri host. Both are "nothing
        // here can carry the work", which the panel discloses.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  const [actionError, setActionError] = React.useState<string | null>(null);

  const generations = React.useMemo(
    () => codingSessionHandoverGenerations(executions, resolveReachability),
    [executions, resolveReachability],
  );
  const model = React.useMemo(() => {
    if (read.fold === null) return null;
    return deriveCodingSessionHandoverModel({
      fold: read.fold,
      viewerPubkey,
      founderPubkey,
      operatorPubkeys:
        read.authority?.activeGrants
          .filter((grant) => grant.maySteer)
          .map((grant) => grant.actorPubkey) ?? [],
      generations,
      thisExecutionProviderPubkey,
      retiredAt,
    });
  }, [
    founderPubkey,
    generations,
    read.authority,
    read.fold,
    retiredAt,
    thisExecutionProviderPubkey,
    viewerPubkey,
  ]);

  const runContinue = React.useCallback(async () => {
    // The guard lives here, not only in the button's `disabled`: a second
    // click that arrived before React re-rendered would otherwise publish a
    // second takeover.
    if (busy !== null) return;
    if (model === null || scope === null || viewerPubkey === null) return;
    if (localBody === null) {
      setActionError(
        "this computer has no provider to continue on, so nothing was claimed",
      );
      return;
    }
    const workdirPath = checkoutPath.trim();
    if (workdirPath.length === 0) {
      setActionError(
        "no checkout directory was chosen, so nothing was claimed",
      );
      return;
    }
    const checkpoint = model.latestCheckpoint;
    if (checkpoint === null) {
      setActionError("no authorized checkpoint names work to continue");
      return;
    }
    setBusy("continue");
    setActionError(null);
    try {
      const result = await continueCodingSessionHandover(
        {
          channelId,
          sessionRef: scope.sessionRef,
          genesisRef: scope.genesisRef,
          viewerPubkey,
          // The body being claimed is **this** host's provider authority: the
          // claim names the execution that will actually carry the work, and
          // naming somebody else's would fence a body this host cannot run.
          bodyPubkey: localBody.pubkey,
          providerInstanceRef: providerInstanceRef ?? localBody.instanceRef,
          repoRef: repoRef ?? checkpoint.body.revision.repoRef,
          projectRef,
          title,
          model: null,
          // The prompt is rendered by the flow itself, after the recovery, so
          // it can name the branch and sha that landed and every line this
          // host could not bring across.
          checkpoint: {
            body: checkpoint.body,
            authorLabel:
              resolveName?.(checkpoint.author) ??
              truncatePubkey(checkpoint.author),
          },
          checkpointRef: checkpoint.eventId,
          // The artifacts and their author travel together: the patch this
          // applies must be the checkpoint author's own.
          artifacts: checkpoint.body.artifacts,
          checkpointAuthor: checkpoint.author,
          checkout: checkoutRequestFor(
            checkpoint,
            scope.sessionRef,
            workdirPath,
            relayOrigin,
          ),
          declaredMissing: checkpoint.body.missing,
        },
        { prepareCheckout: invokeHandoverPrepareCheckout },
      );
      if (!result.ok) {
        setActionError(`${result.step}: ${result.reason}`);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(null);
      read.refresh();
    }
  }, [
    busy,
    channelId,
    checkoutPath,
    localBody,
    model,
    projectRef,
    providerInstanceRef,
    read,
    relayOrigin,
    repoRef,
    resolveName,
    scope,
    title,
    viewerPubkey,
  ]);

  const runTakeBack = React.useCallback(async () => {
    if (busy !== null) return;
    if (scope === null || viewerPubkey === null) return;
    if (localBody === null) {
      setActionError(
        "this computer has no provider to take the session back onto",
      );
      return;
    }
    setBusy("take-back");
    setActionError(null);
    try {
      await claimCodingSessionHandover({
        channelId,
        genesisRef: scope.genesisRef,
        claimantPubkey: viewerPubkey,
        bodyPubkey: localBody.pubkey,
      });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(null);
      read.refresh();
    }
  }, [busy, channelId, localBody, read, scope, viewerPubkey]);

  if (scope === null) return null;
  if (read.errorMessage !== null) {
    return (
      <div className="shrink-0 px-4 pt-3">
        <section
          aria-label="Session handover"
          className="rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm"
          data-testid="coding-session-handover"
        >
          <p data-testid="coding-session-handover-error">
            <span className="font-medium">Unread</span> — this session's
            handover records could not be read: {read.errorMessage}
          </p>
        </section>
      </div>
    );
  }
  if (model === null) return null;
  // A reconstruction with no directory would check nothing out and then sign a
  // continuation saying so. The action waits for a directory instead, and says
  // which prerequisite is missing.
  const checkoutChosen = checkoutPath.trim().length > 0;
  const blockedReason =
    localBody === null
      ? "this computer has no provider to continue this session on."
      : checkoutChosen
        ? null
        : "choose a checkout directory on this computer for the work to land in.";
  // A session nobody has handed over, with nothing to continue, is not worth a
  // panel: the surface appears when there is a claim, a fence, a checkpoint
  // worth continuing from, or a deletion to disclose.
  const hasSomethingToSay =
    model.retired ||
    model.claim.state !== "no-claim" ||
    model.continuation !== null ||
    model.viewerMayContinue;
  if (!hasSomethingToSay) return null;

  return (
    <div className="shrink-0 px-4 pt-3">
      <CodingSessionHandoverPanel
        busy={busy}
        errorMessage={actionError}
        model={model}
        capped={read.capped}
        continueBlockedReason={blockedReason}
        onContinue={
          model.viewerMayContinue && blockedReason === null
            ? () => void runContinue()
            : undefined
        }
        onOpenEvidence={onOpenEvidence}
        onTakeBack={
          model.viewerMayTakeBack && localBody !== null
            ? () => void runTakeBack()
            : undefined
        }
        resolveName={resolveName}
        workdirField={
          model.viewerMayContinue ? (
            <NewCodingSessionWorkdirField
              channelId={channelId}
              onChange={setCheckoutPath}
              projectKey={projectRef}
              value={checkoutPath}
            />
          ) : null
        }
      />
    </div>
  );
}

/** What to fetch and apply, read from the checkpoint's own artifacts. */
function checkoutRequestFor(
  checkpoint: NonNullable<
    ReturnType<typeof deriveCodingSessionHandoverModel>["latestCheckpoint"]
  >,
  sessionRef: string,
  workdir: string | null,
  relayOrigin: string | null,
): CodingSessionHandoverCheckoutRequest | null {
  if (workdir === null) return null;
  const wipRef = checkpoint.body.artifacts.find(
    (artifact) => artifact.kind === "wip-ref",
  );
  if (!wipRef) return null;
  return {
    cwd: workdir,
    // The repository the branch lives in, and the relay that serves it: the
    // native command resolves **which remote** from these rather than
    // assuming a name (AGENTS.md § Remotes).
    repoRef: wipRef.repoRef ?? checkpoint.body.revision.repoRef,
    relayOrigin,
    wipRef: wipRef.ref,
    sha: wipRef.sha,
    sessionRef,
    // The patch is resolved by the flow itself, from the checkpoint's own
    // artifacts (`resolveCheckpointPatch`): the artifact is a *pointer* to a
    // NIP-34 event, and reading it needs the author to verify it against.
    patchText: null,
    baseSha: null,
  };
}

/**
 * The relay's own origin — scheme and host — from its websocket URL.
 *
 * `null` rather than a guess when it cannot be parsed: the native command
 * refuses to pick a remote without it, which is the safe end of that.
 */
function relayOriginOf(relayUrl: string | null): string | null {
  if (!relayUrl) return null;
  try {
    const url = new URL(relayUrl);
    const scheme = url.protocol === "ws:" ? "http:" : "https:";
    return `${scheme}//${url.host}`;
  } catch {
    return null;
  }
}

async function invokeHandoverPrepareCheckout(
  request: CodingSessionHandoverCheckoutRequest,
): Promise<CodingSessionHandoverCheckoutReport> {
  return (await invokeTauri(HANDOVER_PREPARE_CHECKOUT_COMMAND, {
    request,
  })) as CodingSessionHandoverCheckoutReport;
}
