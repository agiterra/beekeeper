/**
 * The session terminal drawer's pure model (SV-25, SV-22).
 *
 * Ported from T3 Code's terminal drawer (`ThreadTerminalDrawer.tsx:93-105`,
 * `types.ts:34-36`, `terminalUiStateStore.ts` split/new/close): the drawer's
 * height rule and its layout of terminals into groups of at most four.
 * Beekeeper's differences are recorded where they apply:
 *
 * - A terminal here is a real built-in shell (`create_shell_session`) whose
 *   id the host minted, and the host — not the layout — says which shells
 *   belong to the session. The layout only arranges them, and is reconciled
 *   against the host's list on every read.
 * - Teammates' shared terminals are listed beside one's own, labelled by
 *   owner and liveness, never by host name (DB11).
 *
 * No module state: the height and the layout live in `localStorage` under
 * keys that carry the community's relay URL, so nothing outlives a community
 * switch in memory and `resetCommunityState()` has nothing to reset.
 */

/** T3's default drawer height (`DEFAULT_THREAD_TERMINAL_HEIGHT`). */
export const CODING_SESSION_TERMINAL_DEFAULT_HEIGHT = 280;
/** T3's minimum (`MIN_DRAWER_HEIGHT`). */
export const CODING_SESSION_TERMINAL_MIN_HEIGHT = 180;
/** T3's maximum, as a share of the window (`MAX_DRAWER_HEIGHT_RATIO`). */
export const CODING_SESSION_TERMINAL_MAX_HEIGHT_RATIO = 0.75;
/** T3's split limit (`MAX_TERMINALS_PER_GROUP`). */
export const CODING_SESSION_TERMINAL_MAX_PER_GROUP = 4;
/** One arrow-key press moves the drawer edge this far. */
export const CODING_SESSION_TERMINAL_KEY_STEP = 16;

/** The tallest the drawer may be in a window `viewportHeight` tall. */
export function codingSessionTerminalMaxHeight(viewportHeight: number): number {
  if (!Number.isFinite(viewportHeight) || viewportHeight <= 0) {
    return CODING_SESSION_TERMINAL_DEFAULT_HEIGHT;
  }
  return Math.max(
    CODING_SESSION_TERMINAL_MIN_HEIGHT,
    Math.floor(viewportHeight * CODING_SESSION_TERMINAL_MAX_HEIGHT_RATIO),
  );
}

/** T3's `clampDrawerHeight`: rounded, at least the minimum, at most 75%. */
export function clampCodingSessionTerminalHeight(
  height: number,
  viewportHeight: number,
): number {
  const safe = Number.isFinite(height)
    ? height
    : CODING_SESSION_TERMINAL_DEFAULT_HEIGHT;
  return Math.min(
    Math.max(Math.round(safe), CODING_SESSION_TERMINAL_MIN_HEIGHT),
    codingSessionTerminalMaxHeight(viewportHeight),
  );
}

/** The drawer height key: per community, shared by every session in it. */
export function codingSessionTerminalHeightStorageKey(
  relayUrl: string,
): string {
  return `beekeeper:session-terminal-height:v1:${relayUrl}`;
}

/** A stored height, or the default when nothing usable is stored. */
export function parseStoredCodingSessionTerminalHeight(
  raw: string | null,
): number {
  if (raw === null) return CODING_SESSION_TERMINAL_DEFAULT_HEIGHT;
  const value = Number(raw);
  return Number.isFinite(value) && value > 0
    ? value
    : CODING_SESSION_TERMINAL_DEFAULT_HEIGHT;
}

// ── Layout ────────────────────────────────────────────────────────────────

export type CodingSessionTerminalSplit = "horizontal" | "vertical";

export type CodingSessionTerminalGroup = {
  id: string;
  terminalIds: string[];
  /** T3: absent means side by side. */
  splitDirection?: "vertical";
};

export type CodingSessionTerminalLayout = {
  terminalIds: string[];
  activeTerminalId: string;
  groups: CodingSessionTerminalGroup[];
  activeGroupId: string;
};

export const EMPTY_CODING_SESSION_TERMINAL_LAYOUT: CodingSessionTerminalLayout =
  Object.freeze({
    terminalIds: [],
    activeTerminalId: "",
    groups: [],
    activeGroupId: "",
  }) as CodingSessionTerminalLayout;

function groupIdFor(terminalId: string, used: ReadonlySet<string>): string {
  const base = `group-${terminalId}`;
  if (!used.has(base)) return base;
  let index = 2;
  while (used.has(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

function copyGroups(
  groups: readonly CodingSessionTerminalGroup[],
): CodingSessionTerminalGroup[] {
  return groups.map((group) => ({
    id: group.id,
    terminalIds: [...group.terminalIds],
    ...(group.splitDirection === "vertical"
      ? { splitDirection: "vertical" as const }
      : {}),
  }));
}

/**
 * Bring a layout in line with the shells that exist (T3's
 * `reconcileThreadTerminalSessionIds` + normalization): unknown ids leave,
 * new ids join as groups of one in `ids` order, and the active terminal and
 * group always resolve.
 */
export function reconcileCodingSessionTerminalLayout(
  layout: CodingSessionTerminalLayout,
  ids: readonly string[],
): CodingSessionTerminalLayout {
  const valid = new Set(ids);
  const assigned = new Set<string>();
  const used = new Set<string>();
  const groups: CodingSessionTerminalGroup[] = [];
  for (const group of layout.groups) {
    const terminalIds = group.terminalIds.filter((id) => {
      if (!valid.has(id) || assigned.has(id)) return false;
      assigned.add(id);
      return true;
    });
    if (terminalIds.length === 0) continue;
    const id = used.has(group.id)
      ? groupIdFor(terminalIds[0] ?? group.id, used)
      : group.id;
    used.add(id);
    groups.push({
      id,
      terminalIds,
      ...(group.splitDirection === "vertical"
        ? { splitDirection: "vertical" as const }
        : {}),
    });
  }
  for (const id of ids) {
    if (assigned.has(id)) continue;
    const groupId = groupIdFor(id, used);
    used.add(groupId);
    groups.push({ id: groupId, terminalIds: [id] });
  }
  const order = new Map(ids.map((id, index) => [id, index] as const));
  groups.sort(
    (left, right) =>
      Math.min(...left.terminalIds.map((id) => order.get(id) ?? Infinity)) -
      Math.min(...right.terminalIds.map((id) => order.get(id) ?? Infinity)),
  );
  const terminalIds = [...ids];
  const activeTerminalId = valid.has(layout.activeTerminalId)
    ? layout.activeTerminalId
    : (terminalIds[0] ?? "");
  const activeGroupId =
    groups.find(
      (group) =>
        group.id === layout.activeGroupId &&
        group.terminalIds.includes(activeTerminalId),
    )?.id ??
    groups.find((group) => group.terminalIds.includes(activeTerminalId))?.id ??
    "";
  return { terminalIds, activeTerminalId, groups, activeGroupId };
}

/** T3's `newThreadTerminal`: a new group of one, made active. */
export function addCodingSessionTerminal(
  layout: CodingSessionTerminalLayout,
  terminalId: string,
): CodingSessionTerminalLayout {
  if (terminalId.length === 0 || layout.terminalIds.includes(terminalId)) {
    return layout;
  }
  const groups = copyGroups(layout.groups);
  const groupId = groupIdFor(terminalId, new Set(groups.map((g) => g.id)));
  groups.push({ id: groupId, terminalIds: [terminalId] });
  return {
    terminalIds: [...layout.terminalIds, terminalId],
    activeTerminalId: terminalId,
    groups,
    activeGroupId: groupId,
  };
}

/** Whether the active group already holds the split limit (T3). */
export function codingSessionTerminalSplitLimitReached(
  layout: CodingSessionTerminalLayout,
): boolean {
  const group = layout.groups.find((g) => g.id === layout.activeGroupId);
  return (
    (group?.terminalIds.length ?? 0) >= CODING_SESSION_TERMINAL_MAX_PER_GROUP
  );
}

/**
 * T3's `splitThreadTerminal`: the new terminal joins the active group just
 * after the active terminal; refused (layout unchanged) at four. With no
 * terminal yet, a split is a new terminal.
 */
export function splitCodingSessionTerminal(
  layout: CodingSessionTerminalLayout,
  terminalId: string,
  direction: CodingSessionTerminalSplit = "horizontal",
): CodingSessionTerminalLayout {
  if (layout.terminalIds.length === 0) {
    return addCodingSessionTerminal(layout, terminalId);
  }
  if (terminalId.length === 0 || layout.terminalIds.includes(terminalId)) {
    return layout;
  }
  if (codingSessionTerminalSplitLimitReached(layout)) return layout;
  const groups = copyGroups(layout.groups);
  let index = groups.findIndex((group) => group.id === layout.activeGroupId);
  if (index < 0) {
    index = groups.findIndex((group) =>
      group.terminalIds.includes(layout.activeTerminalId),
    );
  }
  const destination = groups[index];
  if (!destination) return addCodingSessionTerminal(layout, terminalId);
  const anchor = destination.terminalIds.indexOf(layout.activeTerminalId);
  if (anchor >= 0) destination.terminalIds.splice(anchor + 1, 0, terminalId);
  else destination.terminalIds.push(terminalId);
  if (direction === "vertical") destination.splitDirection = "vertical";
  else delete destination.splitDirection;
  return {
    terminalIds: [...layout.terminalIds, terminalId],
    activeTerminalId: terminalId,
    groups,
    activeGroupId: destination.id,
  };
}

/** T3's `closeThreadTerminal`: the neighbour becomes active. */
export function closeCodingSessionTerminal(
  layout: CodingSessionTerminalLayout,
  terminalId: string,
): CodingSessionTerminalLayout {
  if (!layout.terminalIds.includes(terminalId)) return layout;
  const remaining = layout.terminalIds.filter((id) => id !== terminalId);
  if (remaining.length === 0) return EMPTY_CODING_SESSION_TERMINAL_LAYOUT;
  const closedIndex = layout.terminalIds.indexOf(terminalId);
  const activeTerminalId =
    layout.activeTerminalId === terminalId
      ? (remaining[Math.min(closedIndex, remaining.length - 1)] ??
        remaining[0] ??
        "")
      : layout.activeTerminalId;
  const groups = copyGroups(layout.groups)
    .map((group) => ({
      ...group,
      terminalIds: group.terminalIds.filter((id) => id !== terminalId),
    }))
    .filter((group) => group.terminalIds.length > 0);
  const activeGroupId =
    groups.find((group) => group.terminalIds.includes(activeTerminalId))?.id ??
    groups[0]?.id ??
    "";
  return { terminalIds: remaining, activeTerminalId, groups, activeGroupId };
}

/** T3's `setThreadActiveTerminal`. */
export function activateCodingSessionTerminal(
  layout: CodingSessionTerminalLayout,
  terminalId: string,
): CodingSessionTerminalLayout {
  if (!layout.terminalIds.includes(terminalId)) return layout;
  const activeGroupId =
    layout.groups.find((group) => group.terminalIds.includes(terminalId))?.id ??
    layout.activeGroupId;
  if (
    layout.activeTerminalId === terminalId &&
    layout.activeGroupId === activeGroupId
  ) {
    return layout;
  }
  return { ...layout, activeTerminalId: terminalId, activeGroupId };
}

/** The terminals the active group shows side by side (or stacked). */
export function visibleCodingSessionTerminals(
  layout: CodingSessionTerminalLayout,
): { ids: string[]; direction: CodingSessionTerminalSplit } {
  const group = layout.groups.find((g) => g.id === layout.activeGroupId);
  if (!group) {
    return {
      ids: layout.activeTerminalId ? [layout.activeTerminalId] : [],
      direction: "horizontal",
    };
  }
  return {
    ids: [...group.terminalIds],
    direction: group.splitDirection === "vertical" ? "vertical" : "horizontal",
  };
}

/** T3's group header words. */
export function codingSessionTerminalGroupLabel(
  group: CodingSessionTerminalGroup,
): "Single" | "Side by side" | "Stacked" {
  if (group.terminalIds.length < 2) return "Single";
  return group.splitDirection === "vertical" ? "Stacked" : "Side by side";
}

/** The layout key: per community, channel and session. */
export function codingSessionTerminalLayoutStorageKey(input: {
  relayUrl: string;
  channelId: string;
  sessionKey: string;
}): string {
  return `beekeeper:session-terminal-layout:v1:${input.relayUrl}:${input.channelId}:${input.sessionKey}`;
}

/** A stored layout, or the empty one when nothing usable is stored. */
export function parseStoredCodingSessionTerminalLayout(
  raw: string | null,
): CodingSessionTerminalLayout {
  if (raw === null) return EMPTY_CODING_SESSION_TERMINAL_LAYOUT;
  try {
    const value = JSON.parse(raw) as Partial<CodingSessionTerminalLayout>;
    const strings = (list: unknown): string[] =>
      Array.isArray(list)
        ? list.filter((item): item is string => typeof item === "string")
        : [];
    const groups = Array.isArray(value.groups)
      ? value.groups.flatMap((group): CodingSessionTerminalGroup[] =>
          group && typeof group.id === "string"
            ? [
                {
                  id: group.id,
                  terminalIds: strings(group.terminalIds),
                  ...(group.splitDirection === "vertical"
                    ? { splitDirection: "vertical" as const }
                    : {}),
                },
              ]
            : [],
        )
      : [];
    return {
      terminalIds: strings(value.terminalIds),
      activeTerminalId:
        typeof value.activeTerminalId === "string"
          ? value.activeTerminalId
          : "",
      groups,
      activeGroupId:
        typeof value.activeGroupId === "string" ? value.activeGroupId : "",
    };
  } catch {
    return EMPTY_CODING_SESSION_TERMINAL_LAYOUT;
  }
}

// ── Which shells, and what they say ───────────────────────────────────────

/** A shell as the drawer needs it: its id, the session it was opened for. */
export type CodingSessionTerminalShell = {
  sessionId: string;
  title: string;
  createdAt: number;
  running: boolean;
  restorable: boolean;
  codingSession?: { sessionRef: string; channelId?: string | null } | null;
};

/** This session's shells on this computer, oldest first. */
export function codingSessionShellsFor<T extends CodingSessionTerminalShell>(
  shells: readonly T[],
  session: { sessionKey: string; channelId: string },
): T[] {
  return shells
    .filter(
      (shell) =>
        shell.codingSession?.sessionRef === session.sessionKey &&
        (shell.codingSession.channelId ?? null) === session.channelId,
    )
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.sessionId.localeCompare(right.sessionId),
    );
}

/** A teammate's shared terminal for this session, from its 30623 announce. */
export type CodingSessionSharedTerminal = {
  sessionId: string;
  ownerPubkey: string;
  title: string;
  projectRef: string;
  sessionRef: string | null;
};

/**
 * The shared terminals that belong to this session and are not this
 * computer's own shells (those are already in the drawer, live).
 */
export function codingSessionSharedTerminalsFor<
  T extends CodingSessionSharedTerminal,
>(
  terminals: readonly T[],
  sessionKey: string,
  localShellIds: ReadonlySet<string>,
): T[] {
  return terminals.filter(
    (terminal) =>
      terminal.sessionRef === sessionKey &&
      !localShellIds.has(terminal.sessionId),
  );
}

/** The drawer's header line (DB9): whose shell, how boxed, which tree. */
export function codingSessionTerminalHeaderLine(treeLabel: string): string {
  return `Your shell · not sandboxed · ${treeLabel}`;
}

/** What the drawer can say about a watched terminal's stream. */
export type CodingSessionSharedTerminalStatus =
  | "connecting"
  | "live"
  | "stalled"
  | "ended"
  /** Listed by its announce, not being watched: liveness unknown. */
  | "shared";

/**
 * A teammate's terminal, by owner and liveness — never by host name (DB11).
 * The viewer's own terminal on another of their computers reads as "Your
 * other computer": the drawer lists this computer's shells separately.
 */
export function codingSessionSharedTerminalLabel(input: {
  ownerName: string;
  isSelf: boolean;
  status: CodingSessionSharedTerminalStatus;
}): string {
  const where = input.isSelf
    ? "Your other computer"
    : `${input.ownerName}'s computer`;
  return `${where} · ${input.status}`;
}

/** The labels shells get in the drawer: "Terminal 1", "Terminal 2", … */
export function codingSessionTerminalLabels(
  ids: readonly string[],
): Map<string, string> {
  return new Map(ids.map((id, index) => [id, `Terminal ${index + 1}`]));
}

/** The next free "Terminal N" title for a new shell. */
export function nextCodingSessionTerminalTitle(
  shells: readonly Pick<CodingSessionTerminalShell, "title">[],
): string {
  const taken = new Set(shells.map((shell) => shell.title));
  let index = shells.length + 1;
  while (taken.has(`Terminal ${index}`)) index += 1;
  return `Terminal ${index}`;
}

/** One shell's foreground read: running, at the prompt, or unknown. */
export type CodingSessionShellForeground = {
  sessionId: string;
  runningCommand: boolean | null;
};

/**
 * The Terminal badge's count (SV-22, DB6): this session's shells on this
 * computer whose terminal a command holds right now. Unknown is not counted.
 */
export function countCodingSessionRunningCommands(
  reads: readonly CodingSessionShellForeground[],
  shellIds: ReadonlySet<string>,
): number {
  return reads.filter(
    (read) => shellIds.has(read.sessionId) && read.runningCommand === true,
  ).length;
}

/** The Terminal badge's words, for its label and tooltip. */
export function codingSessionRunningCommandsLabel(count: number): string {
  return count === 1
    ? "1 command running on this computer"
    : `${count} commands running on this computer`;
}

/**
 * Whether this computer's foreground read can be believed right now. A
 * failed read (`isError`) or a last answer older than two poll periods says
 * nothing about now: every live shell is then unknown, never the last
 * answer's "running" or "at the prompt" carried forward.
 */
export function codingSessionForegroundUnknown(input: {
  isError: boolean;
  /** When the last successful read landed (ms epoch); 0 when none has. */
  dataUpdatedAt: number;
  now: number;
  pollMs: number;
}): boolean {
  if (input.isError) return true;
  if (input.dataUpdatedAt <= 0) return false;
  return input.now - input.dataUpdatedAt > input.pollMs * 2;
}

/**
 * The foreground read folded onto this session's live shells: which a
 * command holds, which are at the prompt, and the badge's count. Unknown —
 * no read yet, or `codingSessionForegroundUnknown` — empties all three.
 */
export function codingSessionForegroundFold(input: {
  reads: readonly CodingSessionShellForeground[] | undefined;
  liveIds: readonly string[];
  unknown: boolean;
}): {
  runningIds: ReadonlySet<string>;
  idleIds: ReadonlySet<string>;
  runningCount: number;
} {
  if (input.unknown || !input.reads || input.liveIds.length === 0) {
    return { runningIds: new Set(), idleIds: new Set(), runningCount: 0 };
  }
  const live = new Set(input.liveIds);
  const runningIds = new Set<string>();
  const idleIds = new Set<string>();
  for (const read of input.reads) {
    if (!live.has(read.sessionId)) continue;
    if (read.runningCommand === true) runningIds.add(read.sessionId);
    else if (read.runningCommand === false) idleIds.add(read.sessionId);
  }
  return {
    runningIds,
    idleIds,
    runningCount: countCodingSessionRunningCommands(input.reads, live),
  };
}

/** The drawer's line when this computer could not read its shells. */
export const CODING_SESSION_FOREGROUND_UNKNOWN_LINE =
  "This computer could not tell which of these terminals is running a command.";

/** The line a bounded shared-terminal read that came back full gets. */
export function codingSessionSharedTerminalsTruncatedLine(
  limit: number,
): string {
  return `Only the project's newest ${limit} terminals were checked for ones shared in this session.`;
}

/**
 * Where a session's tree is, when another computer has it: the clause both
 * the Files and Terminal surfaces build their sentence on, so the two state
 * one fact one way. `providerName` is the name of the provider key that runs
 * the session (the execution's signer), which is not the machine's owner:
 * the clause says whose provider runs it, never whose computer it is.
 */
export function codingSessionTreeElsewhereClause(
  providerName: string | null,
): string {
  const name = providerName?.trim() ?? "";
  return name.length > 0
    ? `The working tree is on another computer (${name}'s provider runs this session)`
    : "The working tree is on another computer";
}

/** The provider name the elsewhere clause names, or `null`. */
export function codingSessionTreeProviderName(input: {
  focusedRecord?: { providerAuthorityPubkey?: string | null } | null;
  focusedExecution?: { signerPubkey?: string | null } | null;
  resolveActorName: (pubkey: string) => string | null | undefined;
}): string | null {
  const provider =
    input.focusedRecord?.providerAuthorityPubkey ??
    input.focusedExecution?.signerPubkey ??
    null;
  if (provider === null || provider.length === 0) return null;
  const name = input.resolveActorName(provider)?.trim() ?? "";
  return name.length > 0 ? name : null;
}
