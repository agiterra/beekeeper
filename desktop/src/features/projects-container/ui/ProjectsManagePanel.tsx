import * as React from "react";
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  EllipsisVertical,
  FileText,
  FolderGit2,
  FolderKanban,
  FolderOpen,
  Hash,
  Lock,
  Pencil,
  Trash2,
  Zap,
} from "lucide-react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useChannelsQuery } from "@/features/channels/hooks";
import { StatusEmoji } from "@/features/user-status/ui/StatusEmoji";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import {
  getCodingSessionWorkdirState,
  pickCodingSessionWorkdir,
  setCodingSessionWorkdir,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import type { Channel } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import {
  partitionChannels,
  useProjectContainers,
  useProjectWorkflowBuckets,
  type ProjectContainer,
} from "../hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  displayProjectsWithGeneral,
} from "../lib/projectContainerModel";
import { useUpdateProjectContainerMutation } from "../projectOrganizeMutations";
import { useProjectCapabilitiesMap } from "../lib/projectPermissions";
import { DeleteProjectDialog } from "./DeleteProjectDialog";
import { ProjectSettingsDialog } from "./ProjectSettingsDialog";
import { MoveToProjectMenu } from "./MoveToProjectMenu";
import { useProjectItemMoves } from "./useProjectItemMoves";

type DragItem =
  | { type: "repo"; repo: CodeRepo; fromId: string | null }
  | { type: "channel" | "forum"; channel: Channel; fromId: string | null };

function DraggableItemRow({
  id,
  data,
  icon,
  label,
  menu,
}: {
  id: string;
  data: DragItem;
  icon: React.ReactNode;
  label: string;
  menu: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id,
    data,
  });
  return (
    <li
      ref={setNodeRef}
      className={cn(
        "flex items-center gap-2 rounded-md px-2 py-1 text-sm hover:bg-muted/50",
        isDragging && "opacity-40",
      )}
      data-testid="manage-item-row"
      {...attributes}
      {...listeners}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {menu}
    </li>
  );
}

function ManageSection({
  title,
  icon,
  children,
  emptyHint,
  count,
}: {
  title: string;
  icon: React.ReactNode;
  children: React.ReactNode;
  emptyHint: string;
  count: number;
}) {
  return (
    <div className="min-w-0 flex-1">
      <div className="mb-1 flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        {icon}
        <span>{title}</span>
        <span className="text-2xs text-muted-foreground/60">{count}</span>
      </div>
      {count === 0 ? (
        <p className="rounded-md border border-dashed border-border/60 px-2 py-2 text-xs text-muted-foreground/60">
          {emptyHint}
        </p>
      ) : (
        <ul className="flex flex-col">{children}</ul>
      )}
    </div>
  );
}

/**
 * The directory this project's coding sessions start in on this computer.
 *
 * Host-local and never published — a working directory names one person's disk,
 * and the project is shared. It is stored under the project's coordinate so
 * every session created for the project defaults to the same checkout, ahead of
 * any per-channel or most-recently-used guess.
 */
function ProjectWorkdirRow({ project }: { project: ProjectContainer }) {
  const [path, setPath] = React.useState<string | null>(null);
  const [isSaving, setIsSaving] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    void getCodingSessionWorkdirState()
      .then((state) => {
        if (!cancelled) {
          setPath(state.byProject[project.address]?.path ?? null);
        }
      })
      .catch(() => {
        if (!cancelled) setPath(null);
      });
    return () => {
      cancelled = true;
    };
  }, [project.address]);

  const handlePick = React.useCallback(() => {
    setIsSaving(true);
    void pickCodingSessionWorkdir()
      .then(async (picked) => {
        if (!picked) return;
        const next = await setCodingSessionWorkdir({
          scope: "project",
          key: project.address,
          path: picked,
        });
        setPath(next.byProject[project.address]?.path ?? picked);
      })
      .catch((error) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Could not save the working directory.",
        );
      })
      .finally(() => setIsSaving(false));
  }, [project.address]);

  return (
    <div
      className="mt-3 flex items-center gap-2 border-t border-border/60 pt-3"
      data-testid={`manage-project-workdir-${project.dtag}`}
    >
      <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="shrink-0 text-xs text-muted-foreground">
        Sessions run in
      </span>
      <span
        className={cn(
          "min-w-0 flex-1 truncate font-mono text-2xs",
          path ? "text-foreground" : "text-muted-foreground/60",
        )}
        title={path ?? undefined}
      >
        {path ?? "Not set — falls back to the most recent directory"}
      </span>
      <button
        className="shrink-0 rounded-md px-2 py-1 text-xs text-muted-foreground/80 transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
        data-testid={`manage-project-workdir-pick-${project.dtag}`}
        disabled={isSaving}
        onClick={handlePick}
        type="button"
      >
        {path ? "Change" : "Choose"}
      </button>
    </div>
  );
}

function DroppableCard({
  id,
  children,
  testId,
}: {
  id: string;
  children: React.ReactNode;
  testId: string;
}) {
  const { setNodeRef, isOver } = useDroppable({ id });
  return (
    <section
      ref={setNodeRef}
      className={cn(
        "rounded-lg border border-border bg-card p-4 transition-colors",
        isOver && "border-primary/60 bg-primary/5",
      )}
      data-testid={testId}
    >
      {children}
    </section>
  );
}

/**
 * The Projects management tab: create/rename/delete project containers and
 * organize repos, channels, and forums between them via per-item menus or
 * drag-and-drop onto project cards.
 */
export function ProjectsManagePanel() {
  const { goWorkflow } = useAppNavigation();
  const { projects, reposByProject, unclaimedRepos } = useProjectContainers();
  const channelsQuery = useChannelsQuery();
  const updateMutation = useUpdateProjectContainerMutation();

  const [editTarget, setEditTarget] = React.useState<ProjectContainer | null>(
    null,
  );
  const [deleteTarget, setDeleteTarget] =
    React.useState<ProjectContainer | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );

  const displayProjects = React.useMemo(
    () => displayProjectsWithGeneral(projects),
    [projects],
  );

  const channelBuckets = React.useMemo(
    () => partitionChannels(projects, channelsQuery.data ?? []),
    [projects, channelsQuery.data],
  );

  // Workflows are channel-scoped; each card summarizes the workflows whose
  // trigger channel belongs to that project.
  const { workflowsEnabled, ...workflowBuckets } = useProjectWorkflowBuckets(
    channelsQuery.data,
    channelBuckets.channelsByProject,
    channelBuckets.forumsByProject,
  );

  const projectById = React.useMemo(
    () => new Map(displayProjects.map((project) => [project.id, project])),
    [displayProjects],
  );

  // One roster read for every card on the panel. Calling the single-project
  // hook per card would be a hook in a loop *and* one REQ per project.
  const { capabilitiesFor } = useProjectCapabilitiesMap(displayProjects);

  const moves = useProjectItemMoves(projectById);
  const { requestMoveChannel, requestMoveForum, requestMoveRepo } = moves;

  const handleDragEnd = React.useCallback(
    (event: DragEndEvent) => {
      const item = event.active.data.current as DragItem | undefined;
      const overId = event.over?.id;
      if (!item || typeof overId !== "string") return;
      const target = projectById.get(overId) ?? null;
      if (!target || item.fromId === target.id) return;
      if (item.type === "repo") {
        requestMoveRepo(item.repo, item.fromId, target);
      } else if (item.type === "forum") {
        requestMoveForum(item.channel, item.fromId, target);
      } else {
        requestMoveChannel(item.channel, item.fromId, target);
      }
    },
    [projectById, requestMoveChannel, requestMoveForum, requestMoveRepo],
  );

  const renderProjectCard = (project: ProjectContainer) => {
    const isGeneral = project.dtag === GENERAL_PROJECT_DTAG;
    const isFallback = project.id === LOCAL_GENERAL_ID;
    // Delete is an Owner capability, not a creator one — the creator and a
    // roster `owner` are the same tier, and the relay authorizes both.
    const canManage = !isFallback && capabilitiesFor(project).canDeleteProject;
    const repos = [
      ...(reposByProject.get(project.id) ?? []),
      ...(isGeneral ? unclaimedRepos : []),
    ];
    const streamChannels = [
      ...(channelBuckets.channelsByProject.get(project.id) ?? []),
      ...(isGeneral ? channelBuckets.globalChannels : []),
    ];
    const workflows = [
      ...(workflowBuckets.byProject.get(project.id) ?? []),
      ...(isGeneral ? workflowBuckets.unclaimed : []),
    ];
    const forums = [
      ...(channelBuckets.forumsByProject.get(project.id) ?? []),
      ...(isGeneral ? channelBuckets.unclaimedForums : []),
    ];

    return (
      <DroppableCard
        key={project.id}
        id={project.id}
        testId={`manage-project-${project.dtag}`}
      >
        <div className="mb-3 flex items-start justify-between gap-2">
          <div className="min-w-0">
            <h3 className="flex items-center gap-2 text-sm font-semibold">
              {project.icon ? (
                <StatusEmoji
                  className="size-4 shrink-0 leading-none"
                  value={project.icon}
                />
              ) : (
                <FolderKanban className="size-4 shrink-0" />
              )}
              <span className="truncate">{project.name}</span>
              {project.visibility === "private" ? (
                <Lock
                  aria-label="Private project"
                  className="size-3.5 shrink-0 text-muted-foreground"
                  data-testid={`manage-project-lock-${project.dtag}`}
                />
              ) : null}
              {isFallback ? (
                <span className="text-2xs font-normal text-muted-foreground">
                  (local)
                </span>
              ) : null}
            </h3>
            {project.description ? (
              <p className="mt-0.5 truncate text-xs text-muted-foreground">
                {project.description}
              </p>
            ) : null}
          </div>
          {/* Settings are reachable for every published project — the dialog
              renders relay fields read-only for non-owners. Delete stays
              owner-only. */}
          {!isFallback ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label={`Actions for ${project.name}`}
                  data-testid={`manage-project-actions-${project.dtag}`}
                  className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
                >
                  <EllipsisVertical className="size-4" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setEditTarget(project)}>
                  <Pencil />
                  Project settings
                </DropdownMenuItem>
                {canManage && !isGeneral ? (
                  <DropdownMenuItem
                    className="text-destructive focus:text-destructive"
                    onSelect={() => setDeleteTarget(project)}
                  >
                    <Trash2 />
                    Delete project
                  </DropdownMenuItem>
                ) : null}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </div>

        <div className="flex flex-col gap-4 md:flex-row">
          <ManageSection
            title="Repositories"
            icon={<FolderGit2 className="size-3.5" />}
            count={repos.length}
            emptyHint="Drop repositories here"
          >
            {repos.map((repo) => (
              <div className="group/manage-row" key={repo.repoAddress}>
                <DraggableItemRow
                  id={`repo:${project.id}:${repo.repoAddress}`}
                  data={{ type: "repo", repo, fromId: project.id }}
                  icon={
                    <FolderGit2 className="size-4 shrink-0 text-muted-foreground" />
                  }
                  label={repo.name}
                  menu={
                    <MoveToProjectMenu
                      currentId={project.id}
                      projects={displayProjects}
                      onMove={(target) => {
                        if (target) requestMoveRepo(repo, project.id, target);
                      }}
                    />
                  }
                />
              </div>
            ))}
          </ManageSection>

          <ManageSection
            title="Channels"
            icon={<Hash className="size-3.5" />}
            count={streamChannels.length}
            emptyHint="Drop channels here"
          >
            {streamChannels.map((channel) => (
              <div className="group/manage-row" key={channel.id}>
                <DraggableItemRow
                  id={`channel:${project.id}:${channel.id}`}
                  data={{ type: "channel", channel, fromId: project.id }}
                  icon={
                    <Hash className="size-4 shrink-0 text-muted-foreground" />
                  }
                  label={channel.name}
                  menu={
                    <MoveToProjectMenu
                      currentId={project.id}
                      projects={displayProjects}
                      onMove={(target) =>
                        requestMoveChannel(channel, project.id, target)
                      }
                    />
                  }
                />
              </div>
            ))}
          </ManageSection>

          <ManageSection
            title="Forums"
            icon={<FileText className="size-3.5" />}
            count={forums.length}
            emptyHint="Drop forums here"
          >
            {forums.map((forum) => (
              <div className="group/manage-row" key={forum.id}>
                <DraggableItemRow
                  id={`forum:${project.id}:${forum.id}`}
                  data={{ type: "forum", channel: forum, fromId: project.id }}
                  icon={
                    <FileText className="size-4 shrink-0 text-muted-foreground" />
                  }
                  label={forum.name}
                  menu={
                    <MoveToProjectMenu
                      currentId={project.id}
                      projects={displayProjects}
                      onMove={(target) => {
                        if (target) requestMoveForum(forum, project.id, target);
                      }}
                    />
                  }
                />
              </div>
            ))}
          </ManageSection>

          {workflowsEnabled ? (
            <ManageSection
              title="Workflows"
              icon={<Zap className="size-3.5" />}
              count={workflows.length}
              emptyHint="No workflows"
            >
              {workflows.map((workflow) => (
                <li key={workflow.id}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-sm hover:bg-muted/50"
                    data-testid="manage-workflow-row"
                    onClick={() => void goWorkflow(workflow.id)}
                  >
                    <Zap className="size-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate">
                      {workflow.name}
                    </span>
                  </button>
                </li>
              ))}
            </ManageSection>
          ) : null}
        </div>

        {isFallback ? null : <ProjectWorkdirRow project={project} />}
      </DroppableCard>
    );
  };

  return (
    <DndContext onDragEnd={handleDragEnd} sensors={sensors}>
      <div className="flex flex-col gap-4" data-testid="projects-manage-panel">
        {displayProjects.map(renderProjectCard)}
      </div>

      <ProjectSettingsDialog
        isSaving={updateMutation.isPending}
        onOpenChange={(open) => {
          if (!open) setEditTarget(null);
        }}
        onSave={async (input) => {
          if (!editTarget) return;
          await updateMutation.mutateAsync({ project: editTarget, ...input });
          toast.success("Project updated.");
        }}
        project={editTarget}
      />

      <DeleteProjectDialog
        project={deleteTarget}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      />

      {moves.confirmDialog}
    </DndContext>
  );
}
