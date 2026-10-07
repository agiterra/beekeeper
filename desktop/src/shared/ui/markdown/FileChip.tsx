import * as React from "react";
import { MoreHorizontal } from "lucide-react";
import { toast } from "sonner";

import {
  filePathChipLabel,
  splitFilePathPosition,
} from "@/features/coding-sessions/lib/filePathCandidate";
import type { CodingSessionFileRef } from "@/shared/api/tauriCodingSessionFileRefs";
import { cn } from "@/shared/lib/cn";
import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { isLinuxPlatform, isMacPlatform } from "@/shared/lib/platform";
import { fileTypeIcon } from "@/shared/ui/fileTypeIcon";
import { INLINE_CODE_CHIP_CLASS } from "@/shared/ui/mentionChip";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

import type { FileRefScopeState } from "./fileRefContext";
import {
  MediaContextMenu,
  type MediaContextMenuPosition,
  useDismissMediaContextMenu,
} from "./MediaContextMenu";

/** The reveal action's name on this OS. */
export function revealActionLabel(): string {
  if (isMacPlatform()) return "Reveal in Finder";
  if (isLinuxPlatform()) return "Show in file manager";
  return "Show in Explorer";
}

function failureToast(error: unknown) {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";
  toast.error(message || "This computer could not open that file.");
}

/**
 * A path the agent wrote, as a chip, on the computer that ran the agent.
 *
 * A click opens it in the default app; a right-click or the `⋯` button opens
 * an in-DOM menu (Open, Reveal, Copy relative path, Copy full path). Every
 * action hands the host the **candidate as written**; the host resolves it
 * again, so a file deleted since the answer says so rather than opening
 * something else.
 */
export function FileChip({
  candidate,
  fileRef,
  scope,
}: {
  candidate: string;
  fileRef: CodingSessionFileRef;
  scope: Pick<FileRefScopeState, "open" | "reveal">;
}) {
  const [menu, setMenu] = React.useState<MediaContextMenuPosition | null>(null);
  const closeMenu = React.useCallback(() => setMenu(null), []);
  useDismissMediaContextMenu(Boolean(menu), closeMenu);

  const { path, line } = splitFilePathPosition(candidate);
  const { Icon, colorClass, family } = fileTypeIcon(path, fileRef.isDir);
  const label = filePathChipLabel(candidate);
  const open = React.useCallback(() => {
    scope.open(candidate).catch(failureToast);
  }, [candidate, scope]);
  const reveal = React.useCallback(() => {
    scope.reveal(candidate).catch(failureToast);
  }, [candidate, scope]);

  const items = [
    { label: "Open", onSelect: open },
    { label: revealActionLabel(), onSelect: reveal },
    ...(fileRef.relativePath
      ? [
          {
            label: "Copy relative path",
            onSelect: () =>
              copyTextToClipboard(
                fileRef.relativePath ?? "",
                "Relative path copied",
              ),
          },
        ]
      : []),
    ...(fileRef.fullPath
      ? [
          {
            label: "Copy full path",
            onSelect: () =>
              copyTextToClipboard(fileRef.fullPath ?? "", "Full path copied"),
          },
        ]
      : []),
  ];

  return (
    <span className="group/file-chip inline-flex items-baseline">
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            aria-label={`Open ${label}`}
            className={cn(
              INLINE_CODE_CHIP_CLASS,
              "inline-flex cursor-pointer items-baseline gap-1 border border-border/70",
              "hover:bg-muted focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring",
            )}
            data-file-chip=""
            data-file-ref-candidate={candidate}
            data-file-type={family}
            data-testid="coding-session-file-chip"
            onClick={(event) => {
              event.preventDefault();
              open();
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              setMenu({ x: event.clientX, y: event.clientY });
            }}
            type="button"
          >
            <Icon
              aria-hidden
              className={cn(
                // The chip rule sets the button inline (markdown.css), so the
                // icon must be inline too or preflight's block svg wraps it.
                "me-1 inline-block size-3.5 shrink-0 align-[-0.15em]",
                colorClass,
              )}
            />
            <span>{label}</span>
          </button>
        </TooltipTrigger>
        <TooltipContent
          className="max-w-sm"
          data-testid="coding-session-file-chip-tooltip"
        >
          <div className="font-mono text-xs">
            {fileRef.relativePath ?? "Outside this session's folder"}
          </div>
          <div className="text-2xs text-muted-foreground">
            On this computer
            {line !== undefined
              ? ` · Opens in the default app — line ${line} not selected`
              : ""}
          </div>
        </TooltipContent>
      </Tooltip>
      <button
        aria-label={`Actions for ${label}`}
        className="ms-0.5 self-center rounded-sm p-px text-muted-foreground opacity-0 hover:bg-muted focus-visible:opacity-100 group-hover/file-chip:opacity-100"
        data-testid="coding-session-file-chip-more"
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          const rect = event.currentTarget.getBoundingClientRect();
          setMenu({ x: rect.left, y: rect.bottom + 4 });
        }}
        type="button"
      >
        <MoreHorizontal aria-hidden className="size-3" />
      </button>
      {menu ? (
        <MediaContextMenu
          dataAttributes={["data-file-chip-menu"]}
          items={items}
          position={menu}
        />
      ) : null}
    </span>
  );
}

/**
 * A path-shaped span that stays plain code, with the reason it is not a chip
 * here ("Written on another computer — open it there", "This computer ran
 * this agent, but its folder is gone", "Not found in this session's folder").
 * The tooltip is what keeps plain text honest: "not clickable here" reads
 * differently from "not a path".
 */
export function FileRefPlainCode({
  children,
  className,
  reason,
  ...props
}: React.ComponentProps<"code"> & { reason: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <code
          {...props}
          className={cn(INLINE_CODE_CHIP_CLASS, className)}
          data-file-ref-plain=""
          data-file-ref-reason={reason}
          data-testid="coding-session-file-ref-plain"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: the reason must be reachable by keyboard
          tabIndex={0}
        >
          {children}
        </code>
      </TooltipTrigger>
      <TooltipContent
        className="max-w-xs"
        data-testid="coding-session-file-ref-reason"
      >
        {reason}
      </TooltipContent>
    </Tooltip>
  );
}
