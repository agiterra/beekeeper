import {
  NewCodingSessionChannelPicker,
  NewCodingSessionProjectDestination,
} from "./NewCodingSessionDestination";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import { NewCodingSessionForm } from "./NewCodingSessionLaunchForm";

/**
 * A project the session is being created inside.
 *
 * Supplied by the projects feature's wrapper (glue): the dialog stays the one
 * create flow and only swaps the channel *question* for a channel *fact*.
 */
export type NewCodingSessionProjectContext = {
  projectId: string;
  projectName: string;
  /** Coordinate signed into the create as the session's placement authority. */
  projectRef: string | null;
  /**
   * The repository coordinate (`30617:<owner>:<d>`) this launch's create
   * should name, or null when none is known.
   *
   * LANE-L20 (finding 38): resolved from the checkout the launch found — the
   * project repo whose registered/scanned checkout matched, or the project's
   * only repository — never guessed among two or more with no checkout match.
   */
  repoRef: string | null;
  /** The project's sessions channel, or null until this create publishes one. */
  channelId: string | null;
  /**
   * Local checkout of one of the project's repositories, resolved async —
   * the workdir prefill when the provider has nothing remembered yet.
   */
  defaultWorkdir: string | null;
  /**
   * The project's per-device default agent seat, prefilled once the managed
   * agents resolve — only while the seat is untouched, and only when the
   * agent is still one this computer manages (a stale default is silently
   * ignored rather than producing a create the provider refuses).
   */
  defaultSeat?: { actor: string; role: string } | null;
  /**
   * Resolve — creating it if needed — the channel this session belongs in.
   * Called once, on submit: opening the dialog and walking away must not
   * leave a channel behind.
   */
  ensureChannelId: () => Promise<string>;
};

type NewCodingSessionTargetsInput = Parameters<
  typeof resolveNewCodingSessionTargets
>[0];

/** Resolve click-time selection only from the freshly probed runtime snapshot. */
export function resolveRefreshedNewCodingSessionTarget(input: {
  catalogs: NewCodingSessionTargetsInput["catalogs"];
  channelId: string | null;
  localProvider: NewCodingSessionTargetsInput["localProvider"];
  selectedTargetKey: string | null;
  selectionExplicit: boolean;
}): NewCodingSessionTarget | null {
  return resolveSelectedNewCodingSessionTarget({
    targets: resolveNewCodingSessionTargets({
      catalogs: input.catalogs,
      channelId: input.channelId,
      localProvider: input.localProvider,
    }),
    selectedTargetKey: input.selectedTargetKey,
    selectionExplicit: input.selectionExplicit,
  });
}

/**
 * Launch a coding session into a channel, and optionally into the project that
 * channel serves.
 *
 * This was a full-page route until it became a dialog, and it was two tabs
 * until 2026-09-01. Neither change is cosmetic. Starting a session is
 * something a person does *from* somewhere — a channel, a project — and taking
 * the whole window away made "which channel is this for?" harder to see, not
 * easier. Splitting it into *One session* and *Team* then made "who is
 * running this?" two questions with two half-answers, and the halves drifted:
 * the Team tab had no provider control at all and the One-session tab's model
 * leaked into seats it never showed (item 103, finding 12).
 *
 * There is one form now, and the order of its fields is the order of the
 * thinking: the goal, then who leads it, then what that lead runs on, then
 * who it may hire, then the limits, the directory, and the destination. The
 * name comes late because it is derived from the goal — asking for a title
 * first asks someone to summarize a task they have not described yet.
 */
export function NewCodingSessionDialog({
  channelId,
  onOpenChange,
  open,
  projectContext = null,
}: {
  channelId?: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  projectContext?: NewCodingSessionProjectContext | null;
}) {
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-2xl"
        data-testid="new-coding-session-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            {projectContext
              ? `New coding session in ${projectContext.projectName}`
              : "New coding session"}
          </DialogTitle>
          <DialogDescription className="sr-only">
            Describe the goal, choose who leads it, and say how it runs.
          </DialogDescription>
        </DialogHeader>
        <NewCodingSessionForm
          channelId={channelId}
          onDone={() => onOpenChange(false)}
          projectContext={projectContext}
        />
      </DialogContent>
    </Dialog>
  );
}

// Re-exported so this file stays the one import site for the dialog's parts;
// the implementations moved to keep every file under the 1000-line ceiling.
export { NewCodingSessionChannelPicker, NewCodingSessionProjectDestination };
export { NewCodingSessionForm };
