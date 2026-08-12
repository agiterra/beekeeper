import {
  CircleDot,
  FileText,
  FolderGit2,
  FolderInput,
  FolderKanban,
  GitPullRequest,
  Hash,
  Plus,
  Zap,
} from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import {
  POPOVER_SHADOW_STYLE,
  POPOVER_SURFACE_CLASS,
} from "@/shared/ui/popoverSurface";

const MENU_ITEM_CLASS =
  "flex min-h-9 w-full items-center gap-2 rounded-lg py-2 pl-2 pr-4 text-left text-sm outline-hidden transition-colors hover:bg-muted/50 focus:bg-muted/50 focus:text-foreground focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-4 [&_svg]:shrink-0";

type MenuEntry = {
  label: string;
  icon: React.ComponentType;
  action: () => void;
  testId?: string;
};

export function ProjectsCreateMenu({
  onCreateIssue,
  onCreatePullRequest,
  onCreateRepository,
  onImportRepository,
  onCreateChannel,
  onCreateForum,
  onCreateWorkflow,
  onCreateProject,
}: {
  /** Every entry is optional — absent callbacks don't render, so callers can
   * narrow the menu (e.g. only "Project" while no target project is selected,
   * or no channel/forum/workflow/project entries with the experiment off). */
  onCreateIssue?: () => void;
  onCreatePullRequest?: () => void;
  onCreateRepository?: () => void;
  onImportRepository?: () => void;
  onCreateChannel?: () => void;
  onCreateForum?: () => void;
  onCreateWorkflow?: () => void;
  onCreateProject?: () => void;
}) {
  const [open, setOpen] = React.useState(false);
  const containerRef = React.useRef<HTMLElement>(null);

  React.useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: PointerEvent) {
      if (!containerRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    globalThis.document.addEventListener(
      "pointerdown",
      handlePointerDown,
      true,
    );
    return () =>
      globalThis.document.removeEventListener(
        "pointerdown",
        handlePointerDown,
        true,
      );
  }, [open]);

  function select(action: () => void) {
    setOpen(false);
    action();
  }

  const rawGroups: Array<Array<MenuEntry | undefined>> = [
    [
      onCreateIssue && {
        label: "Issue",
        icon: CircleDot,
        action: onCreateIssue,
      },
      onCreatePullRequest && {
        label: "Pull Request",
        icon: GitPullRequest,
        action: onCreatePullRequest,
      },
    ],
    [
      onCreateRepository && {
        label: "Repository",
        icon: FolderGit2,
        action: onCreateRepository,
      },
      onImportRepository && {
        label: "Import local repository",
        icon: FolderInput,
        action: onImportRepository,
        testId: "projects-create-menu-import-repo",
      },
      onCreateChannel && {
        label: "Channel",
        icon: Hash,
        action: onCreateChannel,
      },
      onCreateForum && {
        label: "Forum",
        icon: FileText,
        action: onCreateForum,
      },
      onCreateWorkflow && {
        label: "Workflow",
        icon: Zap,
        action: onCreateWorkflow,
      },
    ],
    [
      onCreateProject && {
        label: "Project",
        icon: FolderKanban,
        action: onCreateProject,
        testId: "projects-create-menu-project",
      },
    ],
  ];
  const groups = rawGroups
    .map((group) => group.filter((entry): entry is MenuEntry => Boolean(entry)))
    .filter((group) => group.length > 0);

  return (
    <nav
      aria-label="Create project item"
      className="relative shrink-0"
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) {
          setOpen(false);
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          setOpen(false);
          containerRef.current?.querySelector<HTMLElement>("button")?.focus();
        }
      }}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      ref={containerRef}
    >
      <Button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label="Create"
        className="h-8 w-8 rounded-full"
        data-testid="projects-create-menu"
        onClick={() => setOpen(true)}
        size="icon"
        type="button"
        variant="default"
      >
        <Plus className="h-4 w-4" />
      </Button>
      {open ? (
        <div className="absolute right-0 top-full z-50 min-w-48 pt-1">
          <div
            className={`rounded-xl p-1 ${POPOVER_SURFACE_CLASS}`}
            role="menu"
            style={POPOVER_SHADOW_STYLE}
          >
            {groups.map((group, groupIndex) => (
              <React.Fragment key={group[0].label}>
                {groupIndex > 0 ? (
                  <div aria-hidden="true" className="my-1 h-px bg-border/60" />
                ) : null}
                {group.map((entry) => (
                  <button
                    className={MENU_ITEM_CLASS}
                    data-testid={entry.testId}
                    key={entry.label}
                    onClick={() => select(entry.action)}
                    role="menuitem"
                    type="button"
                  >
                    <entry.icon />
                    {entry.label}
                  </button>
                ))}
              </React.Fragment>
            ))}
          </div>
        </div>
      ) : null}
    </nav>
  );
}
