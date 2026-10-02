import { ChevronDown, ListFilter } from "lucide-react";
import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { ProfileAvatar } from "@/features/profile/ui/ProfileAvatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import type { ProjectContainer } from "../hooks";
import { rosterWithOwner, useProjectRosterQuery } from "../lib/projectMembers";
import {
  projectHiddenArtifactsNote,
  projectSessionFilterLabel,
  projectSessionUnattributedNote,
  type ProjectSessionDateRange,
  type ProjectSessionFilter,
} from "../lib/projectSessionFilter";

type MemberMode = ProjectSessionFilter["members"]["mode"];
type RangeKind = ProjectSessionDateRange["kind"];

const RANGE_OPTIONS: Array<{ kind: RangeKind; label: string }> = [
  { kind: "any", label: "Any time" },
  { kind: "today", label: "Today" },
  { kind: "yesterday", label: "Yesterday" },
  { kind: "week", label: "This week" },
  { kind: "month", label: "This month" },
  { kind: "custom", label: "Custom range" },
];

/** Stop a click inside the menu from selecting/closing the item under it. */
const keepMenuOpen = (event: Event) => event.preventDefault();

/**
 * The session filter that sits under a project's session list. Three axes:
 * whose sessions (My / All / Custom with a checkbox per member — the roster
 * plus any founder already seen in this project's sessions who is not on it,
 * so the filter can never hide a session with no way to reveal it again),
 * whether closed and archived sessions show, and a last-activity date range.
 */
export function ProjectSessionFilterMenu({
  project,
  isFallback,
  currentPubkey,
  filter,
  onChange,
  sessionFounders,
  hiddenUnattributed,
  hiddenByState,
  hiddenArtifacts = 0,
}: {
  project: ProjectContainer;
  /** The local General placeholder has no head, so no roster to read. */
  isFallback?: boolean;
  currentPubkey?: string;
  filter: ProjectSessionFilter;
  onChange: (filter: ProjectSessionFilter) => void;
  /** Distinct founders across this project's sessions (lowercase). */
  sessionFounders: string[];
  /** Sessions the current filter hides because their founder is unknown. */
  hiddenUnattributed: number;
  /** Sessions hidden by the closed/archived boxes or the date range. */
  hiddenByState: number;
  /** Pinned artifacts the `showPinnedArtifacts` box is hiding. */
  hiddenArtifacts?: number;
}) {
  const [open, setOpen] = React.useState(false);
  const { members } = filter;
  const custom = members.mode === "custom";
  // One roster read per open menu, not one per project at sidebar mount.
  const rosterQuery = useProjectRosterQuery(
    !isFallback && (open || custom) ? project : null,
  );
  const memberPubkeys = React.useMemo(() => {
    const seen = new Set<string>();
    const ordered: string[] = [];
    const push = (pubkey: string) => {
      const key = pubkey.toLowerCase();
      if (!key || seen.has(key)) return;
      seen.add(key);
      ordered.push(key);
    };
    if (!isFallback) {
      for (const entry of rosterWithOwner(
        project,
        rosterQuery.data ?? project.members,
      )) {
        push(entry.pubkey);
      }
    }
    for (const founder of sessionFounders) push(founder);
    if (members.mode === "custom") {
      for (const pubkey of members.pubkeys) push(pubkey);
    }
    return ordered;
  }, [isFallback, members, project, rosterQuery.data, sessionFounders]);
  const profiles = useUsersBatchQuery(memberPubkeys, {
    enabled: open || custom,
  }).data?.profiles;

  const selected = React.useMemo(
    () =>
      new Set(
        members.mode === "custom"
          ? members.pubkeys.map((p) => p.toLowerCase())
          : [],
      ),
    [members],
  );

  const setMemberMode = (mode: MemberMode) => {
    if (mode === "custom") {
      if (custom) return;
      // Seed with the viewer so a fresh custom set is never empty.
      onChange({
        ...filter,
        members: {
          mode: "custom",
          pubkeys: currentPubkey ? [currentPubkey.toLowerCase()] : [],
        },
      });
      return;
    }
    onChange({ ...filter, members: { mode } });
  };

  const toggleMember = (pubkey: string) => {
    const next = new Set(selected);
    if (next.has(pubkey)) next.delete(pubkey);
    else next.add(pubkey);
    onChange({ ...filter, members: { mode: "custom", pubkeys: [...next] } });
  };

  const setRangeKind = (kind: RangeKind) => {
    if (kind === "custom") {
      if (filter.range.kind === "custom") return;
      onChange({ ...filter, range: { kind: "custom", from: null, to: null } });
      return;
    }
    onChange({ ...filter, range: { kind } });
  };

  const setCustomBound = (bound: "from" | "to", value: string) => {
    const current =
      filter.range.kind === "custom"
        ? filter.range
        : { kind: "custom" as const, from: null, to: null };
    onChange({
      ...filter,
      range: { ...current, [bound]: value === "" ? null : value },
    });
  };

  const note = projectSessionUnattributedNote(hiddenUnattributed);
  const artifactsNote = projectHiddenArtifactsNote(hiddenArtifacts);
  const label = projectSessionFilterLabel(filter);
  // Everything the filter is hiding counts in the badge, pinned artifacts
  // included: the number's job is "there is more than you can see here".
  const hiddenTotal = hiddenUnattributed + hiddenByState + hiddenArtifacts;

  return (
    <DropdownMenu onOpenChange={setOpen} open={open}>
      <DropdownMenuTrigger asChild>
        <button
          aria-label={`Filter sessions in ${project.name}: ${label}`}
          className="flex h-7 w-full items-center gap-1.5 rounded-md px-2 text-2xs text-sidebar-foreground/55 outline-none transition-colors hover:text-sidebar-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring data-[state=open]:text-sidebar-foreground"
          data-testid={`project-session-filter-${project.dtag}`}
          title={note ?? undefined}
          type="button"
        >
          <ListFilter aria-hidden className="size-3 shrink-0" />
          <span className="truncate">{label}</span>
          {hiddenTotal > 0 ? (
            <span
              className="shrink-0 text-sidebar-foreground/45"
              data-testid="project-session-filter-hidden-count"
            >
              +{hiddenTotal} hidden
            </span>
          ) : null}
          <ChevronDown aria-hidden className="ml-auto size-3 shrink-0" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-56">
        <DropdownMenuLabel className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Started by
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          onValueChange={(value) => setMemberMode(value as MemberMode)}
          value={members.mode}
        >
          <DropdownMenuRadioItem
            data-testid="project-session-filter-mode-mine"
            onSelect={keepMenuOpen}
            value="mine"
          >
            My sessions
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem
            data-testid="project-session-filter-mode-all"
            onSelect={keepMenuOpen}
            value="all"
          >
            All sessions
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem
            data-testid="project-session-filter-mode-custom"
            onSelect={keepMenuOpen}
            value="custom"
          >
            Custom
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        {custom ? (
          <div data-testid="project-session-filter-members">
            {memberPubkeys.length === 0 ? (
              <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
                {rosterQuery.isPending && !isFallback
                  ? "Loading members…"
                  : "No members to choose from."}
              </DropdownMenuLabel>
            ) : (
              memberPubkeys.map((pubkey) => {
                const profile = profiles?.[pubkey];
                const name = resolveUserLabel({
                  currentPubkey,
                  profiles,
                  pubkey,
                });
                return (
                  <DropdownMenuCheckboxItem
                    checked={selected.has(pubkey)}
                    className="gap-2 pl-10"
                    data-testid={`project-session-filter-member-${pubkey}`}
                    key={pubkey}
                    onSelect={keepMenuOpen}
                    onCheckedChange={() => toggleMember(pubkey)}
                  >
                    <ProfileAvatar
                      avatarUrl={profile?.avatarUrl ?? null}
                      className="h-5 w-5 text-3xs shadow-none"
                      iconClassName="h-2.5 w-2.5"
                      label={name}
                    />
                    <span className="truncate">{name}</span>
                  </DropdownMenuCheckboxItem>
                );
              })
            )}
          </div>
        ) : null}

        <DropdownMenuSeparator />
        <DropdownMenuLabel className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Last active
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          onValueChange={(value) => setRangeKind(value as RangeKind)}
          value={filter.range.kind}
        >
          {RANGE_OPTIONS.map((option) => (
            <DropdownMenuRadioItem
              data-testid={`project-session-filter-range-${option.kind}`}
              key={option.kind}
              onSelect={keepMenuOpen}
              value={option.kind}
            >
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        {filter.range.kind === "custom" ? (
          <fieldset
            aria-label="Custom date range"
            className="flex items-center gap-2 px-3 py-1.5 text-xs"
            data-testid="project-session-filter-custom-range"
            // Typing in a menu: keep Radix's typeahead and item navigation
            // from swallowing the keystrokes meant for the date inputs.
            onKeyDown={(event) => event.stopPropagation()}
          >
            <label className="flex flex-1 flex-col gap-0.5 text-muted-foreground">
              From
              <input
                className="rounded border border-input bg-background px-1.5 py-0.5 text-xs text-foreground"
                data-testid="project-session-filter-range-from"
                onChange={(event) =>
                  setCustomBound("from", event.currentTarget.value)
                }
                type="date"
                value={filter.range.from ?? ""}
              />
            </label>
            <label className="flex flex-1 flex-col gap-0.5 text-muted-foreground">
              To
              <input
                className="rounded border border-input bg-background px-1.5 py-0.5 text-xs text-foreground"
                data-testid="project-session-filter-range-to"
                onChange={(event) =>
                  setCustomBound("to", event.currentTarget.value)
                }
                type="date"
                value={filter.range.to ?? ""}
              />
            </label>
          </fieldset>
        ) : null}

        <DropdownMenuSeparator />
        <DropdownMenuLabel className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Show
        </DropdownMenuLabel>
        <DropdownMenuCheckboxItem
          checked={filter.showClosed}
          data-testid="project-session-filter-show-closed"
          onCheckedChange={(checked) =>
            onChange({
              ...filter,
              showClosed: checked === true,
              // Archived implies closed: hiding closed hides archived too.
              showArchived: checked === true ? filter.showArchived : false,
            })
          }
          onSelect={keepMenuOpen}
        >
          Show closed
        </DropdownMenuCheckboxItem>
        <DropdownMenuCheckboxItem
          checked={filter.showArchived}
          data-testid="project-session-filter-show-archived"
          onCheckedChange={(checked) =>
            onChange({
              ...filter,
              showArchived: checked === true,
              // Archived implies closed: asking for archived asks for closed.
              showClosed: checked === true ? true : filter.showClosed,
            })
          }
          onSelect={keepMenuOpen}
        >
          Show archived
        </DropdownMenuCheckboxItem>

        <DropdownMenuSeparator />
        <DropdownMenuLabel className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Artifacts
        </DropdownMenuLabel>
        <DropdownMenuCheckboxItem
          checked={filter.showPinnedArtifacts}
          data-testid="project-session-filter-show-pinned-artifacts"
          onCheckedChange={(checked) =>
            onChange({ ...filter, showPinnedArtifacts: checked === true })
          }
          onSelect={keepMenuOpen}
        >
          Show pinned artifacts
        </DropdownMenuCheckboxItem>
        <DropdownMenuLabel className="max-w-64 whitespace-normal text-xs font-normal text-muted-foreground">
          A pin is shared with the whole project; this hides the rows on this
          computer only.
        </DropdownMenuLabel>

        {note || artifactsNote ? (
          <>
            <DropdownMenuSeparator />
            {note ? (
              <DropdownMenuLabel
                className="max-w-64 whitespace-normal text-xs font-normal text-muted-foreground"
                data-testid="project-session-filter-unattributed-note"
              >
                {note}
              </DropdownMenuLabel>
            ) : null}
            {artifactsNote ? (
              <DropdownMenuLabel
                className="max-w-64 whitespace-normal text-xs font-normal text-muted-foreground"
                data-testid="project-session-filter-hidden-artifacts-note"
              >
                {artifactsNote}
              </DropdownMenuLabel>
            ) : null}
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
