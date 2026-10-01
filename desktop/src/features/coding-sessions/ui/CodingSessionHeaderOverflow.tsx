import {
  Download,
  Ellipsis,
  ExternalLink,
  FolderPlus,
  OctagonX,
  RotateCcw,
  ShieldOff,
  Square,
  UserPlus,
} from "lucide-react";

import {
  CODING_SESSION_FULL_ACCESS_LABEL,
  codingSessionFullAccessDetail,
} from "@/features/coding-sessions/lib/codingSessionFullAccess";
import { NEW_SESSION_IN_WORKSPACE_LABEL } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import type { CodingSessionFullAccess } from "./useCodingSessionFullAccess";

/**
 * The Mission header's `⋯` menu — DESIGN-SPEC A7, built.
 *
 * Critique A6: row 1 ran `People 4` · `Stop all (2)` · `Close`, all the same
 * size and the same `ghost` variant, adjacent. One of those is unrecoverable
 * by its own title and the other has a `Reopen`. Two controls that read
 * identically and differ by everything.
 *
 * So the six *actions* collapse here, in the order A7 fixes, and `People` and
 * the surface toggles stay in row 1 — they are navigation, not action, and a
 * menu is the wrong home for a thing you toggle while reading. `Stop all`
 * takes the destructive treatment the composer's own `Stop execution` item
 * already uses (`CodingSessionComposerDeck.tsx`): `text-destructive`, and a
 * second line naming the consequence rather than a tooltip nobody opens.
 *
 * Mission only. Conversation's header keeps its flat run of six buttons and
 * its DOM byte for byte (I8), which is why this is a separate component the
 * header mounts under a lens gate rather than a rewrite of the run itself.
 */
export function CodingSessionHeaderOverflow({
  fullAccess = null,
  isExporting = false,
  newSessionInWorkspaceDetail,
  onAddProvider,
  onCloseSession,
  onExport,
  onNewSessionInWorkspace,
  onOpenChange,
  onPopout,
  onReopenSession,
  onStopAll,
  stopAllLabel,
  stopAllSentence,
}: {
  /**
   * This execution's full-access grant, when its provider is this computer's
   * and the host answered. Null — a foreign provider, a read in flight or
   * failed — offers no item at all rather than an "off" nobody read.
   */
  fullAccess?: CodingSessionFullAccess | null;
  isExporting?: boolean;
  /**
   * The workspace item's second line, resolved by whoever opened this menu.
   *
   * Omitted means *say nothing*: this menu holds a callback, not a read, and
   * a line invented here would be a second answer to a question the sidebar
   * row already answers from the same hook. Silence is honest; a sentence
   * that disagrees with the row is not.
   */
  newSessionInWorkspaceDetail?: string;
  onAddProvider?: () => void;
  onCloseSession?: () => void;
  onExport?: () => void;
  /**
   * Opens a draft on this session's own directory — a read and a dialog, no
   * more (`useNewSessionInWorkspaceAction`). The item carries the shared
   * label so this menu and the sidebar row's context menu cannot drift.
   */
  onNewSessionInWorkspace?: () => void;
  /**
   * Fires when the menu opens and closes. The workspace read runs on open —
   * never on render, hover or scroll — so the caller resolves here and hands
   * the answer back through {@link newSessionInWorkspaceDetail}.
   */
  onOpenChange?: (open: boolean) => void;
  onPopout?: () => void;
  onReopenSession?: () => void;
  onStopAll?: () => void;
  /** `Stop all (2 seats)` — the count of seats, never of "live" seats. */
  stopAllLabel: string;
  /**
   * `Stop 2 seats — 1 live, 1 idle. A stopped seat cannot be resumed.`
   *
   * Built by `codingSessionStopAllModel` from the same W1 map the seat chips
   * read, so the menu cannot print a liveness split the chips disagree with.
   * It is the item's accessible name *and* its visible second line: A6's
   * complaint was that the stakes lived only in a `title`.
   */
  stopAllSentence: string;
}) {
  const items: OverflowItem[] = [];
  // First, and above the destructive run: it creates rather than ends, and
  // it is the only item here that starts something new. Its second line is
  // whatever the opener resolved — the same string, from the same hook, that
  // the sidebar row's item shows for this session — or nothing at all.
  if (onNewSessionInWorkspace) {
    items.push({
      detail: newSessionInWorkspaceDetail,
      icon: <FolderPlus aria-hidden className="size-3.5" />,
      key: "new-session-in-workspace",
      label: NEW_SESSION_IN_WORKSPACE_LABEL,
      onSelect: onNewSessionInWorkspace,
      testId: "coding-session-overflow-new-session-in-workspace",
    });
  }
  if (onAddProvider) {
    items.push({
      icon: <UserPlus aria-hidden className="size-3.5" />,
      key: "add-provider",
      label: "Add provider…",
      onSelect: onAddProvider,
      testId: "coding-session-overflow-add-provider",
    });
  }
  if (fullAccess) {
    const detail = codingSessionFullAccessDetail(fullAccess);
    items.push({
      detail,
      disabled: fullAccess.pending !== null,
      icon: <ShieldOff aria-hidden className="size-3.5" />,
      key: "full-access",
      label: CODING_SESSION_FULL_ACCESS_LABEL,
      name: `${CODING_SESSION_FULL_ACCESS_LABEL}: ${detail}`,
      onSelect: fullAccess.toggle,
      pressed: fullAccess.granted,
      testId: "coding-session-overflow-full-access",
    });
  }
  if (onStopAll) {
    items.push({
      destructive: true,
      detail: stopAllSentence,
      icon: <OctagonX aria-hidden className="size-3.5" />,
      key: "stop-all",
      label: stopAllLabel,
      name: stopAllSentence,
      onSelect: onStopAll,
      testId: "coding-session-overflow-stop-all",
    });
  }
  if (onCloseSession) {
    items.push({
      detail: "Moves it to Settled; its providers keep running.",
      icon: <Square aria-hidden className="size-3.5" />,
      key: "close-session",
      label: "Close session",
      onSelect: onCloseSession,
      testId: "coding-session-overflow-close-session",
    });
  }
  if (onReopenSession) {
    items.push({
      detail: "Returns it to Sessions; starts no provider.",
      icon: <RotateCcw aria-hidden className="size-3.5" />,
      key: "reopen-session",
      label: "Reopen session",
      onSelect: onReopenSession,
      testId: "coding-session-overflow-reopen-session",
    });
  }
  if (onExport) {
    items.push({
      disabled: isExporting,
      icon: <Download aria-hidden className="size-3.5" />,
      key: "export",
      label: "Export transcript",
      onSelect: onExport,
      testId: "coding-session-overflow-export",
    });
  }
  if (onPopout) {
    items.push({
      icon: <ExternalLink aria-hidden className="size-3.5" />,
      key: "popout",
      label: "Pop out",
      onSelect: onPopout,
      testId: "coding-session-overflow-popout",
    });
  }
  if (items.length === 0) return null;

  return (
    <Popover onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <button
          aria-label="Session actions"
          className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="coding-session-overflow"
          title="Session actions"
          type="button"
        >
          <Ellipsis aria-hidden className="size-4" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 p-2" side="bottom">
        <fieldset aria-label="Session actions" className="flex flex-col">
          {items.map((item) => (
            <button
              aria-label={item.name}
              aria-pressed={item.pressed}
              className={cn(
                "flex w-full items-start gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
                item.destructive
                  ? "text-destructive hover:bg-destructive/10"
                  : "text-foreground hover:bg-muted",
              )}
              data-testid={item.testId}
              disabled={item.disabled}
              key={item.key}
              onClick={item.onSelect}
              type="button"
            >
              <span className="mt-0.5 shrink-0">{item.icon}</span>
              <span className="min-w-0">
                <span className="block font-medium">{item.label}</span>
                {item.detail ? (
                  <span className="block text-xs text-muted-foreground">
                    {item.detail}
                  </span>
                ) : null}
              </span>
            </button>
          ))}
        </fieldset>
      </PopoverContent>
    </Popover>
  );
}

type OverflowItem = {
  destructive?: boolean;
  detail?: string;
  disabled?: boolean;
  icon: React.ReactNode;
  key: string;
  label: string;
  /** Accessible name when it must say more than the label does. */
  name?: string;
  onSelect: () => void;
  /** Set only on a toggle item: its current, host-read state. */
  pressed?: boolean;
  testId: string;
};
