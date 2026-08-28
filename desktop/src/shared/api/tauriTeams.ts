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
  /** `true` when this role is in the crew's default seat roster. */
  seated: boolean;
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
};

/** Which stage of an install failed, as `CrewRoleInstallFailure` names it. */
export type CrewRoleInstallFailureStage = "folder" | "keys" | "store";

/** A failed install: the stage that failed, and the cause verbatim. */
export type CrewRoleInstallFailure = {
  failure: CrewRoleInstallFailureStage;
  detail: string;
};

/** Open the OS folder picker for a folder of role packs. `null` if cancelled. */
export async function pickCrewRolePacksDirectory(): Promise<string | null> {
  return (
    (await invokeTauri<string | null>("pick_crew_role_packs_directory")) ?? null
  );
}

/**
 * Install every role pack in `directory` as an agent carrying its home role.
 *
 * `leadName` is the name the lead identity is minted under (plan D11); every
 * other role installs under its pack's own name. Idempotent: a pack already
 * installed refreshes its agent rather than minting a second one.
 */
export async function installCrewRolePacks(
  directory: string,
  leadName?: string | null,
): Promise<InstallCrewRolePacksResponse> {
  return invokeTauri<InstallCrewRolePacksResponse>("install_crew_role_packs", {
    directory,
    leadName: leadName ?? null,
  });
}
