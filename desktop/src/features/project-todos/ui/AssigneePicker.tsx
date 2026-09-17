import { Bot, UserRound, X } from "lucide-react";
import * as React from "react";

import { useProjectAgents } from "@/features/project-agents/lib/useProjectAgents";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import {
  rosterWithOwner,
  useProjectRosterQuery,
} from "@/features/projects-container/lib/projectMembers";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { UserAvatar } from "@/shared/ui/UserAvatar";

import { type TodoPerson, todoPerson } from "../lib/todoPeople";

/** An avatar with the agent glyph when the person is an agent. */
export function PersonAvatar({
  person,
  size = "xs",
  testId,
}: {
  person: TodoPerson;
  size?: "xs" | "sm";
  testId?: string;
}) {
  return (
    <span className="relative inline-flex shrink-0" data-testid={testId}>
      <UserAvatar
        avatarUrl={person.avatarUrl}
        displayName={person.name}
        size={size}
      />
      {person.isAgent ? (
        <Bot
          aria-label="Agent"
          className="-bottom-0.5 -right-0.5 absolute h-2.5 w-2.5 rounded-full bg-background text-muted-foreground"
          data-testid="todo-assignee-agent"
        />
      ) : null}
    </span>
  );
}

/**
 * The people an item can be assigned to: the project's roster (creator
 * included) and its agents. Mounted only while the picker is open, since the
 * agents read fans out over the project's session channels.
 */
function AssigneeCandidates({
  project,
  current,
  onPick,
}: {
  project: ProjectContainer;
  current: string | null;
  onPick: (pubkey: string | null) => void;
}) {
  const rosterQuery = useProjectRosterQuery(project);
  const roster = rosterWithOwner(project, rosterQuery.data ?? project.members);
  const { model, isLoading } = useProjectAgents(project.id);
  const agentRows = [...model.projectAgents, ...model.borrowed];
  const humanPubkeys = roster.map((entry) => entry.pubkey);
  const profiles = useUsersBatchQuery(humanPubkeys);
  const humans: TodoPerson[] = humanPubkeys.map((pubkey) =>
    todoPerson(pubkey, profiles.data?.profiles),
  );
  const agents: TodoPerson[] = agentRows
    .filter((row) => !humanPubkeys.includes(row.pubkey.toLowerCase()))
    .map((row) => ({
      pubkey: row.pubkey.toLowerCase(),
      name: row.name,
      avatarUrl: row.avatarUrl,
      isAgent: true,
    }));
  const seen = new Set<string>();
  const candidates = [...humans, ...agents].filter((person) => {
    if (seen.has(person.pubkey)) return false;
    seen.add(person.pubkey);
    return true;
  });

  return (
    <ul
      className="flex max-h-72 flex-col gap-0.5 overflow-y-auto"
      data-testid="todo-assignee-options"
    >
      <li>
        <button
          className={cn(
            "flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-sm hover:bg-accent",
            current === null && "bg-accent/60",
          )}
          onClick={() => onPick(null)}
          type="button"
        >
          <X aria-hidden="true" className="h-3.5 w-3.5 text-muted-foreground" />
          Unassigned
        </button>
      </li>
      {candidates.map((person) => (
        <li key={person.pubkey}>
          <button
            className={cn(
              "flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-sm hover:bg-accent",
              current === person.pubkey && "bg-accent/60",
            )}
            data-testid={`todo-assignee-option-${person.pubkey}`}
            onClick={() => onPick(person.pubkey)}
            type="button"
          >
            <PersonAvatar person={person} />
            <span className="truncate">{person.name}</span>
            {person.isAgent ? (
              <span className="ml-auto text-2xs text-muted-foreground">
                agent
              </span>
            ) : null}
          </button>
        </li>
      ))}
      {isLoading && agents.length === 0 ? (
        <li className="px-2 py-1 text-2xs text-muted-foreground">
          Reading agents…
        </li>
      ) : null}
    </ul>
  );
}

/**
 * The assignee chip on a to-do row: the person's avatar and name (with the
 * agent glyph for an agent), a popover to reassign or clear, or an
 * "Assign" placeholder. A read-only viewer gets the chip with no trigger.
 */
export function AssigneePicker({
  project,
  assignee,
  canEdit,
  onChange,
  testId,
}: {
  project: ProjectContainer;
  assignee: TodoPerson | null;
  canEdit: boolean;
  onChange: (pubkey: string | null) => void;
  testId?: string;
}) {
  const [open, setOpen] = React.useState(false);
  const chip = (
    <span
      className={cn(
        "inline-flex max-w-40 items-center gap-1 rounded-sm px-1 py-0.5 text-2xs",
        assignee ? "text-foreground" : "text-muted-foreground",
        canEdit && "hover:bg-accent",
      )}
      data-testid={testId}
    >
      {assignee ? (
        <>
          <PersonAvatar person={assignee} />
          <span className="truncate">{assignee.name}</span>
        </>
      ) : (
        <>
          <UserRound aria-hidden="true" className="h-3 w-3" />
          Assign
        </>
      )}
    </span>
  );
  if (!canEdit) return chip;
  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <button
          aria-label={
            assignee ? `Assigned to ${assignee.name}; change` : "Assign"
          }
          className="rounded-sm focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
          type="button"
        >
          {chip}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 p-2">
        {open ? (
          <AssigneeCandidates
            current={assignee?.pubkey ?? null}
            onPick={(pubkey) => {
              onChange(pubkey);
              setOpen(false);
            }}
            project={project}
          />
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
