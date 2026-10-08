/**
 * SV-29 "Edit from here": the hover action under a prompt, the dialog it
 * opens, and the collapsed row a rewound generation opens with.
 *
 * The dialog offers exactly what the provider would accept (see
 * `resolveCodingSessionRewindAvailability`) and says why anything else is
 * off. Publishing is not the outcome: the provider's signed 44224 is, read
 * through the same pinned trusted ingress a reconnect uses. On success the
 * window follows generation N+1 and the composer there is handed the edited
 * prompt's words; a refusal, `TREE_BUSY` (naming the seat), or a cut that
 * could not reopen ("still remembers") stays in the dialog in the provider's
 * own words.
 *
 * Nothing here deletes a turn. The rewound turns stay in the record, and
 * N+1's `session_rewound` row says how many, who rewound them, and what
 * happened to the files — or, when the signed rewind is not in this view,
 * that it cannot say.
 *
 * Contexts only, no module-level state: nothing here outlives the tree, so
 * `resetCommunityState()` has nothing to reset.
 */
import { useQuery } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { History } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionRewind,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { requestCodingSessionDraftRecovery } from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import { resolveCodingSessionPromptAuthor } from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import { buildCodingSessionResumeInput } from "@/features/coding-sessions/lib/codingSessionResumeSeat";
import { codingSessionResumeSeatDeps } from "@/features/coding-sessions/lib/codingSessionResumeSeatDeps";
import {
  type CodingSessionRewindAvailability,
  type CodingSessionRewindChoice,
  type CodingSessionRewindFiles,
  type CodingSessionRewindOutcome,
  type CodingSessionRewindRecord,
  codingSessionRewindPrefill,
  countCodingSessionRewoundTurns,
  findCodingSessionRewindReceipt,
  foldCodingSessionRewindOutcome,
  joinCodingSessionRewindRecord,
  resolveCodingSessionRewindAvailability,
} from "@/features/coding-sessions/lib/codingSessionRewind";
import {
  CODING_SESSION_REWIND_ACTION_LABEL,
  CODING_SESSION_REWIND_CHOICES,
  CODING_SESSION_REWIND_DETACHED_NOTE,
  CODING_SESSION_REWIND_DIALOG_BODY,
  codingSessionRewindBlockText,
  codingSessionRewindFilesText,
  codingSessionRewindOutcomeRows,
  codingSessionRewoundRowLabel,
} from "@/features/coding-sessions/lib/codingSessionRewindRows";
import { publishSeatedCodingSessionResume } from "@/features/coding-sessions/lib/codingSessionSeatedCreate";
import { buildCodingSessionTranscriptGenerationId } from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import { establishedCodingSessionTarget } from "@/features/coding-sessions/lib/codingSessionTrustedIngress";
import type { CodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { resolveCodingSessionUmbrellaComposerAuthority } from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { parseCodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { NEW_CODING_SESSION_STALL_MS } from "@/features/coding-sessions/lib/newCodingSessionModel";
import {
  type CodingSessionIngressClient,
  useCodingSessionLifecycleResolution,
} from "@/features/coding-sessions/lib/useTrustedCodingSessionIngress";
import { relayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
} from "@/shared/constants/kinds";
import { verifyEventSignatures } from "@/shared/lib/authors";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { useCodingSessionTurnCheckpoint } from "./CodingSessionCheckpointsContext";
import { useCodingSessionDisclosure } from "./CodingSessionTranscriptDisclosure";
import {
  CodingSessionPromptAttributionContext,
  CodingSessionTranscriptGenerationContext,
} from "./CodingSessionTranscriptItem";
import {
  type CodingSessionSurfaceCtx,
  useCodingSessionSurfaceCtx,
} from "./surfaces/codingSessionSurfaceContext";

/**
 * What the workspace knows that the surface context does not: the live
 * collaborator grants (`acceptedOperators`, `null` while the roster loads).
 * Absent, only the founder (or anyone, on an ungoverned session) may rewind —
 * the composer's founder-only fallback, never a widened one.
 */
export const CodingSessionRewindScopeContext = React.createContext<{
  acceptedOperators: ReadonlySet<string> | null;
  /** Ingress transport seam; production passes nothing. */
  client?: CodingSessionIngressClient;
} | null>(null);

/**
 * The workspace's grants, handed to every "Edit from here" below it — the
 * same live operator set the composer's authority reads, so a granted
 * collaborator may rewind exactly when they may restart.
 */
export function CodingSessionRewindScope({
  acceptedOperators,
  children,
}: {
  acceptedOperators: ReadonlySet<string> | null;
  children: React.ReactNode;
}) {
  const value = React.useMemo(
    () => ({ acceptedOperators }),
    [acceptedOperators],
  );
  return (
    <CodingSessionRewindScopeContext.Provider value={value}>
      {children}
    </CodingSessionRewindScopeContext.Provider>
  );
}

type RewindScope = {
  channelId: string;
  /** The generation the prompt was sent to. */
  record: CodingSessionCatalogRecord;
  /** The execution's current generation, which a rewind must address. */
  current: CodingSessionCatalogRecord;
  /** All of the execution's generations this view holds, oldest first. */
  generations: readonly CodingSessionCatalogRecord[];
  mayControl: boolean | null;
  sessionClosed: boolean;
};

function resolveRewindScope(
  ctx: CodingSessionSurfaceCtx | null,
  generationId: string,
  acceptedOperators: ReadonlySet<string> | null,
): RewindScope | null {
  if (ctx === null || generationId === "") return null;
  for (const { execution } of ctx.executions) {
    const generations = [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ];
    const record = generations.find(
      (entry) => entry.generationId === generationId,
    );
    if (!record) continue;
    const authority = resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: ctx.umbrella,
      currentUserPubkey: ctx.currentUserPubkey,
      acceptedOperators,
    });
    return {
      channelId: ctx.channelId,
      record,
      current: execution.activeGeneration,
      generations,
      mayControl: authority.isUnresolved ? null : authority.canPromptExecutions,
      sessionClosed: ctx.sessionClosed,
    };
  }
  return null;
}

function useRewindScope(): RewindScope | null {
  const generationId = React.useContext(
    CodingSessionTranscriptGenerationContext,
  );
  const ctx = useCodingSessionSurfaceCtx();
  const scope = React.useContext(CodingSessionRewindScopeContext);
  const acceptedOperators = scope?.acceptedOperators ?? null;
  return React.useMemo(
    () => resolveRewindScope(ctx, generationId, acceptedOperators),
    [acceptedOperators, ctx, generationId],
  );
}

type PromptItem = Extract<TranscriptItem, { type: "message" }>;

/**
 * "Edit from here", beside a prompt's time and copy. Renders nothing outside
 * a coding-session workspace, and nothing for a steered prompt: that one
 * joined a turn already running, so there is no turn of its own to cut at.
 */
export function CodingSessionRewindAction({ item }: { item: PromptItem }) {
  const scope = useRewindScope();
  const generationId = React.useContext(
    CodingSessionTranscriptGenerationContext,
  );
  const checkpoint = useCodingSessionTurnCheckpoint(
    generationId,
    item.turnId ?? "",
  );
  const [open, setOpen] = React.useState(false);
  if (scope === null || item.steered === true || item.role !== "user") {
    return null;
  }
  const availability = resolveCodingSessionRewindAvailability({
    checkpoint: item.turnId ? checkpoint : null,
    turnRunning: scope.current.status === "running",
    mayControl: scope.mayControl,
    sessionClosed: scope.sessionClosed,
    inCurrentGeneration:
      scope.record.generationId === scope.current.generationId,
  });
  return (
    <>
      <button
        aria-label={CODING_SESSION_REWIND_ACTION_LABEL}
        className="inline-flex size-6 items-center justify-center rounded-md transition-colors hover:bg-accent/30 hover:text-foreground"
        data-testid="coding-session-user-message-rewind"
        onClick={() => setOpen(true)}
        title={CODING_SESSION_REWIND_ACTION_LABEL}
        type="button"
      >
        <History aria-hidden className="size-3" />
      </button>
      {open ? (
        <CodingSessionRewindDialog
          availability={availability}
          onOpenChange={setOpen}
          promptText={item.text}
          scope={scope}
        />
      ) : null}
    </>
  );
}

function CodingSessionRewindDialog({
  availability,
  onOpenChange,
  promptText,
  scope,
}: {
  availability: CodingSessionRewindAvailability;
  onOpenChange: (open: boolean) => void;
  promptText: string;
  scope: RewindScope;
}) {
  const [command, setCommand] = React.useState<{
    id: string;
    files: CodingSessionRewindFiles;
  } | null>(null);
  const [publishError, setPublishError] = React.useState<string | null>(null);
  const [outcome, setOutcome] = React.useState<CodingSessionRewindOutcome>({
    kind: "pending",
  });
  const target = scope.current.commandTarget;
  const provider = scope.current.providerAuthorityPubkey;
  const settled = outcome.kind !== "pending";
  const inFlight = command !== null && !settled && publishError === null;

  const choose = (files: CodingSessionRewindFiles) => {
    if (!target || !provider || !availability.checkpointEventId || inFlight) {
      return;
    }
    const checkpoint = availability.checkpointEventId;
    const commandId = createCodingSessionLifecycleCommandId();
    setPublishError(null);
    setOutcome({ kind: "pending" });
    setCommand({ id: commandId, files });
    // A seated execution restages its seat under the rewind's command id,
    // exactly as a restart does: the new generation is minted under it.
    void publishSeatedCodingSessionResume(
      buildCodingSessionResumeInput({
        commandId,
        seat: {
          actorPubkey: scope.current.agentRef,
          role: scope.current.role,
          projectRef: scope.current.projectRef,
          sessionId: target.sessionId,
        },
        deps: codingSessionResumeSeatDeps,
        publish: () =>
          publishCodingSessionRewind({
            channelId: scope.channelId,
            commandId,
            target,
            providerAuthorityPubkey: provider,
            checkpoint,
            files,
          }),
      }),
    ).catch((error: unknown) => {
      setPublishError(
        error instanceof Error && error.message.trim()
          ? error.message
          : "The rewind could not be published.",
      );
    });
  };

  const rows =
    publishError !== null
      ? { tone: "warning" as const, lines: [publishError] }
      : command !== null
        ? codingSessionRewindOutcomeRows(outcome)
        : null;

  return (
    <Dialog onOpenChange={onOpenChange} open>
      <DialogContent data-testid="coding-session-rewind-dialog">
        <DialogHeader>
          <DialogTitle>{CODING_SESSION_REWIND_ACTION_LABEL}</DialogTitle>
          <DialogDescription>
            {CODING_SESSION_REWIND_DIALOG_BODY}
          </DialogDescription>
        </DialogHeader>
        <p
          className="line-clamp-3 rounded-md bg-muted px-3 py-2 text-sm text-foreground"
          data-testid="coding-session-rewind-prompt"
        >
          {promptText.trim() || "Prompt with no text"}
        </p>
        <div className="flex flex-col gap-2">
          {(["keep", "restore"] as const).map((files) => (
            <RewindChoice
              choice={files === "keep" ? availability.chat : availability.files}
              files={files}
              inFlight={inFlight}
              key={files}
              onChoose={choose}
              selected={command?.files === files}
            />
          ))}
        </div>
        <p className="text-xs text-muted-foreground">
          {CODING_SESSION_REWIND_DETACHED_NOTE}
        </p>
        {rows ? (
          <div
            className={cn(
              "flex flex-col gap-0.5 rounded-md border px-3 py-2 text-sm",
              rows.tone === "warning"
                ? "border-amber-500/40 text-amber-800 dark:text-amber-200"
                : "border-border text-muted-foreground",
            )}
            data-outcome={publishError !== null ? "unpublished" : outcome.kind}
            data-testid="coding-session-rewind-outcome"
            role="status"
          >
            {rows.lines.map((line) => (
              <span key={line}>{line}</span>
            ))}
          </div>
        ) : null}
        <DialogFooter>
          <Button
            onClick={() => onOpenChange(false)}
            type="button"
            variant="ghost"
          >
            {settled ? "Close" : "Cancel"}
          </Button>
        </DialogFooter>
        {command !== null && provider && publishError === null ? (
          <RewindWatcher
            channelId={scope.channelId}
            commandId={command.id}
            key={command.id}
            onClose={() => onOpenChange(false)}
            onOutcome={setOutcome}
            promptText={promptText}
            providerAuthorityPubkey={provider}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function RewindChoice({
  choice,
  files,
  inFlight,
  onChoose,
  selected,
}: {
  choice: CodingSessionRewindChoice;
  files: CodingSessionRewindFiles;
  inFlight: boolean;
  onChoose: (files: CodingSessionRewindFiles) => void;
  selected: boolean;
}) {
  const copy = CODING_SESSION_REWIND_CHOICES[files];
  return (
    <button
      aria-disabled={!choice.enabled || inFlight}
      className={cn(
        "flex flex-col items-start gap-0.5 rounded-lg border px-3 py-2 text-left transition-colors",
        choice.enabled
          ? "border-border hover:bg-accent/30"
          : "cursor-not-allowed border-border/60 opacity-70",
        selected && "border-primary",
      )}
      data-enabled={choice.enabled ? "true" : "false"}
      data-testid={`coding-session-rewind-choice-${files}`}
      disabled={!choice.enabled || inFlight}
      onClick={() => onChoose(files)}
      type="button"
    >
      <span className="text-sm font-medium text-foreground">{copy.label}</span>
      <span className="text-xs text-muted-foreground">{copy.detail}</span>
      {choice.enabled ? null : (
        <span
          className="text-xs text-amber-800 dark:text-amber-200"
          data-testid={`coding-session-rewind-choice-${files}-reason`}
        >
          {codingSessionRewindBlockText(choice.block)}
        </span>
      )}
    </button>
  );
}

/**
 * One published rewind's answer, watched. Mounted only while one is in
 * flight, keyed by command id, exactly like the reconnect watcher.
 */
function RewindWatcher({
  channelId,
  commandId,
  onClose,
  onOutcome,
  promptText,
  providerAuthorityPubkey,
}: {
  channelId: string;
  commandId: string;
  onClose: () => void;
  onOutcome: (outcome: CodingSessionRewindOutcome) => void;
  promptText: string;
  providerAuthorityPubkey: string;
}) {
  const router = useRouter({ warn: false });
  const client = React.useContext(CodingSessionRewindScopeContext)?.client;
  // Pinned: the answer comes from the provider the command names, which on a
  // session someone else founded is not one this machine may run.
  const snapshot = useCodingSessionLifecycleResolution(
    channelId,
    commandId,
    providerAuthorityPubkey,
    client,
    "pinned",
  );
  const { lifecycle, retainedRawEvents } = snapshot;
  const outcome = React.useMemo(() => {
    const target = lifecycle && "target" in lifecycle ? lifecycle.target : null;
    const rewind = target
      ? findCodingSessionRewindReceipt(
          retainedRawEvents({
            channelId,
            signerPubkey: providerAuthorityPubkey,
            targetKey: buildCodingSessionTargetKey(target),
          }),
          commandId,
          providerAuthorityPubkey,
        )
      : null;
    return foldCodingSessionRewindOutcome({
      lifecycle,
      rewind,
      failedRewind:
        lifecycle?.state === "failed" ? (lifecycle.rewind ?? null) : null,
    });
  }, [
    channelId,
    commandId,
    lifecycle,
    providerAuthorityPubkey,
    retainedRawEvents,
  ]);

  React.useEffect(() => onOutcome(outcome), [onOutcome, outcome]);

  // Follow N+1 once it is established (its metadata is read), handing its
  // composer the edited prompt's words. Once only: a replayed receipt is the
  // same fact, not a second rewind.
  const followed = React.useRef(false);
  const established = establishedCodingSessionTarget(lifecycle);
  React.useEffect(() => {
    if (followed.current || !established || outcome.kind !== "rewound") return;
    followed.current = true;
    requestCodingSessionDraftRecovery(
      codingSessionRewindPrefill({
        commandId,
        channelId,
        target: established,
        text: promptText,
      }),
    );
    toast.success(
      outcome.memory === "seeded"
        ? "Rewound — the new conversation is seeded from the record"
        : "Rewound, but restarted with no memory",
    );
    onClose();
    void router?.navigate({
      to: "/coding-sessions/$channelId/$generationId",
      params: {
        channelId,
        generationId: buildCodingSessionTranscriptGenerationId(
          channelId,
          providerAuthorityPubkey,
          established,
        ),
      },
      search: (previous) => ({
        surface: parseCodingSessionSurface(previous.surface),
      }),
      replace: true,
    });
  }, [
    channelId,
    commandId,
    established,
    onClose,
    outcome,
    promptText,
    providerAuthorityPubkey,
    router,
  ]);

  // The resolution never times out by design; patience lives here.
  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      if (followed.current) return;
      toast.warning(
        "The provider has not answered this rewind yet. It may no longer be running.",
      );
    }, NEW_CODING_SESSION_STALL_MS);
    return () => window.clearTimeout(timer);
  }, []);

  return null;
}

/** How many of the newest commands and receipts the row looks through. */
const REWIND_RECORD_COMMAND_LIMIT = 200;
const REWIND_RECORD_RECEIPT_LIMIT = 500;

async function verified(events: RelayEvent[]): Promise<RelayEvent[]> {
  const ok = await verifyEventSignatures(events);
  return events.filter((_, index) => ok[index] === true);
}

/**
 * The signed rewind that minted `record`'s generation, joined strictly
 * (`joinCodingSessionRewindRecord`), or null when it is not in the newest
 * commands and receipts this reads — said as "not read", never as "none".
 */
function useCodingSessionRewindRecord(
  channelId: string,
  record: CodingSessionCatalogRecord,
): { state: "loading" | "read"; value: CodingSessionRewindRecord | null } {
  const target = record.commandTarget;
  const provider = record.providerAuthorityPubkey;
  const query = useQuery({
    queryKey: [
      "coding-session-rewind-record",
      channelId,
      provider,
      target ? buildCodingSessionTargetKey(target) : null,
    ],
    enabled: target !== null && provider !== null,
    staleTime: 60_000,
    queryFn: async () => {
      if (!target || !provider) return null;
      const [commands, receipts] = await Promise.all([
        relayClient.fetchEvents({
          kinds: [KIND_CODING_SESSION_LIFECYCLE_COMMAND],
          "#h": [channelId],
          limit: REWIND_RECORD_COMMAND_LIMIT,
        }),
        relayClient.fetchEvents({
          kinds: [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
          authors: [provider],
          "#h": [channelId],
          limit: REWIND_RECORD_RECEIPT_LIMIT,
        }),
      ]);
      return joinCodingSessionRewindRecord({
        channelId,
        providerAuthorityPubkey: provider,
        target,
        commands: await verified(commands),
        receipts: await verified(receipts),
      });
    },
  });
  return {
    state: query.isPending && query.fetchStatus !== "idle" ? "loading" : "read",
    value: query.data ?? null,
  };
}

/**
 * N+1's opening row: one collapsed line — "3 turns rewound by Brian · files
 * restored" — over the provider's signed row and what the files and HEAD did.
 * The rewound turns themselves stay where they are in the record.
 *
 * The row's own text (files, memory) is the provider's signed `session_rewound`
 * item; who rewound and how many turns come from the joined 44221 + 44224.
 * When those are not in this view the row says so instead of guessing.
 */
export function CodingSessionRewoundRow({ item }: { item: TranscriptItem }) {
  const scope = useRewindScope();
  const attribution = React.useContext(CodingSessionPromptAttributionContext);
  const [open, setOpen] = useCodingSessionDisclosure(`rewound:${item.id}`);
  const text = item.type === "lifecycle" ? item.text : "";
  if (scope === null) return <RewoundRowShell text={text} />;
  return (
    <RewoundRowWithRecord
      attribution={attribution}
      onToggle={() => setOpen(!open)}
      open={open}
      scope={scope}
      text={text}
    />
  );
}

function RewoundRowWithRecord({
  attribution,
  onToggle,
  open,
  scope,
  text,
}: {
  attribution: React.ContextType<typeof CodingSessionPromptAttributionContext>;
  onToggle: () => void;
  open: boolean;
  scope: RewindScope;
  text: string;
}) {
  const read = useCodingSessionRewindRecord(scope.channelId, scope.record);
  const value = read.value;
  const count = React.useMemo(
    () =>
      value === null
        ? null
        : countCodingSessionRewoundTurns({
            rewind: value.rewind,
            generations: scope.generations.flatMap((record) =>
              record.commandTarget
                ? [
                    {
                      generation: record.commandTarget.generation,
                      items: record.transcript,
                    },
                  ]
                : [],
            ),
          }),
    [scope.generations, value],
  );
  if (value === null) {
    return (
      <RewoundRowShell
        note={
          read.state === "loading"
            ? "Reading the signed rewind…"
            : "The signed rewind command was not found in this view, so who rewound and how many turns are not shown."
        }
        text={text}
      />
    );
  }
  const signer = resolveCodingSessionPromptAuthor({
    currentUserPubkey: attribution.currentUserPubkey,
    operatorPubkey: value.signerPubkey,
    profiles: attribution.profiles,
    resolveSeat: attribution.resolveSeat,
  }).label;
  const label = codingSessionRewoundRowLabel({
    count,
    signer,
    rewind: value.rewind,
    memory: value.memory,
  });
  return (
    <div
      className="rounded-lg border border-border/70 px-3 py-2 text-sm text-muted-foreground"
      data-files={value.rewind.files}
      data-memory={value.memory}
      data-testid="coding-session-rewound-row"
    >
      <button
        aria-expanded={open}
        className="flex w-full items-center gap-2 text-left hover:text-foreground"
        data-testid="coding-session-rewound-row-toggle"
        onClick={onToggle}
        type="button"
      >
        <History aria-hidden className="size-3.5 shrink-0" />
        <span className="font-medium text-foreground/80">{label}</span>
      </button>
      {open ? (
        <div
          className="mt-1.5 flex flex-col gap-0.5 ps-5 text-xs"
          data-testid="coding-session-rewound-row-details"
        >
          {text ? <span>{text}</span> : null}
          <span>
            {codingSessionRewindFilesText(
              value.rewind.files,
              value.rewind.head,
            )}
          </span>
          <span>
            The rewound turns stay in the record.{" "}
            {CODING_SESSION_REWIND_DETACHED_NOTE}
          </span>
        </div>
      ) : null}
    </div>
  );
}

function RewoundRowShell({ note, text }: { note?: string; text: string }) {
  return (
    <div
      className="flex flex-col gap-0.5 rounded-lg border border-border/70 px-3 py-2 text-sm text-muted-foreground"
      data-testid="coding-session-rewound-row"
    >
      <span className="flex items-center gap-2 font-medium text-foreground/80">
        <History aria-hidden className="size-3.5 shrink-0" />
        {text}
      </span>
      {note ? <span className="ps-5 text-xs">{note}</span> : null}
    </div>
  );
}
