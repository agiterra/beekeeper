import * as React from "react";
import { LoaderCircle, Wrench } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  deriveCodingSessionTranscriptModel,
  stabilizeCodingSessionTranscriptModel,
  type CodingSessionTranscriptModel,
  type CodingSessionTurnRestingStatus,
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
  CODING_SESSION_TURN_GAP,
  CODING_SESSION_VIRTUAL_ROW_PAD_CLASS,
  codingSessionEntryRowKind,
  codingSessionRowGap,
  codingSessionTurnTailGap,
  type CodingSessionRowKind,
} from "./CodingSessionTranscriptRhythm";
import { CodingSessionOpenAgentsSurfaceContext } from "./CodingSessionTranscriptAgentsSurface";
import { shouldClampCodingSessionUserMessage } from "./CodingSessionTranscriptUserMessage";
import {
  CodingSessionEntry,
  CodingSessionTranscriptTurnPolicyContext,
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
  /**
   * Opens the workspace's Agents surface (SV-06). When given, a subagent row
   * opens it; omitted, the row expands inline instead.
   */
  onOpenAgentsSurface?: () => void;
  /**
   * The model of exactly these `items` at this `isWorking`, when the caller
   * already derived it with {@link useStableCodingSessionTranscriptModel} —
   * the single workspace does, to read `sessionFacts` for Details and the
   * sandbox chip without a second pass over the transcript per streamed item.
   * Omitted, the transcript derives its own.
   */
  model?: CodingSessionTranscriptModel;
  /**
   * What the caller knows about the session when its latest turn has no
   * completion and is not the one being worked on: `running`, `stopped`
   * (idle, ended — an unended call there "Did not finish"), or `unknown`.
   * Omitted is `unknown`, so an unended call reads "Status unknown" until a
   * caller that holds the status vouches for more. A turn that reported a
   * completion or was followed by another is settled regardless.
   */
  restingStatus?: CodingSessionTurnRestingStatus;
  /**
   * `false` for a transcript that is a fragment of a turn whose working line
   * is already on screen (Mission Live's execution bundle): no second working
   * line, and no second live announcement. Defaults to `true`.
   */
  showWorkingIndicator?: boolean;
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
  model: sharedModel,
  onOpenAgentsSurface,
  resolveSeat,
  restingStatus = "unknown",
  scrollRef,
  showWorkingIndicator = true,
  wakeOperations,
}: CodingSessionTranscriptProps) {
  const ownModel = useStableCodingSessionTranscriptModel(
    sharedModel ? null : items,
    isWorking,
  );
  // One of the two is always set: `ownModel` is null only when shared.
  const model = (sharedModel ?? ownModel) as CodingSessionTranscriptModel;
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
  // One stable opener for the transcript's lifetime, calling whatever the
  // caller passed last, so an inline arrow there re-renders no row.
  const openAgentsSurfaceRef = React.useRef(onOpenAgentsSurface);
  openAgentsSurfaceRef.current = onOpenAgentsSurface;
  const hasAgentsSurface = onOpenAgentsSurface !== undefined;
  const openAgentsSurface = React.useMemo(
    () => (hasAgentsSurface ? () => openAgentsSurfaceRef.current?.() : null),
    [hasAgentsSurface],
  );
  const turnPolicy = React.useMemo(
    () => ({ restingStatus, showWorkingRow: showWorkingIndicator }),
    [restingStatus, showWorkingIndicator],
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
            <CodingSessionOpenAgentsSurfaceContext.Provider
              value={openAgentsSurface}
            >
              <CodingSessionTranscriptTurnPolicyContext.Provider
                value={turnPolicy}
              >
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
                    <div
                      className={`flex flex-col ${CODING_SESSION_TURN_GAP.className}`}
                    >
                      {rows.map((row) => (
                        <CodingSessionTranscriptRowContent
                          key={row.key}
                          row={row}
                        />
                      ))}
                    </div>
                  )}
                  {showWorkingIndicator ? (
                    <span
                      aria-atomic="true"
                      aria-live="polite"
                      className="sr-only"
                      data-testid="coding-session-live-status"
                      role="status"
                    >
                      {codingSessionLiveStatusText(isWorking, restingStatus)}
                    </span>
                  ) : null}
                </div>
              </CodingSessionTranscriptTurnPolicyContext.Provider>
            </CodingSessionOpenAgentsSurfaceContext.Provider>
          </CodingSessionDisclosureContext.Provider>
        </CodingSessionTranscriptGenerationContext.Provider>
      </CodingSessionPromptAttributionContext.Provider>
    </RedactionDictionaryContext.Provider>
  );
}

/**
 * The screen-reader status line. "Idle" only when the caller vouches the
 * session stopped; a session nobody can vouch for is announced as such.
 */
function codingSessionLiveStatusText(
  isWorking: boolean,
  restingStatus: CodingSessionTurnRestingStatus,
): string {
  if (isWorking || restingStatus === "running") return "Coding session working";
  if (restingStatus === "stopped") return "Coding session idle";
  return "Coding session status unknown";
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
    <div className={CODING_SESSION_VIRTUAL_ROW_PAD_CLASS}>
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
  // A settled turn is estimated as it first renders: folded. The gaps come
  // from the same table the turn renders with (SV-08), so a change to the
  // rhythm moves the estimate with it.
  const fold = turn.fold;
  const hidden = new Set(fold?.hiddenIndexes ?? []);
  let previous: CodingSessionRowKind | null = null;
  let height = 0;
  for (const [index, entry] of turn.entries.entries()) {
    if (fold && index === fold.anchorIndex) {
      height += codingSessionRowGap(previous, "fold").px + 36;
      previous = "fold";
    }
    if (hidden.has(index)) continue;
    const kind = codingSessionEntryRowKind(entry);
    height += codingSessionRowGap(previous, kind).px;
    previous = kind;
    if (entry.kind !== "item") {
      height += 28;
    } else if (entry.item.type === "message") {
      height +=
        entry.item.role === "user"
          ? estimateCodingSessionUserMessageHeight(entry.item.text)
          : Math.min(320, 52 + entry.item.text.length / 3);
    } else {
      height += 28;
    }
  }
  if (turn.isWorking) {
    height += codingSessionTurnTailGap(previous, "working").px + 28;
  }
  if (turn.changedFiles.length > 0) {
    height += codingSessionTurnTailGap(previous, "changed-files").px + 48;
  }
  if (!turn.isWorking && (turn.completion || turn.diagnostics.length > 0)) {
    height += codingSessionTurnTailGap(previous, "meta").px + 24;
  }
  return Math.max(88, Math.min(720, height));
}

/**
 * A prompt's bubble: its text, capped where a long prompt clamps (SV-15),
 * plus the author line, plus the "Show full message" control when clamped.
 */
function estimateCodingSessionUserMessageHeight(text: string): number {
  const body = Math.min(52 + text.length / 3, CLAMPED_USER_MESSAGE_BODY_PX);
  return (
    body +
    (shouldClampCodingSessionUserMessage(text) ? CLAMP_TOGGLE_PX : 0) +
    AUTHOR_LINE_PX
  );
}

const CLAMPED_USER_MESSAGE_BODY_PX = 176 + 24;
const CLAMP_TOGGLE_PX = 28;
const AUTHOR_LINE_PX = 20;

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

/**
 * The transcript model of `items`, derived once per change of `items` or
 * `isWorking` and stabilised against the previous one, so an unchanged
 * block, diagnostics list or `sessionFacts` list keeps its reference.
 *
 * `null` items derive nothing and return `null` — the transcript passes that
 * when its caller shared a model it already derived.
 */
export function useStableCodingSessionTranscriptModel(
  items: TranscriptItem[],
  isWorking: boolean,
): CodingSessionTranscriptModel;
export function useStableCodingSessionTranscriptModel(
  items: TranscriptItem[] | null,
  isWorking: boolean,
): CodingSessionTranscriptModel | null;
export function useStableCodingSessionTranscriptModel(
  items: TranscriptItem[] | null,
  isWorking: boolean,
): CodingSessionTranscriptModel | null {
  const previousRef = React.useRef<CodingSessionTranscriptModel | null>(null);
  const next = React.useMemo(
    () =>
      items === null
        ? null
        : deriveCodingSessionTranscriptModel(items, { isWorking }),
    [isWorking, items],
  );
  if (next === null) {
    previousRef.current = null;
    return null;
  }
  const stable = stabilizeCodingSessionTranscriptModel(
    previousRef.current,
    next,
  );
  previousRef.current = stable;
  return stable;
}
