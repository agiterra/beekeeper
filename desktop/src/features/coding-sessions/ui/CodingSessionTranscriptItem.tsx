import * as React from "react";
import { CircleAlert } from "lucide-react";

import { ToolItem } from "@/features/agents/ui/AgentSessionToolItem";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { ThoughtDisclosure } from "@/features/agents/ui/activityRenderClasses/ThoughtActivity";
import { TranscriptActivityItem } from "@/features/agents/ui/activityRenderClasses/TranscriptActivityItem";
import {
  isCodingSessionTranscriptError,
  isCompletedSuccessfulCodingSessionTool,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  resolveCodingSessionPromptAuthor,
  type CodingSessionHireDispatchVouch,
  type CodingSessionPromptSeatResolver,
} from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import {
  deriveCodingSessionTaskModel,
  type CodingSessionTaskModel,
} from "@/features/coding-sessions/lib/codingSessionTaskModel";
import {
  codingSessionWakeReadingForText,
  type CodingSessionWakeOperationIndex,
} from "@/features/coding-sessions/lib/codingSessionWakeReading";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { Markdown } from "@/shared/ui/markdown";
import { RedactedText } from "@/shared/ui/RedactedPill";
import {
  useCodingSessionDisclosure,
  useCodingSessionDisclosureProps,
} from "./CodingSessionTranscriptDisclosure";
import {
  CodingSessionActiveTool,
  CodingSessionInlinePlan,
} from "./CodingSessionTranscriptParts";

/**
 * One transcript item — a prompt, the agent's prose, a tool call, a plan, a
 * thought, an error — and the contexts it reads. Split out of
 * `CodingSessionTranscript.tsx` for the 1000-line ceiling.
 *
 * Every disclosure is keyed `item:<item id>` in the transcript's disclosure
 * store, so a row opened, scrolled away and scrolled back is still open, and
 * opening it re-renders that row alone.
 */

/** The empty index — one frozen instance, so an absent prop is reference-stable. */
export const NO_WAKE_OPERATIONS: CodingSessionWakeOperationIndex = new Map();

/**
 * Everything the deeply nested message row needs to name a prompt's author.
 *
 * Carried by context rather than props: the row sits five `React.memo` layers
 * below the transcript, and threading two more props through all of them would
 * break the memoisation those layers exist for. The default is the honest
 * empty state — no local identity, no resolved profiles — which renders every
 * prompt exactly as it did before attribution existed.
 */
export const CodingSessionPromptAttributionContext = React.createContext<{
  currentUserPubkey: string | null;
  profiles: UserProfileLookup | undefined;
  /**
   * Verified prompt echoes that retired one of this client's optimistic rows
   * on their words alone, because the provider named no command id. The
   * message says so rather than presenting a guess as a match.
   */
  textSettledEchoIds: ReadonlySet<string>;
  /**
   * Names a seat from its actor pubkey, so a turn one seat sent to another
   * reads as that seat rather than as the reader or as a truncated hash.
   * Absent outside Mission, where there is only one seat to confuse.
   */
  resolveSeat?: CodingSessionPromptSeatResolver;
  /**
   * Signed evidence that this execution is a seat this computer hired, when
   * there is any. Null everywhere else — only the founder's machine answers
   * hires, and a name this client cannot check is a claim, not an attribution.
   */
  hireDispatch?: CodingSessionHireDispatchVouch | null;
  /** Fold-resolved operations for the wake reading; empty when none is held. */
  wakeOperations: CodingSessionWakeOperationIndex;
}>({
  currentUserPubkey: null,
  profiles: undefined,
  textSettledEchoIds: new Set(),
  resolveSeat: undefined,
  hireDispatch: null,
  wakeOperations: NO_WAKE_OPERATIONS,
});

/**
 * The execution's generation id, which the shared agent rows take as their
 * agent key. Carried by context so the virtualizer's row renderer can be one
 * stable module-level function.
 */
export const CodingSessionTranscriptGenerationContext =
  React.createContext<string>("");

const GENERIC_AGENT_IDENTITY = {
  agentAvatarUrl: null,
  agentName: "Coding session",
};

export function codingSessionItemDisclosureId(itemId: string): string {
  return `item:${itemId}`;
}

export const CodingSessionItem = React.memo(function CodingSessionItem({
  item,
}: {
  item: TranscriptItem;
}) {
  const generationId = React.useContext(
    CodingSessionTranscriptGenerationContext,
  );
  const promptAttribution = React.useContext(
    CodingSessionPromptAttributionContext,
  );

  if (item.type === "message") {
    if (item.role === "user") {
      const author = resolveCodingSessionPromptAuthor({
        commandId: item.commandId,
        currentUserPubkey: promptAttribution.currentUserPubkey,
        // A hired seat's first turn is signed by the founder's Desktop and
        // written by a lead. Nothing about the *words* says so; the vouch, the
        // absent command id and the stamped operator do.
        hireDispatch: promptAttribution.hireDispatch,
        operatorPubkey: item.operatorPubkey,
        profiles: promptAttribution.profiles,
        resolveSeat: promptAttribution.resolveSeat,
      });
      // Only ever true for a provider that stamps no command id on its echo:
      // this client matched the message to what it sent by comparing the
      // words, which two identical messages defeat.
      const settledByText = promptAttribution.textSettledEchoIds.has(item.id);
      // Finding 17: a turn whose whole text is a team-wake pointer is a
      // machine's addressing, not a person's words. It is read here — never
      // parsed for meaning — into the one line §1f freezes, identically in
      // both lenses because both render this component. Prose returns `null`
      // and takes the untouched path below.
      //
      // `{Who}` is resolved from the **author key**, not from the byline: the
      // byline calls an automatic `team-wake-` command `Beekeeper · team wake`
      // (which is the right caption for a turn nobody typed) and §1f's
      // sentence wants the person or seat the record is signed by. Same
      // resolver, without the two automatic-prefix branches.
      const wakeLine = codingSessionWakeReadingForText({
        operations: promptAttribution.wakeOperations,
        // F2: the key that signed **this turn**. The reading refuses to name
        // an act unless it matches the operation's own author.
        signerPubkey: item.operatorPubkey ?? null,
        text: item.text,
        who: resolveCodingSessionPromptAuthor({
          currentUserPubkey: promptAttribution.currentUserPubkey,
          operatorPubkey: item.operatorPubkey,
          profiles: promptAttribution.profiles,
          resolveSeat: promptAttribution.resolveSeat,
        }).label,
      });
      return (
        <div
          className="group flex flex-col items-end gap-1"
          data-role="user-message"
          data-settled-by={settledByText ? "text" : undefined}
          data-testid="coding-session-user-message"
          data-wake-pointer={wakeLine === null ? undefined : "read"}
          title={
            settledByText
              ? "Matched to the turn you sent by its text — this provider's echo named no command id."
              : undefined
          }
        >
          <div className="min-w-0 max-w-[80%] rounded-2xl bg-muted px-4 py-3 text-base leading-6 text-foreground shadow-sm ring-1 ring-border/40">
            {wakeLine === null ? (
              <Markdown content={item.text.trim() || " "} mediaInset />
            ) : (
              <p data-testid="coding-session-user-message-wake">{wakeLine}</p>
            )}
          </div>
          <p className="pe-1 text-2xs text-muted-foreground">
            <span
              className={
                author.kind === "team-wake" ||
                author.kind === "hire-host" ||
                author.kind === "unrecorded"
                  ? "font-medium text-muted-foreground"
                  : "font-medium text-foreground/75"
              }
              data-author-kind={author.kind}
              data-testid="coding-session-user-message-author"
            >
              {author.label}
            </span>
            {item.steered === true ? (
              // A mid-turn correction the running turn took, not the prompt
              // that opened it. Said beside the author rather than hidden in
              // the `title`, because "who said it" and "when it went in" are
              // read together.
              <span
                className="ms-1 rounded-sm bg-muted px-1 font-medium text-foreground/75"
                data-testid="coding-session-user-message-steered"
                title="Injected into the turn that was already running"
              >
                steered
              </span>
            ) : null}
            {formatCodingSessionMessageTimestamp(item.timestamp)}
          </p>
        </div>
      );
    }

    return (
      <article
        className="min-w-0 text-base leading-6 text-foreground"
        data-role="assistant-message"
        data-testid="coding-session-assistant-message"
      >
        <Markdown
          className="text-base leading-6"
          content={item.text.trim() || " "}
        />
      </article>
    );
  }

  if (item.type === "thought") {
    return <CodingSessionThoughtRow item={item} />;
  }

  if (item.type === "tool") {
    return <CodingSessionToolRow generationId={generationId} item={item} />;
  }

  if (item.type === "plan") {
    const planModel = deriveCodingSessionTaskModel([item]);
    if (planModel) {
      return <CodingSessionPlanRow itemId={item.id} model={planModel} />;
    }
  }

  if (isCodingSessionTranscriptError(item)) {
    return (
      <div
        className="rounded-lg border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        data-testid="coding-session-error"
        role="alert"
      >
        <div className="flex items-start gap-2">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          <div className="min-w-0">
            <p className="font-medium">
              <RedactedText text={item.title || "Session error"} />
            </p>
            {"text" in item && item.text ? (
              <p className="mt-1 whitespace-pre-wrap text-xs opacity-85">
                <RedactedText text={item.text} />
              </p>
            ) : null}
          </div>
        </div>
      </div>
    );
  }

  return (
    <TranscriptActivityItem
      {...GENERIC_AGENT_IDENTITY}
      agentPubkey={generationId}
      item={item}
    />
  );
});

function CodingSessionThoughtRow({
  item,
}: {
  item: Extract<TranscriptItem, { type: "thought" }>;
}) {
  const [open, setOpen] = useCodingSessionDisclosure(
    codingSessionItemDisclosureId(item.id),
  );
  return <ThoughtDisclosure item={item} onOpenChange={setOpen} open={open} />;
}

function CodingSessionPlanRow({
  itemId,
  model,
}: {
  itemId: string;
  model: CodingSessionTaskModel;
}) {
  const disclosure = useCodingSessionDisclosureProps(
    codingSessionItemDisclosureId(itemId),
  );
  return <CodingSessionInlinePlan {...disclosure} model={model} />;
}

/** A tool call: a plan snapshot, a call still running, or a settled row. */
export function CodingSessionToolRow({
  generationId,
  item,
}: {
  generationId: string;
  item: Extract<TranscriptItem, { type: "tool" }>;
}) {
  const disclosureId = codingSessionItemDisclosureId(item.id);
  const [open, setOpen] = useCodingSessionDisclosure(disclosureId);
  const onOpenChange = React.useCallback(
    (_id: string, next: boolean) => setOpen(next),
    [setOpen],
  );
  const disclosure = { disclosureId, open, onOpenChange };
  const planModel = React.useMemo(
    () =>
      isCompletedSuccessfulCodingSessionTool(item)
        ? deriveCodingSessionTaskModel([item])
        : null,
    [item],
  );
  if (planModel) {
    return <CodingSessionInlinePlan {...disclosure} model={planModel} />;
  }
  if (item.status === "executing" || item.status === "pending") {
    return <CodingSessionActiveTool {...disclosure} item={item} />;
  }
  return (
    <ToolItem
      {...GENERIC_AGENT_IDENTITY}
      agentPubkey={generationId}
      item={item}
      onOpenChange={setOpen}
      open={open}
    />
  );
}

function formatCodingSessionMessageTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "";
  return ` · ${date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  })}`;
}
