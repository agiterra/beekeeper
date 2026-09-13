import * as React from "react";
import { UnifiedAgentsSection } from "./UnifiedAgentsSection";
import { useBakedBuildEnvQuery } from "@/features/agents/hooks";
import { getInheritedAgentDefaults } from "./bakedEnvHelpers";
import { EllipsisVertical, OctagonX, Settings2 } from "lucide-react";
import {
  consumePendingSnapshotImport,
  subscribeSnapshotImport,
} from "@/features/agents/openSnapshotImportFromUrlEvent";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { AddAgentToChannelDialog } from "./AddAgentToChannelDialog";
import { AddTeamToChannelDialog } from "./AddTeamToChannelDialog";
import { AgentDefaultsDialog } from "./AgentDefaultsDialog";
import { AgentDialog } from "./AgentDialog";
import { AgentDirectoryDetail } from "./AgentDirectoryDetail";
import { AgentDirectoryList } from "./AgentDirectoryList";
import { PersonaCatalogDialog } from "./PersonaCatalogDialog";
import { PersonaDeleteDialog } from "./PersonaDeleteDialog";
import { PersonaShareDialog } from "./PersonaShareDialog";
import { AgentSnapshotExportDialog } from "./AgentSnapshotExportDialog";
import { AgentSnapshotImportDialog } from "./AgentSnapshotImportDialog";
import { TeamSnapshotExportDialog } from "./TeamSnapshotExportDialog";
import { TeamSnapshotImportDialog } from "./TeamSnapshotImportDialog";
import { TeamShareDialog } from "./TeamShareDialog";
import { TeamDeleteDialog } from "./TeamDeleteDialog";
import { TeamDialog } from "./TeamDialog";
import { InstallCrewRolesDialog } from "./InstallCrewRolesDialog";
import { RolePacksProjectSelector } from "./RolePacksProjectSelector";
import { TeamsSection } from "./TeamsSection";
import { useRolePacksProject } from "./useRolePacksProject";
import { useManagedAgentActions } from "./useManagedAgentActions";
import { usePersonaActions } from "./usePersonaActions";
import { useTeamActions } from "./useTeamActions";
import { useProfilePanel } from "@/shared/context/ProfilePanelContext";
import {
  agentDirectoryFilter,
  type AgentDirectoryFilters,
} from "@/features/agents/lib/agentDirectoryModel";
import { useAgentDirectory } from "@/features/agents/lib/useAgentDirectory";
import { isManagedAgentActive } from "@/features/agents/lib/managedAgentControlActions";
import { useGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { normalizePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { PageHeader } from "@/shared/ui/PageHeader";
import {
  crewRolesInstalledToast,
  INSTALL_CREW_ROLES_MENU_LABEL,
  rolePacksProjectFallbackNote,
} from "./installCrewRolesCopy";
import {
  AGENTS_PROJECT_ROLES_DESCRIPTION,
  AGENTS_PROJECT_ROLES_TESTID,
  INSTALL_PROJECT_ROLES_TESTID,
  SAVED_AGENT_GROUPS_DESCRIPTION,
  SAVED_AGENT_GROUPS_TESTID,
  SAVED_AGENT_GROUPS_TITLE,
} from "./agentDirectoryCopy";

const DEFAULT_AGENT_DIRECTORY_FILTERS: AgentDirectoryFilters = {
  role: null,
  status: "any",
  projectId: null,
  installedOnly: true,
};

export function AgentsView() {
  const { openPersonaProfilePanel, openProfilePanel } = useProfilePanel();
  const { globalConfig } = useGlobalAgentConfig();
  const { data: bakedEnv } = useBakedBuildEnvQuery({ enabled: true });
  const inheritedDefaults = getInheritedAgentDefaults(globalConfig, bakedEnv);
  const agents = useManagedAgentActions();
  const personas = usePersonaActions();
  const teamImportInputRef = React.useRef<HTMLInputElement | null>(null);
  const aiDefaultsTriggerRef = React.useRef<HTMLButtonElement>(null);
  const fullAiDefaultsTriggerRef = React.useRef<HTMLButtonElement>(null);
  const compactActionsTriggerRef = React.useRef<HTMLButtonElement>(null);
  const [isAiDefaultsOpen, setIsAiDefaultsOpen] = React.useState(false);
  const [isInstallCrewRolesOpen, setIsInstallCrewRolesOpen] =
    React.useState(false);
  const { goCodingSession } = useAppNavigation();
  const directory = useAgentDirectory();
  const [selectedPubkey, setSelectedPubkey] = React.useState<string | null>(
    null,
  );
  const [directoryFilters, setDirectoryFilters] =
    React.useState<AgentDirectoryFilters>(DEFAULT_AGENT_DIRECTORY_FILTERS);
  // Ledger 85: the installer opens on the project's own `personas/roles`
  // folder. There is ONE selected project on this page — the directory's
  // project filter — and the role-pack selector reads and writes that same
  // value from the same project list. On "Any project" a project is resolved
  // from what the app already records, and the selector says it fell back.
  // With no project at all nothing is resolved and the dialog is unchanged.
  const rolePacksProject = useRolePacksProject({
    projects: directory.projects,
    selectedProjectId: directoryFilters.projectId,
  });
  const activeProject = rolePacksProject.project;
  const chooseProject = React.useCallback((projectId: string) => {
    setDirectoryFilters((current) => ({ ...current, projectId }));
  }, []);
  const filteredDirectoryRows = React.useMemo(
    () => agentDirectoryFilter(directory.rows, directoryFilters),
    [directory.rows, directoryFilters],
  );
  const selectedDirectoryRow = React.useMemo(
    () => directory.rows.find((row) => row.pubkey === selectedPubkey) ?? null,
    [directory.rows, selectedPubkey],
  );
  const selectedManagedAgent = React.useMemo(() => {
    if (!selectedDirectoryRow) return null;
    return (
      agents.managedAgents.find(
        (agent) =>
          normalizePubkey(agent.pubkey) === selectedDirectoryRow.pubkey,
      ) ?? null
    );
  }, [agents.managedAgents, selectedDirectoryRow]);
  const selectedEditablePersona = React.useMemo(() => {
    if (!selectedManagedAgent?.personaId) return null;
    return (
      personas.libraryPersonas.find(
        (persona) => persona.id === selectedManagedAgent.personaId,
      ) ?? null
    );
  }, [personas.libraryPersonas, selectedManagedAgent]);

  // The directory's "Add agent" button opens exactly what the old agents
  // section opened: the catalog dialog, whose create tab is `AgentDialog` in
  // `mode="definition"`. No new dialog was introduced for it.
  function openAgentCatalog() {
    personas.prepareCreate();
    personas.openCatalog();
  }

  function openAiDefaults(trigger: HTMLButtonElement | null) {
    aiDefaultsTriggerRef.current = trigger;
    setIsAiDefaultsOpen(true);
  }

  function setAiDefaultsDialogOpen(open: boolean) {
    if (!open) {
      aiDefaultsTriggerRef.current =
        fullAiDefaultsTriggerRef.current?.offsetParent !== null
          ? fullAiDefaultsTriggerRef.current
          : compactActionsTriggerRef.current;
    }
    setIsAiDefaultsOpen(open);
  }

  const teamActions = useTeamActions(
    {
      setActionNoticeMessage: agents.setActionNoticeMessage,
      setActionErrorMessage: agents.setActionErrorMessage,
    },
    {
      refetchManagedAgents: agents.refetchManagedAgents,
      refetchRelayAgents: agents.refetchRelayAgents,
    },
  );

  const isActionPending =
    agents.isPending ||
    personas.isPending ||
    teamActions.createTeamMutation.isPending ||
    teamActions.updateTeamMutation.isPending ||
    teamActions.deleteTeamMutation.isPending;
  const runningAgentCount = agents.managedAgents.filter((agent) =>
    isManagedAgentActive(agent),
  ).length;
  const hasSavedAgentDefaults = Boolean(
    globalConfig.preferred_runtime?.trim() ||
      globalConfig.provider?.trim() ||
      globalConfig.model?.trim() ||
      Object.values(globalConfig.env_vars).some(
        (value) => value.trim().length > 0,
      ),
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: mount-only; personas.handleImportSnapshotFile and teamActions.handleImportTeamSnapshotFile are stable
  React.useEffect(() => {
    // Consume a snapshot import that was enqueued before navigation (e.g. from
    // a timeline AgentSnapshotCard click that navigated here).
    const pending = consumePendingSnapshotImport();
    if (pending) {
      if (pending.snapshotKind === "team") {
        void teamActions.handleImportTeamSnapshotFile(
          pending.fileBytes,
          pending.fileName,
        );
      } else {
        void personas.handleImportSnapshotFile(
          pending.fileBytes,
          pending.fileName,
        );
      }
    }

    return subscribeSnapshotImport(({ fileBytes, fileName, snapshotKind }) => {
      if (snapshotKind === "team") {
        void teamActions.handleImportTeamSnapshotFile(fileBytes, fileName);
      } else {
        void personas.handleImportSnapshotFile(fileBytes, fileName);
      }
    });
  }, []);

  return (
    <>
      <div
        className="relative flex min-h-0 flex-1 overflow-hidden"
        data-testid="agents-view"
      >
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden overscroll-contain px-4 py-7 sm:px-6 sm:py-8">
          <div
            className="mx-auto w-full max-w-6xl space-y-8 [container-type:inline-size]"
            data-testid="agents-page-content"
          >
            <PageHeader
              action={
                <>
                  <div className="flex flex-wrap justify-end gap-2 [@container(max-width:40rem)]:hidden">
                    <Button
                      data-testid="agent-defaults-button"
                      ref={fullAiDefaultsTriggerRef}
                      onClick={(event) => openAiDefaults(event.currentTarget)}
                      size="sm"
                      variant="outline"
                    >
                      <Settings2 />
                      {hasSavedAgentDefaults
                        ? "Agent defaults"
                        : "Set agent defaults"}
                    </Button>
                    {runningAgentCount > 0 ? (
                      <Button
                        disabled={isActionPending}
                        onClick={() => {
                          void agents.handleBulkStopRunning();
                        }}
                        size="sm"
                        variant="outline"
                      >
                        <OctagonX />
                        Stop running agents
                      </Button>
                    ) : null}
                  </div>

                  <DropdownMenu modal={false}>
                    <DropdownMenuTrigger asChild>
                      <Button
                        aria-label="Agent actions"
                        className="hidden [@container(max-width:40rem)]:inline-flex"
                        data-testid="agent-actions-menu-trigger"
                        ref={compactActionsTriggerRef}
                        size="icon"
                        type="button"
                        variant="outline"
                      >
                        <EllipsisVertical />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onSelect={() => {
                          openAiDefaults(compactActionsTriggerRef.current);
                        }}
                      >
                        <Settings2 />
                        {hasSavedAgentDefaults
                          ? "Agent defaults"
                          : "Set agent defaults"}
                      </DropdownMenuItem>
                      {runningAgentCount > 0 ? (
                        <DropdownMenuItem
                          disabled={isActionPending}
                          onSelect={() => {
                            void agents.handleBulkStopRunning();
                          }}
                        >
                          <OctagonX />
                          Stop running agents
                        </DropdownMenuItem>
                      ) : null}
                    </DropdownMenuContent>
                  </DropdownMenu>
                </>
              }
              description="Set up and manage your agents."
              title="Agents"
            />
            <div className="flex flex-col gap-8">
              <section
                className="flex flex-col gap-2"
                data-testid={AGENTS_PROJECT_ROLES_TESTID}
              >
                <div className="flex flex-wrap items-center gap-3">
                  <RolePacksProjectSelector
                    fallbackNote={rolePacksProjectFallbackNote(
                      rolePacksProject.source,
                      directoryFilters.projectId !== null,
                    )}
                    onSelect={chooseProject}
                    project={activeProject}
                    projects={rolePacksProject.projects}
                  />
                  <Button
                    data-testid={INSTALL_PROJECT_ROLES_TESTID}
                    disabled={isActionPending}
                    onClick={() => setIsInstallCrewRolesOpen(true)}
                    size="sm"
                    type="button"
                    variant="outline"
                  >
                    {INSTALL_CREW_ROLES_MENU_LABEL}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  {AGENTS_PROJECT_ROLES_DESCRIPTION}
                </p>
              </section>

              <AgentDirectoryList
                allRows={directory.rows}
                error={directory.error}
                filters={directoryFilters}
                installationsError={directory.installationsError}
                isLoading={directory.isLoading}
                onAddAgent={openAgentCatalog}
                onFiltersChange={setDirectoryFilters}
                onSelectRow={setSelectedPubkey}
                projects={directory.projects}
                rows={filteredDirectoryRows}
                seatNotice={directory.seatNotice}
                selectedPubkey={selectedPubkey}
              />

              <details data-testid="agent-definitions-management">
                <summary className="cursor-pointer text-sm font-medium">
                  Manage definitions
                </summary>
                <p className="my-3 text-sm text-muted-foreground">
                  Edit, share, or delete the saved definitions your agents use.
                </p>
                <UnifiedAgentsSection
                  defaultModel={inheritedDefaults.model.value}
                  actionErrorMessage={agents.actionErrorMessage}
                  actionNoticeMessage={agents.actionNoticeMessage}
                  agents={agents.managedAgents.filter(
                    (agent) => agent.personaId != null,
                  )}
                  agentsError={
                    agents.managedAgentsQuery.error instanceof Error
                      ? agents.managedAgentsQuery.error
                      : null
                  }
                  isActionPending={isActionPending}
                  isAgentsLoading={agents.managedAgentsQuery.isLoading}
                  startingAgentPubkey={agents.startingAgentPubkey}
                  restartingAgentPubkey={agents.restartingAgentPubkey}
                  startingPersonaIds={agents.startingPersonaIds}
                  onOpenAgentProfile={(pubkey, options) => {
                    openProfilePanel?.(pubkey, options);
                  }}
                  onOpenPersonaProfile={(persona) => {
                    openPersonaProfilePanel?.(persona);
                  }}
                  onStartAgent={(pubkey) => {
                    void agents.handleStart(pubkey);
                  }}
                  onRestartAgent={(pubkey) => {
                    void agents.handleRestart(pubkey);
                  }}
                  onStartPersona={(persona) => {
                    void agents.handleStartPersona(persona);
                  }}
                  // Persona props
                  personas={personas.libraryPersonas}
                  personasError={
                    personas.personasQuery.error instanceof Error
                      ? personas.personasQuery.error
                      : null
                  }
                  personaFeedbackErrorMessage={
                    personas.personaFeedbackSurface === "library"
                      ? personas.personaErrorMessage
                      : null
                  }
                  personaFeedbackNoticeMessage={
                    personas.personaFeedbackSurface === "library"
                      ? personas.personaNoticeMessage
                      : null
                  }
                  isPersonasLoading={personas.personasQuery.isLoading}
                  isPersonasPending={personas.isPending}
                  onOpenCatalog={openAgentCatalog}
                  onDuplicatePersona={personas.openDuplicate}
                  onEditPersona={personas.openEdit}
                  onSharePersona={personas.openShare}
                  onDeactivatePersona={(persona) => {
                    void personas.handleSetActive(persona, false, "library");
                  }}
                  onDeletePersona={personas.openDelete}
                />
              </details>

              <details data-testid={SAVED_AGENT_GROUPS_TESTID}>
                <summary className="cursor-pointer text-sm font-medium">
                  {SAVED_AGENT_GROUPS_TITLE}
                </summary>
                <p className="my-3 text-sm text-muted-foreground">
                  {SAVED_AGENT_GROUPS_DESCRIPTION}
                </p>
                <TeamsSection
                  error={
                    teamActions.teamsQuery.error instanceof Error
                      ? teamActions.teamsQuery.error
                      : null
                  }
                  isLoading={teamActions.teamsQuery.isLoading}
                  isPending={
                    teamActions.createTeamMutation.isPending ||
                    teamActions.updateTeamMutation.isPending ||
                    teamActions.deleteTeamMutation.isPending
                  }
                  onCreate={teamActions.openCreateDialog}
                  onDelete={teamActions.setTeamToDelete}
                  onDuplicate={teamActions.openDuplicateDialog}
                  onEdit={teamActions.openEditDialog}
                  onAddToChannel={teamActions.setTeamToAddToChannel}
                  onShare={teamActions.openShare}
                  onImport={() => {
                    teamImportInputRef.current?.click();
                  }}
                  onInstallCrewRoles={() => setIsInstallCrewRolesOpen(true)}
                  personas={personas.libraryPersonas}
                  teams={teamActions.teams}
                />
              </details>
            </div>
          </div>
        </div>

        {selectedDirectoryRow ? (
          <div
            className="w-[400px] shrink-0 py-7 pr-4 sm:pr-6"
            key={selectedPubkey}
          >
            <AgentDirectoryDetail
              canEdit={selectedEditablePersona !== null}
              isPending={agents.isPending}
              isRestartPending={
                agents.restartingAgentPubkey === selectedDirectoryRow.pubkey
              }
              isStartPending={
                agents.startingAgentPubkey === selectedDirectoryRow.pubkey
              }
              onClose={() => setSelectedPubkey(null)}
              onEdit={() => {
                if (selectedEditablePersona) {
                  personas.openEdit(selectedEditablePersona);
                }
              }}
              onRestart={() => {
                void agents.handleRestart(selectedDirectoryRow.pubkey);
              }}
              onSelectSeat={(channelId, generationId) => {
                void goCodingSession(channelId, generationId);
              }}
              onStart={() => {
                void agents.handleStart(selectedDirectoryRow.pubkey);
              }}
              onStop={() => {
                void agents.handleStop(selectedDirectoryRow.pubkey);
              }}
              row={selectedDirectoryRow}
            />
          </div>
        ) : null}
      </div>

      <InstallCrewRolesDialog
        onInstalled={(result) => {
          void teamActions.teamsQuery.refetch();
          void agents.refetchManagedAgents();
          void personas.personasQuery.refetch();
          directory.refetchInstallations();
          agents.setActionNoticeMessage(crewRolesInstalledToast(result));
        }}
        onOpenChange={setIsInstallCrewRolesOpen}
        open={isInstallCrewRolesOpen}
        project={
          activeProject
            ? { address: activeProject.address, name: activeProject.name }
            : null
        }
      />

      <AgentDefaultsDialog
        onOpenChange={setAiDefaultsDialogOpen}
        open={isAiDefaultsOpen}
        returnFocusRef={aiDefaultsTriggerRef}
      />

      {agents.agentToAddToChannel ? (
        <AddAgentToChannelDialog
          agent={agents.agentToAddToChannel}
          onAdded={agents.handleAddedToChannel}
          onOpenChange={(open) => {
            if (!open) {
              agents.setAgentToAddToChannel(null);
            }
          }}
          open={agents.agentToAddToChannel !== null}
        />
      ) : null}
      {personas.personaDialogState ? (
        <AgentDialog
          description={personas.personaDialogState.description}
          error={
            personas.updatePersonaMutation.error instanceof Error
              ? personas.updatePersonaMutation.error
              : personas.updatePersonaAndPublishMutation.error instanceof Error
                ? personas.updatePersonaAndPublishMutation.error
                : personas.createPersonaMutation.error instanceof Error
                  ? personas.createPersonaMutation.error
                  : null
          }
          initialValues={personas.personaDialogState.initialValues}
          isPending={personas.isPending}
          mode="definition-edit"
          runtimes={personas.acpRuntimesQuery.data ?? []}
          runtimeCatalogStatus={
            personas.acpRuntimesQuery.isLoading
              ? "loading"
              : personas.acpRuntimesQuery.isError
                ? "error"
                : "ready"
          }
          onOpenChange={(open) => {
            if (!open) {
              personas.setPersonaDialogState(null);
            }
          }}
          onSubmit={(input, options) =>
            personas.handleSubmit(
              input,
              undefined,
              undefined,
              undefined,
              options,
            )
          }
          open={personas.personaDialogState !== null}
          publishCatalogUpdatesOnSave={
            "id" in personas.personaDialogState.initialValues &&
            personas.sharedCatalogPersonaIdSet.has(
              personas.personaDialogState.initialValues.id,
            )
          }
          submitLabel={personas.personaDialogState.submitLabel}
          title={personas.personaDialogState.title}
        />
      ) : null}
      {personas.personaToDelete ? (
        <PersonaDeleteDialog
          instanceCount={
            (agents.managedAgents ?? []).filter(
              (a) => a.personaId === personas.personaToDelete?.id,
            ).length
          }
          onConfirm={(persona) => {
            void personas.handleDelete(persona);
          }}
          onOpenChange={(open) => {
            if (!open) {
              personas.setPersonaToDelete(null);
            }
          }}
          open={personas.personaToDelete !== null}
          persona={personas.personaToDelete}
        />
      ) : null}
      {personas.personaToShare ? (
        <PersonaShareDialog
          catalogShareLevel={personas.getPersonaCatalogShareLevel(
            personas.personaToShare.persona,
          )}
          isPending={personas.isPending}
          linkedAgentPubkey={personas.personaToShare.linkedAgentPubkey}
          effectiveAvatarUrl={personas.personaToShare.effectiveAvatarUrl}
          onCatalogShareLevelChange={(shareLevel) => {
            const shareTarget = personas.personaToShare;
            if (!shareTarget) return;
            void personas.setPersonaCatalogShareLevel(
              shareTarget.persona,
              shareLevel,
            );
          }}
          onExport={() => {
            const shareTarget = personas.personaToShare;
            if (!shareTarget) return;
            personas.setPersonaToShare(null);
            personas.setPersonaToExportSnapshot(shareTarget);
          }}
          onOpenChange={(open) => {
            if (!open) {
              personas.setPersonaToShare(null);
            }
          }}
          open={personas.personaToShare !== null}
          persona={personas.personaToShare.persona}
        />
      ) : null}
      {personas.personaToExportSnapshot ? (
        <AgentSnapshotExportDialog
          agentName={personas.personaToExportSnapshot.persona.displayName}
          isSavePending={personas.isPending}
          open={personas.personaToExportSnapshot !== null}
          linkedAgentPubkey={personas.personaToExportSnapshot.linkedAgentPubkey}
          onSaveFile={(memoryLevel, format) => {
            if (personas.personaToExportSnapshot) {
              personas.handleExportSnapshot(
                personas.personaToExportSnapshot.persona,
                personas.personaToExportSnapshot.linkedAgentPubkey,
                personas.personaToExportSnapshot.effectiveAvatarUrl,
                memoryLevel,
                format,
              );
            }
          }}
          onOpenChange={(open) => {
            if (!open) {
              personas.setPersonaToExportSnapshot(null);
            }
          }}
        />
      ) : null}
      {personas.snapshotImportState ? (
        <AgentSnapshotImportDialog
          open={personas.snapshotImportState !== null}
          preview={personas.snapshotImportState.preview}
          isConfirming={personas.isSnapshotImportConfirming}
          result={personas.snapshotImportResult}
          confirmError={personas.snapshotImportConfirmError}
          onConfirm={(keepAllowlist) => {
            void personas.handleConfirmSnapshotImport(keepAllowlist);
          }}
          onOpenChange={(open) => {
            if (!open) {
              personas.closeSnapshotImportDialog();
            }
          }}
        />
      ) : null}
      {personas.isCatalogDialogOpen ? (
        <PersonaCatalogDialog
          createContent={({ onDirtyChange, onRequestClose }) => (
            <AgentDialog
              definitionError={
                personas.createPersonaMutation.error instanceof Error
                  ? personas.createPersonaMutation.error
                  : null
              }
              embedded
              isDefinitionPending={personas.isPending}
              mode="definition"
              onDirtyChange={onDirtyChange}
              onOpenChange={(open) => {
                if (!open) onRequestClose();
              }}
              onSubmitDefinition={personas.handleSubmit}
              runtimes={personas.acpRuntimesQuery.data ?? []}
              runtimeCatalogStatus={
                personas.acpRuntimesQuery.isLoading
                  ? "loading"
                  : personas.acpRuntimesQuery.isError
                    ? "error"
                    : "ready"
              }
              submitLabel="Add agent"
            />
          )}
          error={
            personas.catalogQuery.error instanceof Error
              ? personas.catalogQuery.error
              : null
          }
          feedbackErrorMessage={
            personas.personaFeedbackSurface === "catalog"
              ? personas.personaErrorMessage
              : null
          }
          feedbackNoticeMessage={
            personas.personaFeedbackSurface === "catalog"
              ? personas.personaNoticeMessage
              : null
          }
          isLoading={personas.catalogQuery.isLoading}
          isPending={personas.isPending}
          onClearFeedback={() => {
            personas.clearFeedback("catalog");
          }}
          onImportFile={(fileBytes, fileName) => {
            void personas.handleImportSnapshotFile(fileBytes, fileName);
          }}
          onOpenChange={personas.setIsCatalogDialogOpen}
          onSelectPersona={async (persona, active) => {
            const addedPersona = await personas.handleSetActive(
              persona,
              active,
              "catalog",
            );
            if (!active || !addedPersona) return;

            personas.setIsCatalogDialogOpen(false);
            openPersonaProfilePanel?.(addedPersona);
          }}
          open={personas.isCatalogDialogOpen}
          personas={personas.catalogPersonas}
        />
      ) : null}
      {teamActions.teamDialogState ? (
        <TeamDialog
          description={teamActions.teamDialogState.description}
          error={
            teamActions.updateTeamMutation.error instanceof Error
              ? teamActions.updateTeamMutation.error
              : teamActions.createTeamMutation.error instanceof Error
                ? teamActions.createTeamMutation.error
                : null
          }
          initialValues={teamActions.teamDialogState.initialValues}
          isPending={
            teamActions.createTeamMutation.isPending ||
            teamActions.updateTeamMutation.isPending
          }
          onOpenChange={(open) => {
            if (!open) {
              teamActions.setTeamDialogState(null);
            }
          }}
          onDeleteRemovedPersonas={teamActions.handleDeleteRemovedPersonas}
          onSubmit={teamActions.handleTeamSubmit}
          open={teamActions.teamDialogState !== null}
          personas={personas.libraryPersonas}
          submitLabel={teamActions.teamDialogState.submitLabel}
          title={teamActions.teamDialogState.title}
        />
      ) : null}
      {teamActions.teamToDelete ? (
        <TeamDeleteDialog
          onConfirm={(team) => {
            void teamActions.handleDeleteTeam(team);
          }}
          onOpenChange={(open) => {
            if (!open) {
              teamActions.setTeamToDelete(null);
            }
          }}
          open={teamActions.teamToDelete !== null}
          team={teamActions.teamToDelete}
        />
      ) : null}
      {teamActions.teamToAddToChannel ? (
        <AddTeamToChannelDialog
          onDeployed={teamActions.handleTeamDeployed}
          onOpenChange={(open) => {
            if (!open) {
              teamActions.setTeamToAddToChannel(null);
            }
          }}
          open={teamActions.teamToAddToChannel !== null}
          personas={personas.libraryPersonas}
          team={teamActions.teamToAddToChannel}
        />
      ) : null}
      {teamActions.teamToShare ? (
        <TeamShareDialog
          isPending={
            teamActions.createTeamMutation.isPending ||
            teamActions.updateTeamMutation.isPending ||
            teamActions.deleteTeamMutation.isPending
          }
          onExport={() => {
            if (teamActions.teamToShare) {
              const team = teamActions.teamToShare;
              teamActions.setTeamToShare(null);
              teamActions.openExportSnapshot(team);
            }
          }}
          onOpenChange={(open) => {
            if (!open) {
              teamActions.setTeamToShare(null);
            }
          }}
          open={teamActions.teamToShare !== null}
          team={teamActions.teamToShare}
        />
      ) : null}
      {teamActions.teamToExport ? (
        <TeamSnapshotExportDialog
          isSavePending={teamActions.exportTeamSnapshotMutation.isPending}
          open={teamActions.teamToExport !== null}
          team={teamActions.teamToExport}
          onSaveFile={(memoryLevel, format) => {
            if (teamActions.teamToExport) {
              teamActions.handleExportTeamSnapshot(
                teamActions.teamToExport,
                memoryLevel,
                format,
              );
            }
          }}
          onOpenChange={(open) => {
            if (!open) {
              teamActions.setTeamToExport(null);
            }
          }}
        />
      ) : null}
      {teamActions.teamSnapshotImportState ? (
        <TeamSnapshotImportDialog
          open={teamActions.teamSnapshotImportState !== null}
          preview={teamActions.teamSnapshotImportState.preview}
          isConfirming={teamActions.isTeamSnapshotImportConfirming}
          result={teamActions.teamSnapshotImportResult}
          confirmError={teamActions.teamSnapshotImportConfirmError}
          onConfirm={(keepAllowlist) => {
            void teamActions.handleConfirmTeamSnapshotImport(keepAllowlist);
          }}
          onOpenChange={(open) => {
            if (!open) {
              teamActions.closeTeamSnapshotImportDialog();
            }
          }}
        />
      ) : null}
      {/* Hidden file input for team snapshot import via file picker */}
      <input
        accept=".team.json,.team.png"
        className="hidden"
        data-testid="team-snapshot-import-input"
        ref={teamImportInputRef}
        type="file"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (!file) return;
          const reader = new FileReader();
          reader.onload = () => {
            const buffer = reader.result as ArrayBuffer;
            const fileBytes = Array.from(new Uint8Array(buffer));
            void teamActions.handleImportTeamSnapshotFile(fileBytes, file.name);
          };
          reader.readAsArrayBuffer(file);
          // Reset so the same file can be picked again.
          e.target.value = "";
        }}
      />
    </>
  );
}
