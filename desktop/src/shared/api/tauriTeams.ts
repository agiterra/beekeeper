import { invokeTauri } from "@/shared/api/tauri";
import type {
  AgentTeam,
  AgentTeamCrew,
  CreateTeamInput,
  UpdateTeamInput,
} from "@/shared/api/types";

/** Wire shape of `TeamCrew` — camelCase inside a snake_case record. */
type RawTeamCrew = {
  primary: string;
  seats: {
    personaId: string;
    role: string;
    driver?: string | null;
    model?: string | null;
    vendor?: string | null;
  }[];
};

type RawTeam = {
  id: string;
  name: string;
  description: string | null;
  instructions?: string | null;
  persona_ids: string[];
  /** Absent on an older backend, and absent for an ordinary team. */
  crew?: RawTeamCrew | null;
  is_builtin?: boolean;
  source_dir?: string | null;
  is_symlink?: boolean;
  symlink_target?: string | null;
  version?: string | null;
  created_at: string;
  updated_at: string;
};

function fromRawTeam(team: RawTeam): AgentTeam {
  return {
    id: team.id,
    name: team.name,
    description: team.description,
    instructions: team.instructions ?? null,
    personaIds: team.persona_ids,
    crew: (team.crew as AgentTeamCrew | null | undefined) ?? null,
    isBuiltin: team.is_builtin ?? false,
    sourceDir: team.source_dir ?? null,
    isSymlink: team.is_symlink ?? false,
    symlinkTarget: team.symlink_target ?? null,
    version: team.version ?? null,
    createdAt: team.created_at,
    updatedAt: team.updated_at,
  };
}

export async function listTeams(): Promise<AgentTeam[]> {
  return (await invokeTauri<RawTeam[]>("list_teams")).map(fromRawTeam);
}

export async function createTeam(input: CreateTeamInput): Promise<AgentTeam> {
  return fromRawTeam(
    await invokeTauri<RawTeam>("create_team", {
      input: {
        name: input.name,
        description: input.description,
        instructions: input.instructions,
        personaIds: input.personaIds,
      },
    }),
  );
}

export async function updateTeam(input: UpdateTeamInput): Promise<AgentTeam> {
  return fromRawTeam(
    await invokeTauri<RawTeam>("update_team", {
      input: {
        id: input.id,
        name: input.name,
        description: input.description,
        instructions: input.instructions,
        personaIds: input.personaIds,
      },
    }),
  );
}

export async function deleteTeam(id: string): Promise<void> {
  await invokeTauri("delete_team", { id });
}

// ── Team snapshot types ─────────────────────────────────────────────────────

export type SnapshotFormat = "json" | "png";
export type SnapshotMemoryLevel = "none" | "core" | "everything";

export type EncodedTeamSnapshotPayload = {
  fileBytes: number[];
  fileName: string;
};

export type TeamSnapshotMemberPreview = {
  displayName: string;
  systemPrompt: string | null;
  avatarUrl: string | null;
  hasSourceAllowlist: boolean;
  sourceAllowlistCount: number;
};

export type TeamSnapshotImportPreview = {
  name: string;
  description: string | null;
  instructions: string | null;
  members: TeamSnapshotMemberPreview[];
  hasSourceAllowlist: boolean;
  /**
   * Set when the snapshot declares a crew whose seats cannot be bound to its
   * own members. The team still imports — without the crew — and this sentence
   * is why, so the import does not silently look like an ordinary team.
   */
  crewWarning?: string | null;
};

export type TeamSnapshotImportConfirm = {
  fileBytes: number[];
  keepAllowlist: boolean;
};

export type TeamSnapshotImportMemberResult = {
  displayName: string;
  pubkey: string;
  personaId: string;
  memoryWritten: number;
  memoryTotal: number;
  memoryErrors: string[];
  profileSyncError: string | null;
};

/** Wire shape of the nested `TeamRecord` — Rust has no `rename_all` so fields
 *  arrive in snake_case, matching the existing `RawTeam` convention. */
type RawTeamRecord = {
  id: string;
  name: string;
  description: string | null;
  persona_ids: string[];
  instructions: string | null;
  is_builtin: boolean;
  source_dir: string | null;
  is_symlink: boolean;
  symlink_target: string | null;
  version: string | null;
  created_at: string;
  updated_at: string;
};

/** Raw wire shape of the import result — outer struct is camelCase,
 *  but the nested `team` field is snake_case (no `rename_all` on TeamRecord). */
type RawTeamSnapshotImportResult = {
  team: RawTeamRecord;
  personaIds: string[];
  members: TeamSnapshotImportMemberResult[];
};

export type TeamSnapshotImportResult = {
  team: AgentTeam;
  personaIds: string[];
  members: TeamSnapshotImportMemberResult[];
};

// ── Team snapshot commands ───────────────────────────────────────────────────

export async function exportTeamSnapshot(
  id: string,
  memoryLevel: SnapshotMemoryLevel,
  format: SnapshotFormat,
): Promise<boolean> {
  return invokeTauri<boolean>("export_team_snapshot", {
    id,
    memoryLevel,
    format,
  });
}

export async function encodeTeamSnapshotForSend(
  id: string,
  memoryLevel: SnapshotMemoryLevel,
  format: SnapshotFormat,
): Promise<EncodedTeamSnapshotPayload> {
  return invokeTauri<EncodedTeamSnapshotPayload>(
    "encode_team_snapshot_for_send",
    {
      id,
      memoryLevel,
      format,
    },
  );
}

export async function previewTeamSnapshotImport(
  fileBytes: number[],
  fileName: string,
): Promise<TeamSnapshotImportPreview> {
  return invokeTauri<TeamSnapshotImportPreview>(
    "preview_team_snapshot_import",
    {
      fileBytes,
      fileName,
    },
  );
}

export async function confirmTeamSnapshotImport(
  input: TeamSnapshotImportConfirm,
): Promise<TeamSnapshotImportResult> {
  const raw = await invokeTauri<RawTeamSnapshotImportResult>(
    "confirm_team_snapshot_import",
    { input },
  );
  return {
    team: fromRawTeam(raw.team),
    personaIds: raw.personaIds,
    members: raw.members,
  };
}

// ── Crew role packs ─────────────────────────────────────────────────────────

/** One installed role, as `install_crew_role_packs` reports it. */
export type InstalledCrewRole = {
  personaId: string;
  personaName: string;
  role: string;
  agentPubkey: string;
  agentName: string;
  packDir: string;
  /** `true` when an agent already installed from this pack was refreshed. */
  refreshed: boolean;
  /**
   * `true` when this run gave an already-installed identity a new name.
   *
   * Distinct from `refreshed`: a refresh that changed nothing is not a
   * rename, and only a rename owes the relay a fresh kind:0 profile.
   */
  renamed: boolean;
  /** `true` when this role is in the crew's default seat roster. */
  seated: boolean;
};

/** One row of the installer's "Name your team" list, as the scan reports it. */
export type CrewRoleNameChoice = {
  /** The role the pack's persona declares — the key of the install's map. */
  role: string;
  personaName: string;
  packDir: string;
  /**
   * What the field starts on: the installed identity's current name, or the
   * pack's own display name when nothing is installed from it yet.
   */
  defaultName: string;
  /** `true` when an identity is already installed from this pack. */
  installed: boolean;
};

/** A chosen folder together with what one read-only scan of it found. */
export type PickedCrewRolePacks = {
  directory: string;
  packs: CrewRoleNameChoice[];
  skipped: SkippedCrewRolePack[];
};

/** One child of the chosen folder that produced no role, and why. */
export type SkippedCrewRolePack = { path: string; reason: string };

export type InstallCrewRolePacksResponse = {
  teamId: string;
  teamName: string;
  installed: InstalledCrewRole[];
  skipped: SkippedCrewRolePack[];
  /**
   * The roles the crew that was written actually seats, in seat order.
   *
   * Read off the crew block the install produced, never off the default
   * roster: a partial install seats fewer roles than the roster names.
   */
  seated: string[];
  /** Roster roles whose pack was not installed, so they hold no seat. */
  dropped: string[];
  /**
   * What went wrong republishing the installed identities' relay profiles, or
   * `null` when every one landed.
   *
   * The install itself succeeded when this is set — the stores are written.
   * What is not true is that the relay knows these identities by the names
   * this computer now uses, which is exactly the state ledger 80 (e) found.
   */
  profileSyncError: string | null;
};

/** Which stage of an install failed, as `CrewRoleInstallFailure` names it. */
export type CrewRoleInstallFailureStage = "folder" | "keys" | "store" | "relay";

/** A failed install: the stage that failed, and the cause verbatim. */
export type CrewRoleInstallFailure = {
  failure: CrewRoleInstallFailureStage;
  detail: string;
};

/**
 * What one read-only look at a project's `personas/roles` folder found.
 *
 * `exists` is separate from an empty `packs` list on purpose: "that checkout
 * has no personas/roles folder" and "it has one and it is empty" send an
 * operator to different places, and the dialog says which.
 */
export type ProjectRolePacksScan = {
  /** The folder the scan looked in, named whether or not it is there. */
  directory: string;
  exists: boolean;
  /** The same rows `pickCrewRolePacksDirectory` returns, from the same scan. */
  packs: CrewRoleNameChoice[];
  skipped: SkippedCrewRolePack[];
};

/**
 * Look at `<checkoutDir>/personas/roles` without opening a picker.
 *
 * Read-only — it mints nothing and writes nothing. Rejects only when a folder
 * that is there cannot be read; a folder that is absent comes back as
 * `exists: false`, because a checkout with no role packs is an ordinary state
 * rather than a failure.
 */
export async function scanProjectRolePacks(
  checkoutDir: string,
): Promise<ProjectRolePacksScan> {
  return invokeTauri<ProjectRolePacksScan>(
    "scan_project_role_packs_directory",
    {
      checkoutDir,
    },
  );
}

/**
 * Open the OS folder picker for a folder of role packs and scan what was
 * picked. `null` if the operator cancelled.
 *
 * The scan comes back with the pick because the dialog asks a name per role
 * pack, and cannot render that list before something has read the folder.
 * Scanning writes nothing.
 */
export async function pickCrewRolePacksDirectory(): Promise<PickedCrewRolePacks | null> {
  return (
    (await invokeTauri<PickedCrewRolePacks | null>(
      "pick_crew_role_packs_directory",
    )) ?? null
  );
}

/**
 * Install every role pack in `directory` as an agent carrying its home role.
 *
 * `names` maps a role to the name that identity is installed under (plan D11);
 * a role missing from the map keeps its pack's own name. Idempotent: a pack
 * already installed refreshes its agent — renaming it in place when the name
 * changed — rather than minting a second one.
 */
export async function installCrewRolePacks(
  directory: string,
  names: Record<string, string> | null,
  expectedRelayUrl: string,
): Promise<InstallCrewRolePacksResponse> {
  return invokeTauri<InstallCrewRolePacksResponse>("install_crew_role_packs", {
    directory,
    names: names ?? null,
    expectedRelayUrl,
  });
}
