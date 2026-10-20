import {
  Columns3,
  Filter,
  List,
  Plus,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-react";
import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";

import type {
  PlanningCapabilities,
  PlanningContainer,
} from "../lib/planningTypes";
import {
  DEFAULT_PLANNING_FILTERS,
  type PlanningDisplay,
  type PlanningFilters,
  type PlanningGroupBy,
  type PlanningOrderBy,
  type PlanningScope,
  hasActiveFilters,
} from "../lib/planningView";
import { PriorityGlyph, StatusGlyph } from "./PlanningPickers";

const SCOPES: { id: PlanningScope; label: string }[] = [
  { id: "all", label: "All issues" },
  { id: "mine", label: "My issues" },
  { id: "active", label: "Active" },
  { id: "backlog", label: "Backlog" },
  { id: "triage", label: "Triage" },
];

const GROUP_LABELS: Record<PlanningGroupBy, string> = {
  status: "Status",
  assignee: "Assignee",
  priority: "Priority",
  container: "List",
  none: "No grouping",
};

const ORDER_LABELS: Record<PlanningOrderBy, string> = {
  manual: "Manual",
  priority: "Priority",
  updated: "Last updated",
  created: "Created",
};

function toggle(values: readonly string[], value: string): string[] {
  return values.includes(value)
    ? values.filter((v) => v !== value)
    : [...values, value];
}

export function PlanningToolbar({
  display,
  filters,
  capabilities,
  containers,
  canCreate,
  searchRef,
  onDisplayChange,
  onFiltersChange,
  onCreate,
}: {
  display: PlanningDisplay;
  filters: PlanningFilters;
  capabilities: PlanningCapabilities;
  containers: readonly PlanningContainer[];
  canCreate: boolean;
  searchRef: React.RefObject<HTMLInputElement | null>;
  onDisplayChange: (display: PlanningDisplay) => void;
  onFiltersChange: (filters: PlanningFilters) => void;
  onCreate: () => void;
}) {
  const filtered = hasActiveFilters(filters);
  return (
    <div className="flex flex-col gap-2" data-testid="planning-toolbar">
      <div className="flex flex-wrap items-center gap-1">
        <nav aria-label="Planning views" className="flex items-center gap-1">
          {SCOPES.map((scope) => (
            <button
              aria-pressed={filters.scope === scope.id}
              className={cn(
                "rounded-md px-2 py-1 text-xs font-medium focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                filters.scope === scope.id
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
              )}
              data-testid={`planning-scope-${scope.id}`}
              key={scope.id}
              onClick={() => onFiltersChange({ ...filters, scope: scope.id })}
              type="button"
            >
              {scope.label}
            </button>
          ))}
        </nav>
        <span className="flex-1" />
        <fieldset
          aria-label="Layout"
          className="m-0 flex items-center rounded-md border border-border p-0.5"
        >
          <button
            aria-label="List"
            aria-pressed={display.layout === "list"}
            className={cn(
              "rounded-sm p-1 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
              display.layout === "list"
                ? "bg-accent text-foreground"
                : "text-muted-foreground",
            )}
            data-testid="planning-layout-list"
            onClick={() => onDisplayChange({ ...display, layout: "list" })}
            title="List"
            type="button"
          >
            <List aria-hidden="true" className="h-3.5 w-3.5" />
          </button>
          <button
            aria-label="Board"
            aria-pressed={display.layout === "board"}
            className={cn(
              "rounded-sm p-1 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
              display.layout === "board"
                ? "bg-accent text-foreground"
                : "text-muted-foreground",
            )}
            data-testid="planning-layout-board"
            onClick={() => onDisplayChange({ ...display, layout: "board" })}
            title="Board"
            type="button"
          >
            <Columns3 aria-hidden="true" className="h-3.5 w-3.5" />
          </button>
        </fieldset>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              data-testid="planning-display-menu"
              size="xs"
              variant="outline"
            >
              <SlidersHorizontal aria-hidden="true" />
              Display
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-56">
            <DropdownMenuLabel>Group by</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) =>
                onDisplayChange({
                  ...display,
                  groupBy: value as PlanningGroupBy,
                })
              }
              value={display.groupBy}
            >
              {(Object.keys(GROUP_LABELS) as PlanningGroupBy[]).map((id) => (
                <DropdownMenuRadioItem
                  data-testid={`planning-group-by-${id}`}
                  key={id}
                  value={id}
                >
                  {GROUP_LABELS[id]}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>Order by</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              onValueChange={(value) =>
                onDisplayChange({
                  ...display,
                  orderBy: value as PlanningOrderBy,
                })
              }
              value={display.orderBy}
            >
              {(Object.keys(ORDER_LABELS) as PlanningOrderBy[]).map((id) => (
                <DropdownMenuRadioItem
                  data-testid={`planning-order-by-${id}`}
                  key={id}
                  value={id}
                >
                  {ORDER_LABELS[id]}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
            <DropdownMenuSeparator />
            <DropdownMenuCheckboxItem
              checked={display.showClosed}
              data-testid="planning-show-closed"
              onCheckedChange={(checked) =>
                onDisplayChange({ ...display, showClosed: checked === true })
              }
            >
              Show done and canceled in the list
            </DropdownMenuCheckboxItem>
            <p className="px-2 pt-1 pb-1.5 text-2xs text-muted-foreground">
              Display options are yours and stay on this computer.
            </p>
          </DropdownMenuContent>
        </DropdownMenu>
        {canCreate ? (
          <Button
            aria-keyshortcuts="C"
            data-testid="planning-new-issue"
            onClick={onCreate}
            size="xs"
            title="New issue (C)"
          >
            <Plus aria-hidden="true" />
            New issue
          </Button>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center gap-1">
        <div className="relative w-56">
          <Search
            aria-hidden="true"
            className="-translate-y-1/2 absolute top-1/2 left-2 h-3.5 w-3.5 text-muted-foreground"
          />
          <Input
            aria-keyshortcuts="/"
            aria-label="Search issues"
            className="h-7 pl-7 text-xs"
            data-testid="planning-search"
            onChange={(event) =>
              onFiltersChange({ ...filters, query: event.target.value })
            }
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.currentTarget.blur();
              }
            }}
            placeholder="Search issues  /"
            ref={searchRef}
            value={filters.query}
          />
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              data-testid="planning-filter-menu"
              size="xs"
              variant="outline"
            >
              <Filter aria-hidden="true" />
              Filter
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-56">
            <DropdownMenuSub>
              <DropdownMenuSubTrigger data-testid="planning-filter-status">
                Status
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-48">
                {capabilities.statuses.map((status) => (
                  <DropdownMenuCheckboxItem
                    checked={filters.statusIds.includes(status.id)}
                    data-testid={`planning-filter-status-${status.id}`}
                    key={status.id}
                    onCheckedChange={() =>
                      onFiltersChange({
                        ...filters,
                        statusIds: toggle(filters.statusIds, status.id),
                      })
                    }
                    onSelect={(event) => event.preventDefault()}
                  >
                    <span className="flex items-center gap-2">
                      <StatusGlyph category={status.category} />
                      {status.label}
                    </span>
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger data-testid="planning-filter-priority">
                Priority
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-48">
                {capabilities.priorities.map((priority) => (
                  <DropdownMenuCheckboxItem
                    checked={filters.priorityIds.includes(priority.id)}
                    data-testid={`planning-filter-priority-${priority.id}`}
                    key={priority.id}
                    onCheckedChange={() =>
                      onFiltersChange({
                        ...filters,
                        priorityIds: toggle(filters.priorityIds, priority.id),
                      })
                    }
                    onSelect={(event) => event.preventDefault()}
                  >
                    <span className="flex items-center gap-2">
                      <PriorityGlyph level={priority.level} />
                      {priority.label}
                    </span>
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger data-testid="planning-filter-assignee">
                Assignee
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="w-48">
                <DropdownMenuRadioGroup
                  onValueChange={(value) =>
                    onFiltersChange({
                      ...filters,
                      assignee:
                        value === "me"
                          ? { kind: "me" }
                          : value === "unassigned"
                            ? { kind: "unassigned" }
                            : { kind: "any" },
                    })
                  }
                  value={
                    filters.assignee.kind === "person"
                      ? "any"
                      : filters.assignee.kind
                  }
                >
                  <DropdownMenuRadioItem value="any">
                    Anyone
                  </DropdownMenuRadioItem>
                  <DropdownMenuRadioItem
                    data-testid="planning-filter-assignee-me"
                    value="me"
                  >
                    Assigned to me
                  </DropdownMenuRadioItem>
                  <DropdownMenuRadioItem
                    data-testid="planning-filter-assignee-unassigned"
                    value="unassigned"
                  >
                    No assignee
                  </DropdownMenuRadioItem>
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            {containers.length > 1 ? (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger data-testid="planning-filter-container">
                  List
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="w-56">
                  <DropdownMenuRadioGroup
                    onValueChange={(value) =>
                      onFiltersChange({
                        ...filters,
                        containerId: value === "" ? null : value,
                      })
                    }
                    value={filters.containerId ?? ""}
                  >
                    <DropdownMenuRadioItem value="">
                      Every list
                    </DropdownMenuRadioItem>
                    {containers.map((container) => (
                      <DropdownMenuRadioItem
                        key={container.id}
                        value={container.id}
                      >
                        <span className="truncate">{container.title}</span>
                      </DropdownMenuRadioItem>
                    ))}
                  </DropdownMenuRadioGroup>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
        {filtered ? (
          <button
            className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
            data-testid="planning-clear-filters"
            onClick={() =>
              onFiltersChange({
                ...DEFAULT_PLANNING_FILTERS,
                scope: filters.scope,
              })
            }
            type="button"
          >
            <X aria-hidden="true" className="h-3 w-3" />
            Clear filters
          </button>
        ) : null}
      </div>
    </div>
  );
}
