import * as React from "react";
import { LoaderCircle, Wrench } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  deriveCodingSessionTranscriptModel,
  stabilizeCodingSessionTranscriptModel,
  type CodingSessionTranscriptModel,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { useTextSettledCodingSessionEchoes } from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import type {
  CodingSessionHireDispatchVouch,
  CodingSessionPromptSeatResolver,
} from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import type { CodingSessionWakeOperationIndex } from "@/features/coding-sessions/lib/codingSessionWakeReading";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { RedactionDictionaryContext } from "@/shared/ui/redactionDictionary";
import { VirtualizedList } from "@/shared/ui/VirtualizedList";
import { useRedactionDictionary } from "../useRedactionDictionary";
import {
  CodingSessionDisclosureContext,
  type CodingSessionDisclosureStore,
  createCodingSessionDisclosureStore,
  useCodingSessionDisclosureProps,
} from "./CodingSessionTranscriptDisclosure";
import {
  CodingSessionPromptAttributionContext,
  CodingSessionTranscriptGenerationContext,
  NO_WAKE_OPERATIONS,
} from "./CodingSessionTranscriptItem";
import { CodingSessionDiagnostics } from "./CodingSessionTranscriptParts";
import {
  CodingSessionEntry,
  CodingSessionTurn,
} from "./CodingSessionTranscriptTurn";

export {
  createCodingSessionDisclosureStore,
  type CodingSessionDisclosureStore,
} from "./CodingSessionTranscriptDisclosure";

type CodingSessionTranscriptProps = {
  generationId: string;
  isWorking: boolean;
  items: TranscriptItem[];
  scrollRef?: React.RefObject<HTMLElement | null>;
  /**
   * The viewer's own pubkey, used to decide whether a stamped user message is
   * theirs. Omitted or `null` means the identity is not known yet, in which
   * case attributed prompts are labelled with the operator rather than `"You"`.
   */
  currentUserPubkey?: string | null;
  /**
   * Profiles for the operators appearing in `items`, resolved by the caller.
   *
   * Resolution is the caller's job because it needs the community-scoped batch
   * profile query, and this component is rendered once per turn block on the
   * umbrella surface — hoisting it keeps that to one lookup per surface and
   * keeps this renderer free of data dependencies. Unresolved operators fall
   * back to a truncated pubkey, which is always true.
   */
  operatorProfiles?: UserProfileLookup;
  /**
   * Names a seat from its actor pubkey. Mission supplies it so a turn one
   * seat sent to another is attributed to that seat; the one-seat lens has
   * no second seat to attribute to and passes nothing.
   */
  resolveSeat?: CodingSessionPromptSeatResolver;
  /**
   * Signed evidence that this execution is a seat this computer hired.
   * Supplied by the umbrella, which knows the execution's `agentRef` and its
   * own `sessionRef` and can join both to this host's hire record.
   */
  hireDispatch?: CodingSessionHireDispatchVouch | null;
  /**
   * Fold-resolved operations, so an identifier-only team wake renders as the
   * one line §1f freezes instead of as its own pointer JSON (finding 17).
   *
   * Absent is the honest empty index, not a licence to guess: a surface that
   * holds no fold still replaces the JSON, with the unresolved line naming the
   * operation it could not resolve.
   */
  wakeOperations?: CodingSessionWakeOperationIndex;
  /**
   * Where open/closed state for this transcript's disclosures lives. Omit it
   * and the transcript keeps its own for as long as it is mounted; pass one
   * (from `createCodingSessionDisclosureStore`) to keep that state across a
   * remount of the transcript itself, e.g. when a surface virtualizes whole
   * transcripts. Read once, at mount.
   */
  disclosureStore?: CodingSessionDisclosureStore;
};

export const CODING_SESSION_VIRTUALIZATION_THRESHOLD = 40;

type CodingSessionTranscriptRow =
  | {
      kind: "block";
      block: CodingSessionTranscriptModel["blocks"][number];
      key: string;
    }
  | {
      kind: "diagnostics";
      diagnostics: TranscriptItem[];
      key: "session-diagnostics";
    };

export function CodingSessionTranscript({
  currentUserPubkey,
  disclosureStore,
  hireDispatch,
  operatorProfiles,
  generationId,
  isWorking,
  items,
  resolveSeat,
  scrollRef,
  wakeOperations,
}: CodingSessionTranscriptProps) {
  const model = useStableCodingSessionTranscriptModel(items, isWorking);
  // Only ever non-empty on the machine whose provider signed these items; see
  // `useRedactionDictionary`. A context rather than a prop because the pill
  // that reads it is produced inside cached markdown element trees.
  const redactions = useRedactionDictionary(items);
  const textSettledEchoIds = useTextSettledCodingSessionEchoes();
  const promptAttribution = React.useMemo(
    () => ({
      currentUserPubkey: currentUserPubkey ?? null,
      profiles: operatorProfiles,
      resolveSeat,
      textSettledEchoIds,
      hireDispatch: hireDispatch ?? null,
      wakeOperations: wakeOperations ?? NO_WAKE_OPERATIONS,
    }),
    [
      currentUserPubkey,
      hireDispatch,
      operatorProfiles,
      resolveSeat,
      textSettledEchoIds,
      wakeOperations,
    ],
  );
  // One store for the transcript's lifetime: every row reads its own id from
  // it, so opening one disclosure re-renders that disclosure alone, and a
  // row the virtualizer unmounts comes back as it was left.
  const [ownStore] = React.useState(
    () => disclosureStore ?? createCodingSessionDisclosureStore(),
  );
  const rows = React.useMemo(
    () => buildCodingSessionTranscriptRows(model),
    [model],
  );
  const averageEstimatedRowSize = React.useMemo(
    () =>
      rows.length === 0
        ? 160
        : Math.round(
            rows.reduce(
              (total, row) =>
                total + estimateCodingSessionTranscriptRowSize(row),
              0,
            ) / rows.length,
          ),
    [rows],
  );

  if (rows.length === 0) {
    return (
      <div
        className="flex min-h-48 flex-col items-center justify-center text-center"
        data-testid="coding-session-transcript-empty"
      >
        {isWorking ? (
          <LoaderCircle className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" />
        ) : (
          <Wrench className="size-4 text-muted-foreground" />
        )}
        <p className="mt-3 text-sm font-medium">
          {isWorking ? "Session is working" : "No conversation yet"}
        </p>
        <p className="mt-1 text-sm text-muted-foreground">
          {/* True whatever the provider's flush cadence: it publishes prose
              in pieces (a paragraph, a tool boundary, the end of the turn),
              and each piece appears when it is published. */}
          {isWorking
            ? "The agent's work appears here as the provider publishes it."
            : "Send a prompt to begin this coding session."}
        </p>
      </div>
    );
  }

  const shouldVirtualize =
    rows.length > CODING_SESSION_VIRTUALIZATION_THRESHOLD && scrollRef;

  return (
    <RedactionDictionaryContext.Provider value={redactions}>
      <CodingSessionPromptAttributionContext.Provider value={promptAttribution}>
        <CodingSessionTranscriptGenerationContext.Provider value={generationId}>
          <CodingSessionDisclosureContext.Provider value={ownStore}>
            <div
              aria-label="Live coding-session conversation"
              aria-live="off"
              data-transcript-renderer={
                shouldVirtualize ? "virtualized" : "static"
              }
              data-testid="coding-session-transcript"
              role="log"
            >
              {shouldVirtualize ? (
                <VirtualizedList
                  estimateSize={averageEstimatedRowSize}
                  getItemKey={getCodingSessionTranscriptRowKey}
                  innerClassName="w-full"
                  items={rows}
                  overscan={6}
                  renderItem={renderCodingSessionTranscriptVirtualRow}
                  scrollRef={scrollRef}
                />
              ) : (
                <div className="flex flex-col gap-5">
                  {rows.map((row) => (
                    <CodingSessionTranscriptRowContent
                      key={row.key}
                      row={row}
                    />
                  ))}
                </div>
              )}
              <span
                aria-atomic="true"
                aria-live="polite"
                className="sr-only"
                data-testid="coding-session-live-status"
                role="status"
              >
                {isWorking ? "Coding session working" : "Coding session idle"}
              </span>
            </div>
          </CodingSessionDisclosureContext.Provider>
        </CodingSessionTranscriptGenerationContext.Provider>
      </CodingSessionPromptAttributionContext.Provider>
    </RedactionDictionaryContext.Provider>
  );
}

/**
 * The virtualizer's row renderer. Module-level so it never changes: every
 * input a row needs travels by context, and each row is memoized on its own
 * block.
 */
function renderCodingSessionTranscriptVirtualRow(
  row: CodingSessionTranscriptRow,
): React.ReactNode {
  return (
    <div className="pb-5">
      <CodingSessionTranscriptRowContent row={row} />
    </div>
  );
}

export function buildCodingSessionTranscriptRows(
  model: CodingSessionTranscriptModel,
): CodingSessionTranscriptRow[] {
  const rows: CodingSessionTranscriptRow[] = model.blocks.map((block) => ({
    kind: "block",
    block,
    key: block.kind === "turn" ? `turn:${block.id}` : `item:${block.id}`,
  }));
  if (model.diagnostics.length > 0) {
    rows.push({
      kind: "diagnostics",
      diagnostics: model.diagnostics,
      key: "session-diagnostics",
    });
  }
  return rows;
}

export function getCodingSessionTranscriptRowKey(
  row: CodingSessionTranscriptRow,
): string {
  return row.key;
}

export function estimateCodingSessionTranscriptRowSize(
  row: CodingSessionTranscriptRow,
): number {
  if (row.kind === "diagnostics") return 48;
  if (row.block.kind === "standalone") return 96;
  const turn = row.block;
  // A settled turn is estimated as it first renders: folded.
  const fold = turn.fold;
  const hidden = new Set(fold?.hiddenIndexes ?? []);
  const entryEstimate = turn.entries.reduce((height, entry, index) => {
    if (hidden.has(index)) return height;
    if (entry.kind !== "item") return height + 28;
    if (entry.item.type === "message") {
      return height + Math.min(320, 52 + entry.item.text.length / 3);
    }
    return height + 32;
  }, 0);
  const stateEstimate =
    (fold ? 28 : 0) +
    (turn.isWorking ? 28 : 0) +
    (turn.completion ? 24 : 0) +
    (turn.changedFiles.length > 0 ? 48 : 0);
  return Math.max(88, Math.min(720, entryEstimate + stateEstimate + 20));
}

/**
 * One row of the transcript. Rows are rebuilt as objects whenever the model
 * changes, so the memo compares what a row *shows* — its block, which the
 * model keeps identical while unchanged — rather than the row wrapper.
 */
const CodingSessionTranscriptRowContent = React.memo(
  function CodingSessionTranscriptRowContent({
    row,
  }: {
    row: CodingSessionTranscriptRow;
  }) {
    if (row.kind === "diagnostics") {
      return <CodingSessionSessionDiagnostics diagnostics={row.diagnostics} />;
    }
    if (row.block.kind === "turn") {
      return <CodingSessionTurn turn={row.block} />;
    }
    return (
      <div
        className="content-visibility-auto"
        data-message-id={row.block.id}
        data-testid="coding-session-standalone"
      >
        <CodingSessionEntry entry={row.block.entry} />
      </div>
    );
  },
  (previous, next) =>
    codingSessionTranscriptRowsShowSame(previous.row, next.row),
);

function codingSessionTranscriptRowsShowSame(
  left: CodingSessionTranscriptRow,
  right: CodingSessionTranscriptRow,
): boolean {
  if (left.kind === "block" && right.kind === "block") {
    return left.block === right.block;
  }
  if (left.kind === "diagnostics" && right.kind === "diagnostics") {
    return left.diagnostics === right.diagnostics;
  }
  return false;
}

function CodingSessionSessionDiagnostics({
  diagnostics,
}: {
  diagnostics: TranscriptItem[];
}) {
  const disclosure = useCodingSessionDisclosureProps("session:diagnostics");
  return (
    <CodingSessionDiagnostics
      {...disclosure}
      diagnostics={diagnostics}
      label="Session details"
    />
  );
}

function useStableCodingSessionTranscriptModel(
  items: TranscriptItem[],
  isWorking: boolean,
): CodingSessionTranscriptModel {
  const previousRef = React.useRef<CodingSessionTranscriptModel | null>(null);
  const next = React.useMemo(
    () => deriveCodingSessionTranscriptModel(items, { isWorking }),
    [isWorking, items],
  );
  const stable = stabilizeCodingSessionTranscriptModel(
    previousRef.current,
    next,
  );
  previousRef.current = stable;
  return stable;
}
