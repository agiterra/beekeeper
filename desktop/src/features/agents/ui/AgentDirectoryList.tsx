import * as React from "react";
import { CircleAlert, Plus } from "lucide-react";

import {
  installationsForProject,
  type AgentDirectoryFilters,
  type AgentDirectoryRow,
  type AgentDirectoryStatusFilter,
} from "@/features/agents/lib/agentDirectoryModel";
import type { ProjectContainer } from "@/features/projects-container/hooks";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import {
  AGENT_DIRECTORY_ADD_LABEL,
  AGENT_DIRECTORY_ADD_TESTID,
  AGENT_DIRECTORY_EMPTY,
  AGENT_DIRECTORY_EMPTY_TESTID,
  AGENT_DIRECTORY_ERROR_TESTID,
  AGENT_DIRECTORY_FILTERED_EMPTY,
  AGENT_DIRECTORY_FILTERED_EMPTY_TESTID,
  AGENT_DIRECTORY_INSTALLATIONS_ERROR_TESTID,
  AGENT_DIRECTORY_LOADING_ARIA,
  AGENT_DIRECTORY_LOADING_TESTID,
  AGENT_DIRECTORY_SEAT_NOTICE_TESTID,
  AGENT_FILTERS_TESTID,
  AGENT_FILTER_ALL_ROLES,
  AGENT_FILTER_ANY_PROJECT,
  AGENT_FILTER_ANY_STATUS,
  AGENT_FILTER_INSTALLED_HELPER,
  AGENT_FILTER_INSTALLED_LABEL,
  AGENT_FILTER_INSTALLED_TESTID,
  AGENT_FILTER_PROJECT_GROUP_LABEL,
  AGENT_FILTER_PROJECT_TESTID,
  AGENT_FILTER_ROLE_TESTID,
  AGENT_FILTER_STATUS_NOT_SEATED,
  AGENT_FILTER_STATUS_PROJECT_AGENT,
  AGENT_FILTER_STATUS_RUNNING,
  AGENT_FILTER_STATUS_SEATED,
  AGENT_FILTER_STATUS_STOPPED,
  AGENT_FILTER_STATUS_TESTID,
  AGENT_INSTALLED_HERE_NO,
  AGENT_ROW_LAUNCHES_AS_TESTID,
  AGENT_ROW_NAME_TESTID,
  AGENT_ROW_NOT_INSTALLED_TESTID,
  AGENT_ROW_PACK_TESTID,
  AGENT_ROW_PROJECT_TESTID,
  AGENT_ROW_ROLE_HISTORY_TESTID,
  AGENT_ROW_SEAT_TESTID,
  AGENT_ROW_TESTID,
  AGENT_ROW_UNASSOCIATED_TESTID,
  agentDirectoryErrorText,
  agentProjectText,
  currentSeatText,
  installationsErrorText,
  launchesAsText,
  seatNoticeText,
  unassociatedInstallationText,
} from "./agentDirectoryCopy";
import {
  AgentDirectoryRenameButton,
  AgentDirectoryRenameForm,
} from "./AgentDirectoryRename";
import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_SHARED_HOME_LABEL,
} from "./AgentHomeRoleBadges";

const SELECT_CLASS =
  "h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50";

function agentRowPackText(row: AgentDirectoryRow): string | null {
  if (row.packRefusedSharedHome === true) return AGENT_SHARED_HOME_LABEL;
  if (row.hasRolePack === false) return AGENT_NO_ROLE_PACK_LABEL;
  return null;
}

function AddAgentButton({ onAddAgent }: { onAddAgent: () => void }) {
  return (
    <Button
      data-testid={AGENT_DIRECTORY_ADD_TESTID}
      onClick={onAddAgent}
      size="sm"
      type="button"
      variant="outline"
    >
      <Plus />
      {AGENT_DIRECTORY_ADD_LABEL}
    </Button>
  );
}

function AgentDirectoryRowItem({
  row,
  isSelected,
  seatUnknown,
  projectId,
  onSelect,
}: {
  row: AgentDirectoryRow;
  isSelected: boolean;
  seatUnknown: boolean;
  projectId: string | null;
  onSelect: (pubkey: string) => void;
}) {
  const [isRenaming, setIsRenaming] = React.useState(false);
  const canRename = canRenameFromRow(row);
  return (
    <div className="relative flex flex-col gap-2">
      <AgentDirectoryRowButton
        isSelected={isSelected}
        onSelect={onSelect}
        projectId={projectId}
        reserveActionSpace={canRename && !isRenaming}
        row={row}
        seatUnknown={seatUnknown}
      />
      {canRename && !isRenaming ? (
        <div className="absolute right-2 top-2">
          <AgentDirectoryRenameButton
            name={row.name}
            onClick={() => setIsRenaming(true)}
          />
        </div>
      ) : null}
      {canRename && isRenaming ? (
        <AgentDirectoryRenameForm
          name={row.name}
          onDone={() => setIsRenaming(false)}
          pubkey={row.pubkey}
        />
      ) : null}
    </div>
  );
}

function AgentDirectoryListSkeleton() {
  return (
    <div
      aria-label={AGENT_DIRECTORY_LOADING_ARIA}
      className="flex flex-col gap-2"
      data-testid={AGENT_DIRECTORY_LOADING_TESTID}
      role="status"
    >
      {["first", "second", "third"].map((key) => (
        <Skeleton className="h-14 w-full rounded-lg" key={key} />
      ))}
    </div>
  );
}

/**
 * Any agent with a managed record on this computer can be renamed from its
 * row (native `update_managed_agent` needs that record). A wire-only agent is
 * not this computer's to rename.
 */
function canRenameFromRow(row: AgentDirectoryRow): boolean {
  return row.isInstalled;
}

function AgentDirectoryRowButton({
  row,
  isSelected,
  seatUnknown,
  projectId,
  onSelect,
  reserveActionSpace,
}: {
  row: AgentDirectoryRow;
  isSelected: boolean;
  seatUnknown: boolean;
  /** The directory's selected project, so its installation is named first. */
  projectId: string | null;
  onSelect: (pubkey: string) => void;
  /** Leave room at the top right for the row's rename action. */
  reserveActionSpace: boolean;
}) {
  const packText = agentRowPackText(row);
  const matching = installationsForProject(
    { installedProjects: row.unassociatedInstallations },
    projectId,
  );
  const unassociatedText = unassociatedInstallationText(
    matching.length > 0
      ? [
          ...matching,
          ...row.unassociatedInstallations.filter(
            (entry) => !matching.includes(entry),
          ),
        ]
      : row.unassociatedInstallations,
  );
  return (
    <button
      className={`flex w-full flex-col gap-1 rounded-lg border px-4 py-3 text-left transition-colors hover:bg-muted/60 ${
        isSelected ? "border-ring bg-muted/40" : "border-border"
      } ${reserveActionSpace ? "pr-28" : ""}`}
      data-testid={AGENT_ROW_TESTID}
      data-pubkey={row.pubkey}
      onClick={() => onSelect(row.pubkey)}
      type="button"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="font-medium" data-testid={AGENT_ROW_NAME_TESTID}>
          {row.name}
        </span>
        <span className="text-sm text-muted-foreground">{row.modelLabel}</span>
        <span
          className="text-sm text-muted-foreground"
          data-testid={AGENT_ROW_LAUNCHES_AS_TESTID}
        >
          {launchesAsText(row.homeRole)}
        </span>
        {!row.isInstalled ? (
          <span
            className="text-xs text-muted-foreground"
            data-testid={AGENT_ROW_NOT_INSTALLED_TESTID}
          >
            {AGENT_INSTALLED_HERE_NO}
          </span>
        ) : null}
      </div>
      <span
        className="min-w-0 break-words text-sm text-foreground"
        data-project-ref={row.project?.projectRef ?? undefined}
        data-testid={AGENT_ROW_PROJECT_TESTID}
      >
        {agentProjectText(
          row.project,
          row.homeRole,
          row.projectKnown,
          row.carriedFromAnotherComputer ?? false,
        )}
      </span>
      {unassociatedText ? (
        <span
          className="min-w-0 break-words text-sm text-amber-800 dark:text-amber-400"
          data-testid={AGENT_ROW_UNASSOCIATED_TESTID}
        >
          {unassociatedText}
        </span>
      ) : null}
      {row.needsRestart ? (
        <span
          className="text-sm text-amber-800 dark:text-amber-400"
          data-testid={`agent-needs-restart-${row.pubkey}`}
        >
          Restart to apply changes
        </span>
      ) : null}
      {row.lastError ? (
        <span
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid={`agent-runtime-error-${row.pubkey}`}
          title={row.lastError}
        >
          <CircleAlert className="size-4 shrink-0" aria-hidden="true" />
          {row.lastError}
        </span>
      ) : null}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
        <span data-testid={AGENT_ROW_ROLE_HISTORY_TESTID}>
          {row.roleHistoryLabel}
        </span>
        <span
          data-seat-status={row.currentSeat?.status ?? undefined}
          data-testid={AGENT_ROW_SEAT_TESTID}
        >
          {currentSeatText(row.currentSeat, { seatUnknown })}
        </span>
        {packText ? (
          <span data-testid={AGENT_ROW_PACK_TESTID}>{packText}</span>
        ) : null}
      </div>
    </button>
  );
}

export function AgentDirectoryList({
  rows,
  allRows,
  projects,
  filters,
  onFiltersChange,
  selectedPubkey,
  onSelectRow,
  onAddAgent,
  isLoading,
  error,
  seatNotice,
  installationsError = null,
}: {
  rows: AgentDirectoryRow[];
  /** Unfiltered rows, used to compute filter option lists. */
  allRows: AgentDirectoryRow[];
  projects: ProjectContainer[];
  filters: AgentDirectoryFilters;
  onFiltersChange: (next: AgentDirectoryFilters) => void;
  selectedPubkey: string | null;
  onSelectRow: (pubkey: string) => void;
  /** Opens the agent catalog dialog — the directory's only creation entry. */
  onAddAgent: () => void;
  isLoading: boolean;
  error: string | null;
  seatNotice: { message: string; detail: string } | null;
  /** The setup-journal read failed; missing-association warnings are absent. */
  installationsError?: string | null;
}) {
  const roleOptions = React.useMemo(() => {
    const slugs = new Set<string>();
    for (const row of allRows) {
      for (const slug of row.roleSlugs) slugs.add(slug);
    }
    return [...slugs].sort();
  }, [allRows]);

  if (isLoading) {
    return <AgentDirectoryListSkeleton />;
  }

  if (error) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        data-testid={AGENT_DIRECTORY_ERROR_TESTID}
        role="alert"
      >
        {agentDirectoryErrorText(error)}
      </div>
    );
  }

  if (allRows.length === 0) {
    return (
      <div
        className="flex flex-col items-center gap-3 rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground"
        data-testid={AGENT_DIRECTORY_EMPTY_TESTID}
      >
        {AGENT_DIRECTORY_EMPTY}
        <AddAgentButton onAddAgent={onAddAgent} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div
        className="flex flex-wrap items-center gap-3"
        data-testid={AGENT_FILTERS_TESTID}
      >
        <select
          className={SELECT_CLASS}
          data-testid={AGENT_FILTER_ROLE_TESTID}
          onChange={(event) =>
            onFiltersChange({
              ...filters,
              role: event.target.value === "" ? null : event.target.value,
            })
          }
          value={filters.role ?? ""}
        >
          <option value="">{AGENT_FILTER_ALL_ROLES}</option>
          {roleOptions.map((slug) => (
            <option key={slug} value={slug}>
              {slug}
            </option>
          ))}
        </select>

        <select
          className={SELECT_CLASS}
          data-testid={AGENT_FILTER_STATUS_TESTID}
          onChange={(event) =>
            onFiltersChange({
              ...filters,
              status: event.target.value as AgentDirectoryStatusFilter,
            })
          }
          value={filters.status}
        >
          <option value="any">{AGENT_FILTER_ANY_STATUS}</option>
          <option value="running">{AGENT_FILTER_STATUS_RUNNING}</option>
          <option value="stopped">{AGENT_FILTER_STATUS_STOPPED}</option>
          <option value="seated">{AGENT_FILTER_STATUS_SEATED}</option>
          <option value="not-seated">{AGENT_FILTER_STATUS_NOT_SEATED}</option>
          <option value="project-agent">
            {AGENT_FILTER_STATUS_PROJECT_AGENT}
          </option>
        </select>

        <select
          className={SELECT_CLASS}
          data-testid={AGENT_FILTER_PROJECT_TESTID}
          onChange={(event) =>
            onFiltersChange({
              ...filters,
              projectId: event.target.value === "" ? null : event.target.value,
            })
          }
          value={filters.projectId ?? ""}
        >
          <option value="">{AGENT_FILTER_ANY_PROJECT}</option>
          <optgroup label={AGENT_FILTER_PROJECT_GROUP_LABEL}>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </optgroup>
        </select>

        <label className="flex items-center gap-2 text-sm">
          <input
            checked={filters.installedOnly}
            data-testid={AGENT_FILTER_INSTALLED_TESTID}
            onChange={(event) =>
              onFiltersChange({
                ...filters,
                installedOnly: event.target.checked,
              })
            }
            type="checkbox"
          />
          {AGENT_FILTER_INSTALLED_LABEL}
        </label>
        {!filters.installedOnly ? (
          <span className="text-xs text-muted-foreground">
            {AGENT_FILTER_INSTALLED_HELPER}
          </span>
        ) : null}

        <div className="ml-auto">
          <AddAgentButton onAddAgent={onAddAgent} />
        </div>
      </div>

      {installationsError ? (
        <div
          className="rounded-lg border border-warning/40 bg-warning/10 px-4 py-2 text-sm"
          data-testid={AGENT_DIRECTORY_INSTALLATIONS_ERROR_TESTID}
        >
          {installationsErrorText(installationsError)}
        </div>
      ) : null}

      {seatNotice ? (
        <div
          className="rounded-lg border border-warning/40 bg-warning/10 px-4 py-2 text-sm"
          data-testid={AGENT_DIRECTORY_SEAT_NOTICE_TESTID}
        >
          {seatNoticeText(seatNotice.message, seatNotice.detail)}
        </div>
      ) : null}

      {rows.length === 0 ? (
        <div
          className="rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground"
          data-testid={AGENT_DIRECTORY_FILTERED_EMPTY_TESTID}
        >
          {AGENT_DIRECTORY_FILTERED_EMPTY}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {rows.map((row) => (
            <AgentDirectoryRowItem
              isSelected={row.pubkey === selectedPubkey}
              key={row.pubkey}
              onSelect={onSelectRow}
              projectId={filters.projectId}
              row={row}
              seatUnknown={seatNotice !== null && row.currentSeat === null}
            />
          ))}
        </div>
      )}
    </div>
  );
}
