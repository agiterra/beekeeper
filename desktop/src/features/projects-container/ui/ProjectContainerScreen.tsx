import * as React from "react";
import {
  Bot,
  EllipsisVertical,
  FileText,
  FolderGit2,
  FolderKanban,
  Hash,
  Lock,
  Pencil,
  Plus,
  Terminal,
  Trash2,
  Zap,
} from "lucide-react";
import { toast } from "sonner";

import { useNavigate } from "@tanstack/react-router";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { ProjectTerminalsCard } from "@/features/builtin-shell/ui/ProjectTerminalsCard";
import { ProjectTodosCard } from "@/features/project-todos/ui/ProjectTodosCard";
import { ProjectPulseCard } from "@/features/project-pulse/ui/ProjectPulseCard";
import { ProjectPulseScreen } from "@/features/project-pulse/ui/ProjectPulseScreen";
import { useChannelsQuery } from "@/features/channels/hooks";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { useDeleteRepositoryDialog } from "@/features/projects/ui/useDeleteRepositoryDialog";
import {
  useManagedAgentsQuery,
  usePersonasQuery,
} from "@/features/agents/hooks";
import { FeatureGate, useFeatureEnabled } from "@/shared/features";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import type { Channel } from "@/shared/api/types";

import {
  partitionChannels,
  useProjectCodingSessionBuckets,
  useProjectContainers,
  useProjectWorkflowBuckets,
  type ProjectContainer,
} from "../hooks";
import {
  LOCAL_GENERAL_ID,
  GENERAL_PROJECT_DTAG,
  displayProjectsWithGeneral,
  makeLocalGeneral,
} from "../lib/projectContainerModel";
import { attachableProjectRepos } from "../lib/attachableRepos";
import { projectAgentRows } from "../lib/projectChildren";
import { useProjectRosterQuery } from "../lib/projectMembers";
import { useProjectCapabilities } from "../lib/projectPermissions";
import { compareProjectCodingSessionEntries } from "../lib/projectCodingSessionShelf";
import { resolveProjectCodingSessionOpenTarget } from "../lib/projectFoundedCodingSessionShelf";
import { useRelayOrigin } from "@/shared/lib/useRelayOrigin";
import { withoutProjectSessionTransportChannels } from "../lib/projectSessionsChannel";
import { useUpdateProjectContainerMutation } from "../projectOrganizeMutations";
import {
  linkedRepoCloneUrl,
  useLinkProjectRepoMutation,
} from "../useLinkProjectRepo";
import { DeleteProjectDialog } from "./DeleteProjectDialog";
import { ProjectSettingsDialog } from "./ProjectSettingsDialog";
import { LinkProjectRepoDialog } from "./LinkProjectRepoDialog";
import { MoveToProjectMenu } from "./MoveToProjectMenu";
import { ProjectMembersCard } from "./ProjectMembersCard";
import { ProjectPageTabs, type ProjectPageTab } from "./ProjectPageTabs";
import { SectionCard, EmptyHint } from "./SectionCard";
import { ProjectSectionRepoAddMenu } from "./ProjectSectionRepoAddMenu";
import {
  ProjectsScreenCreateDialogs,
  type ProjectsScreenCreateKind,
} from "./ProjectsScreenCreateDialogs";
import { useProjectItemMoves } from "./useProjectItemMoves";

/**
 * Project container home: the project-scoped management surface. Everything
 * inside the project — repos, channels, forums, workflows, and agents —
 * links to its existing screen; sections offer scoped creation, item rows
 * move between projects, and the owner can edit or delete the project.
 */
export function ProjectContainerScreen({
  projectId,
  tab = "overview",
}: {
  projectId: string;
  /** Which page section the route asked for; the overview is the default. */
  tab?: ProjectPageTab;
}) {
  const {
    goAgents,
    goChannel,
    goCodingSession,
    goFoundedCodingSession,
    goNewProjectCodingSession,
    goProjectRepo,
    goProjects,
    goWorkflow,
  } = useAppNavigation();
  const navigate = useNavigate();
  const { projects, reposByProject, unclaimedRepos } = useProjectContainers();
  // Transports included: the session buckets subscribe to them; the stream
  // list below filters them out via withoutProjectSessionTransportChannels.
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const personas = usePersonasQuery();
  const managedAgents = useManagedAgentsQuery();
  const pulseEnabled = useFeatureEnabled("project-pulse");

  const project: ProjectContainer | null = React.useMemo(() => {
    // The fallback URL upgrades to the real General once one is published
    // (e.g. right after creating an item from this screen).
    if (projectId === LOCAL_GENERAL_ID) {
      return (
        projects.find((candidate) => candidate.dtag === GENERAL_PROJECT_DTAG) ??
        makeLocalGeneral()
      );
    }
    return (
      projects.find(
        (candidate) =>
          candidate.id === projectId || candidate.dtag === projectId,
      ) ?? null
    );
  }, [projects, projectId]);

  const channelBuckets = React.useMemo(
    () => partitionChannels(projects, channelsQuery.data ?? []),
    [projects, channelsQuery.data],
  );

  // Workflows are channel-scoped; their project is the channel's project.
  const { workflowsEnabled, ...workflowBuckets } = useProjectWorkflowBuckets(
    channelsQuery.data,
    channelBuckets.channelsByProject,
    channelBuckets.forumsByProject,
  );
  const sessionBuckets = useProjectCodingSessionBuckets(
    channelsQuery.data,
    channelBuckets.channelsByProject,
    channelBuckets.forumsByProject,
    channelBuckets.transportsByProject,
  );

  const personasById = React.useMemo(
    () =>
      new Map(
        (personas.data ?? []).map((persona) => [
          persona.id,
          persona.displayName,
        ]),
      ),
    [personas.data],
  );
  const managedAgentsByPubkey = React.useMemo(
    () =>
      new Map(
        (managedAgents.data ?? []).map((agent) => [
          agent.pubkey.toLowerCase(),
          agent.name,
        ]),
      ),
    [managedAgents.data],
  );

  const displayProjects = React.useMemo(
    () => displayProjectsWithGeneral(projects),
    [projects],
  );
  const projectById = React.useMemo(
    () => new Map(displayProjects.map((entry) => [entry.id, entry])),
    [displayProjects],
  );
  const moves = useProjectItemMoves(projectById);

  // Hide the "Add existing repository" entry while no repo outside this
  // project exists to move in.
  const attachableRepoCount = React.useMemo(
    () =>
      project
        ? attachableProjectRepos(
            projects,
            reposByProject,
            unclaimedRepos,
            project,
          ).candidates.length
        : 0,
    [projects, reposByProject, unclaimedRepos, project],
  );

  const updateMutation = useUpdateProjectContainerMutation();
  const linkMutation = useLinkProjectRepoMutation();
  // The header count and Members card share this cached roster read; it
  // falls back to the head event's members until a 39010 projection exists.
  const rosterQuery = useProjectRosterQuery(project);
  const capabilities = useProjectCapabilities(project);
  const repoDelete = useDeleteRepositoryDialog();
  const relayOrigin = useRelayOrigin();
  const [editOpen, setEditOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [createKind, setCreateKind] =
    React.useState<ProjectsScreenCreateKind | null>(null);
  const [linkRepo, setLinkRepo] = React.useState<CodeRepo | null>(null);

  if (!project) {
    // A Pulse deep link to a project this community cannot read still gets
    // Pulse's own answer: it distinguishes "still loading" from "not readable"
    // and says neither is a claim that the project is empty. A bare "not
    // found" here would state a completed verdict over a read in flight.
    if (tab === "pulse" && pulseEnabled) {
      return (
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
          <div
            className="mx-auto w-full max-w-3xl px-6 py-8"
            data-testid="project-pulse-tab"
          >
            <ProjectPulseScreen embedded projectId={projectId} />
          </div>
        </div>
      );
    }
    return (
      <div className="flex flex-1 items-center justify-center">
        <EmptyHint>Project not found.</EmptyHint>
      </div>
    );
  }

  const isGeneral = project.dtag === GENERAL_PROJECT_DTAG;
  const isFallback = project.id === LOCAL_GENERAL_ID;
  // Delete is an Owner capability, not a creator one: the project's creator
  // and anybody seated `owner` on its roster are the same tier, and the
  // relay authorizes both (`project_owner_admits_deletion`).
  const canManage = !isFallback && capabilities.canDeleteProject;
  const repos = [
    ...(reposByProject.get(project.id) ?? []),
    ...(isGeneral ? unclaimedRepos : []),
  ];
  // Each half is sorted, the concat is not — re-sort so General's unclaimed
  // sessions interleave by status/recency instead of always trailing.
  const codingSessions = [
    ...(sessionBuckets.byProject.get(project.id) ?? []),
    ...(isGeneral ? sessionBuckets.unclaimed : []),
  ].sort(compareProjectCodingSessionEntries);
  const streamChannels = withoutProjectSessionTransportChannels({
    projectName: project.name,
    channels: [
      ...(channelBuckets.channelsByProject.get(project.id) ?? []),
      ...(isGeneral ? channelBuckets.globalChannels : []),
    ],
    codingSessions,
  });
  const forums = [
    ...(channelBuckets.forumsByProject.get(project.id) ?? []),
    ...(isGeneral ? channelBuckets.unclaimedForums : []),
  ];
  const workflows = workflowsEnabled
    ? [
        ...(workflowBuckets.byProject.get(project.id) ?? []),
        ...(isGeneral ? workflowBuckets.unclaimed : []),
      ]
    : [];
  const agents = projectAgentRows(project, personasById, managedAgentsByPubkey);
  const actionIconButton = (
    label: string,
    testid: string,
    onClick: () => void,
    disabled?: boolean,
  ) => (
    <button
      type="button"
      aria-label={label}
      data-testid={testid}
      disabled={disabled}
      className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
      onClick={onClick}
    >
      <Plus className="size-4" />
    </button>
  );

  const createButton = (kind: ProjectsScreenCreateKind, label: string) =>
    actionIconButton(label, `project-section-create-${kind}`, () =>
      setCreateKind(kind),
    );

  const channelRow = (
    channel: Channel,
    icon: React.ReactNode,
    onMove: (target: ProjectContainer) => void,
  ) => (
    <li
      className="group/manage-row flex items-center gap-1"
      data-testid="project-screen-item-row"
      key={channel.id}
    >
      <Button
        className="h-8 min-w-0 flex-1 justify-start gap-2 px-2"
        onClick={() => void goChannel(channel.id)}
        variant="ghost"
      >
        {icon}
        <span className="truncate">{channel.name}</span>
      </Button>
      <MoveToProjectMenu
        currentId={project.id}
        projects={displayProjects}
        onMove={onMove}
      />
    </li>
  );

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-3xl px-6 py-8">
        <header className="mb-6 flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="flex items-center gap-2 text-xl font-semibold">
              <FolderKanban className="size-5" />
              {project.name}
              {project.visibility === "private" ? (
                <Lock
                  aria-label="Private project"
                  className="size-4 shrink-0 text-muted-foreground"
                  data-testid="project-container-lock"
                />
              ) : null}
            </h1>
            {project.description ? (
              <p className="mt-1 text-sm text-muted-foreground">
                {project.description}
              </p>
            ) : null}
            {(() => {
              // Roster (authoritative when loaded) + the implicit creator.
              const rosterSize = (rosterQuery.data ?? project.members).length;
              const memberCount = rosterSize + 1;
              if (project.visibility !== "private" && rosterSize === 0) {
                return null;
              }
              return (
                <p
                  className="mt-1 text-xs text-muted-foreground"
                  data-testid="project-container-member-count"
                >
                  {project.visibility === "private"
                    ? `Private · ${memberCount} members`
                    : `${memberCount} members`}
                </p>
              );
            })()}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {/* Settings are reachable for every published project — the
                dialog itself renders relay fields read-only for non-owners.
                Delete stays owner-only. */}
            {!isFallback ? (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    aria-label={`Actions for ${project.name}`}
                    data-testid="project-screen-actions"
                    className="flex size-9 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
                  >
                    <EllipsisVertical className="size-4" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    data-testid="project-screen-edit"
                    onSelect={() => setEditOpen(true)}
                  >
                    <Pencil />
                    Project settings
                  </DropdownMenuItem>
                  {canManage && !isGeneral ? (
                    <DropdownMenuItem
                      className="text-destructive focus:text-destructive"
                      data-testid="project-screen-delete"
                      onSelect={() => setDeleteOpen(true)}
                    >
                      <Trash2 />
                      Delete project
                    </DropdownMenuItem>
                  ) : null}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
          </div>
        </header>

        {/* Pulse is a section of the project, not a separate screen. Same two
            gates as the card below: the preview flag, and a real project head
            — the local General placeholder has no coordinate to read against,
            so it gets no tab strip at all. */}
        <ProjectPageTabs
          active={tab}
          projectId={project.id}
          showPulse={pulseEnabled && !isFallback}
        />

        {tab === "pulse" && pulseEnabled && !isFallback ? (
          <div data-testid="project-pulse-tab">
            <ProjectPulseScreen embedded projectId={project.id} />
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            {/* The local General placeholder has no coordinate to manage a
              roster against — the card appears once the real head exists. */}
            {!isFallback ? <ProjectMembersCard project={project} /> : null}

            <SectionCard
              count={codingSessions.length}
              icon={<Terminal className="size-4" />}
              title="Coding sessions"
              action={actionIconButton(
                "New coding session",
                "project-section-create-coding-session",
                () => void goNewProjectCodingSession(project.id),
              )}
            >
              {codingSessions.length === 0 ? (
                <EmptyHint>
                  {sessionBuckets.state.kind === "ready"
                    ? "No coding sessions in this project."
                    : sessionBuckets.state.message}
                </EmptyHint>
              ) : (
                <ul className="flex flex-col gap-1">
                  {codingSessions.map((entry) => (
                    <li key={`${entry.channelId}:${entry.generationId}`}>
                      <Button
                        className="h-8 w-full justify-start gap-2 px-2"
                        data-session-status={entry.status.kind}
                        data-testid="project-screen-coding-session-row"
                        onClick={() => {
                          // A founded row opens the founded route; its id in
                          // the generation slot is never a generation.
                          const target =
                            resolveProjectCodingSessionOpenTarget(entry);
                          if (target.kind === "founded") {
                            void goFoundedCodingSession(
                              target.channelId,
                              target.sessionRef,
                            );
                          } else {
                            void goCodingSession(
                              target.channelId,
                              target.generationId,
                            );
                          }
                        }}
                        variant="ghost"
                      >
                        <Terminal className="size-4 shrink-0 text-muted-foreground" />
                        <span className="truncate">{entry.label}</span>
                        <span className="ml-auto shrink-0 text-2xs text-muted-foreground">
                          {[entry.runtimeLabel, entry.status.label]
                            .filter(Boolean)
                            .join(" · ")}
                        </span>
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </SectionCard>

            {/* Pulse sits between the sessions it describes and the agents that
              run them. Gated twice: the preview flag, and a real project head —
              the local General placeholder has no coordinate, so its Pulse
              could only ever be a screen that never loads. "Open Pulse"
              switches to the Pulse tab above. */}
            {!isFallback ? (
              <FeatureGate feature="project-pulse">
                <ProjectPulseCard
                  onOpenPulse={() =>
                    void navigate({
                      to: "/projects/$projectId",
                      params: { projectId: project.id },
                      search: { tab: "pulse" },
                    })
                  }
                  project={project}
                />
              </FeatureGate>
            ) : null}

            <SectionCard
              count={agents.length}
              icon={<Bot className="size-4" />}
              title="Agents"
            >
              {agents.length === 0 ? (
                <EmptyHint>No agents in this project.</EmptyHint>
              ) : (
                <ul className="flex flex-col gap-1">
                  {agents.map((agent) => (
                    <li key={agent.key}>
                      <Button
                        className="h-8 w-full justify-start gap-2 px-2"
                        onClick={() => void goAgents()}
                        variant="ghost"
                      >
                        <Bot className="size-4 shrink-0 text-muted-foreground" />
                        <span className="truncate">{agent.label}</span>
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </SectionCard>

            <ProjectTodosCard project={project} />

            <FeatureGate feature="builtin-shell">
              <ProjectTerminalsCard
                projectAddress={
                  project.id === LOCAL_GENERAL_ID ? null : project.address
                }
                canDelete={capabilities.canDeleteResource}
                isFallback={project.id === LOCAL_GENERAL_ID}
                repos={repos}
              />
            </FeatureGate>

            <SectionCard
              count={streamChannels.length}
              icon={<Hash className="size-4" />}
              title="Channels"
              action={createButton("channel", "New channel")}
            >
              {streamChannels.length === 0 ? (
                <EmptyHint>No channels in this project.</EmptyHint>
              ) : (
                <ul className="flex flex-col gap-1">
                  {streamChannels.map((channel) =>
                    channelRow(
                      channel,
                      <Hash className="size-4 shrink-0 text-muted-foreground" />,
                      (target) =>
                        moves.requestMoveChannel(channel, project.id, target),
                    ),
                  )}
                </ul>
              )}
            </SectionCard>

            <SectionCard
              count={repos.length}
              icon={<FolderGit2 className="size-4" />}
              title="Code"
              action={
                <ProjectSectionRepoAddMenu
                  attachAvailable={attachableRepoCount > 0}
                  onAttachExisting={() => setCreateKind("repo-attach")}
                  onCreateNew={() => setCreateKind("repo")}
                  onImportLocal={() => setCreateKind("repo-import")}
                />
              }
            >
              {repos.length === 0 ? (
                <EmptyHint>No repositories in this project.</EmptyHint>
              ) : (
                <ul className="flex flex-col gap-1">
                  {repos.map((repo) => (
                    <li
                      className="group/manage-row flex items-center gap-1"
                      data-testid="project-screen-item-row"
                      key={repo.repoAddress}
                    >
                      <Button
                        className="h-8 min-w-0 flex-1 justify-start gap-2 px-2"
                        onClick={() =>
                          void goProjectRepo(projectId, repo.repoAddress)
                        }
                        variant="ghost"
                      >
                        <FolderGit2 className="size-4 shrink-0 text-muted-foreground" />
                        <span className="truncate">{repo.name}</span>
                      </Button>
                      <MoveToProjectMenu
                        currentId={project.id}
                        deleteLabel="Delete repository"
                        projects={displayProjects}
                        onDelete={
                          capabilities.canDeleteResource(repo.owner)
                            ? () => repoDelete.requestDelete(repo)
                            : undefined
                        }
                        onMove={(target) =>
                          moves.requestMoveRepo(repo, project.id, target)
                        }
                        onLinkLocal={() => setLinkRepo(repo)}
                      />
                    </li>
                  ))}
                </ul>
              )}
            </SectionCard>

            {workflowsEnabled ? (
              <SectionCard
                count={workflows.length}
                icon={<Zap className="size-4" />}
                title="Workflows"
                action={createButton("workflow", "New workflow")}
              >
                {workflows.length === 0 ? (
                  <EmptyHint>No workflows in this project.</EmptyHint>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {workflows.map((workflow) => (
                      <li key={workflow.id}>
                        <Button
                          className="h-8 w-full justify-start gap-2 px-2"
                          onClick={() => void goWorkflow(workflow.id)}
                          variant="ghost"
                        >
                          <Zap className="size-4 shrink-0 text-muted-foreground" />
                          <span className="truncate">{workflow.name}</span>
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
              </SectionCard>
            ) : null}

            <SectionCard
              count={forums.length}
              icon={<FileText className="size-4" />}
              title="Forums"
              action={
                <FeatureGate feature="forum">
                  {createButton("forum", "New forum")}
                </FeatureGate>
              }
            >
              {forums.length === 0 ? (
                <EmptyHint>No forums in this project.</EmptyHint>
              ) : (
                <ul className="flex flex-col gap-1">
                  {forums.map((forum) =>
                    channelRow(
                      forum,
                      <FileText className="size-4 shrink-0 text-muted-foreground" />,
                      (target) =>
                        moves.requestMoveForum(forum, project.id, target),
                    ),
                  )}
                </ul>
              )}
            </SectionCard>
          </div>
        )}
      </div>

      <ProjectSettingsDialog
        isSaving={updateMutation.isPending}
        onOpenChange={(open) => {
          if (!open) setEditOpen(false);
        }}
        onSave={async (input) => {
          await updateMutation.mutateAsync({ project, ...input });
          toast.success("Project updated.");
        }}
        project={editOpen ? project : null}
      />

      <DeleteProjectDialog
        project={deleteOpen ? project : null}
        onOpenChange={(open) => {
          if (!open) setDeleteOpen(false);
        }}
        onDeleted={() => void goProjects({ filter: "projects" })}
      />

      {moves.confirmDialog}
      {repoDelete.dialog}

      <ProjectsScreenCreateDialogs
        kind={createKind}
        targetProject={project}
        onClose={() => setCreateKind(null)}
      />

      <LinkProjectRepoDialog
        cloneUrl={linkRepo ? linkedRepoCloneUrl(linkRepo, relayOrigin) : null}
        isLinking={linkMutation.isPending}
        onLink={async (input) => {
          if (!linkRepo) return;
          const result = await linkMutation.mutateAsync({
            repo: linkRepo,
            relayOrigin,
            ...input,
          });
          toast.success(`Linked ${result.name} to ${result.path}.`);
        }}
        onOpenChange={(open) => {
          if (!open) setLinkRepo(null);
        }}
        open={linkRepo !== null}
        repoName={linkRepo?.name ?? ""}
      />
    </div>
  );
}
