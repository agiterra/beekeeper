import type {
  CrewRoleInstallFailure,
  CrewRoleInstallFailureStage,
  CrewRoleNameChoice,
  InstallCrewRolePacksResponse,
  ProjectRolePacksScan,
} from "@/shared/api/tauriTeams";

/**
 * Copy and result derivations for the team-role installer dialog.
 *
 * Kept out of the component so every sentence the operator reads is a value a
 * test can assert. The strings are verbatim from the front-door spec — a lane
 * that invents copy invents a comfortable version.
 */

export const INSTALL_CREW_ROLES_TITLE = "Install team roles";

export const INSTALL_CREW_ROLES_BODY =
  "Pick a folder of role packs. Every pack whose persona declares a role becomes one agent on this computer — carrying that role and that pack — and they all join one team you can launch.";

export const INSTALL_CREW_ROLES_CHOOSE_FOLDER = "Choose folder…";

/**
 * The label over a folder the dialog chose for the operator (ledger 85).
 *
 * Shown only when the path came from the project's own checkout, so a folder
 * the operator picked is never captioned as the project's.
 */
export const INSTALL_CREW_ROLES_PROJECT_FOLDER_LABEL =
  "The project's role packs";

/**
 * The project this dialog was opened in has no checkout directory on this
 * computer, so there is nowhere to look for its packs.
 *
 * Names the setting that fixes it rather than only saying no: the checkout
 * directory is a per-computer project setting (Andy's 75f9fc8f), and an
 * operator who has never opened that tab has no way to guess that from
 * "No folder chosen".
 */
export const INSTALL_CREW_ROLES_NO_CHECKOUT =
  "This project has no checkout directory yet — set one in Project settings, or choose a folder";

/**
 * Why the project's folder was not pre-chosen, or `null` when it was.
 *
 * Absent and empty are told apart, because they are different problems: the
 * first says this checkout has no role packs folder at all, the second says
 * the folder is there and nothing in it is a role pack. Both name the exact
 * path, so nobody has to guess where the dialog looked.
 */
export function crewRolesProjectFolderNote(
  scan: ProjectRolePacksScan,
): string | null {
  if (!scan.exists) {
    return `${scan.directory} is not there, so this project has no role packs to install — choose a folder instead.`;
  }
  if (scan.packs.length === 0) {
    return `${scan.directory} holds no role packs — choose a folder instead.`;
  }
  return null;
}

/** Heading of the per-identity name fields (plan D11, ledger 84). */
export const INSTALL_CREW_ROLES_TEAM_NAMES_LABEL = "Name your team";

/**
 * What the name fields do, said plainly.
 *
 * Every one of these is an identity a person addresses by name and that is
 * minted once — so typing a new name over one that is already installed
 * renames *it*, here and on the relay, instead of minting a second identity
 * beside it. Ledger 80 (e) is the reason the relay half is spelled out: a
 * session header kept reading the old name because nothing republished the
 * identity's profile.
 */
export const INSTALL_CREW_ROLES_TEAM_NAMES_HINT =
  "These are the names you will address, so it is worth giving them one. " +
  "A role already installed here is renamed in place — its agent, its card " +
  "and its relay profile — never installed twice. Leave a field alone and " +
  "that identity keeps the name it has.";

/** One name field, derived from the scan the folder picker returned. */
export type CrewRoleNameField = {
  /** The role, and the key this field's name is submitted under. */
  role: string;
  /** Title-cased role, shown beside the field. */
  label: string;
  /** The name the field starts on. */
  defaultName: string;
  /** `true` when an identity is already installed from this pack. */
  installed: boolean;
  packDir: string;
  personaName: string;
};

/**
 * The name fields to render, in the scan's own order — the lead first, then
 * the rest of the roster, then the roles that install unseated.
 *
 * Derived from the scan rather than from a constant roster: a folder holding
 * three packs gets three fields, and a field can never name a role this
 * install will not touch.
 */
export function crewRoleNameFields(
  packs: CrewRoleNameChoice[],
): CrewRoleNameField[] {
  return packs.map((pack) => ({
    role: pack.role,
    label: crewRoleLabel(pack.role),
    defaultName: pack.defaultName,
    installed: pack.installed,
    packDir: pack.packDir,
    personaName: pack.personaName,
  }));
}

/**
 * The role→name map the install carries.
 *
 * Every scanned pack gets an entry, so the backend never has to guess what an
 * absent key meant. A field the operator blanked falls back to its default —
 * an identity with no name is not a thing this installer can write.
 */
export function crewRoleNamesMap(
  packs: CrewRoleNameChoice[],
  values: Record<string, string>,
): Record<string, string> {
  const names: Record<string, string> = {};
  for (const pack of packs) {
    const typed = (values[pack.role] ?? "").trim();
    names[pack.role] = typed.length > 0 ? typed : pack.defaultName;
  }
  return names;
}

/**
 * What the installer will *try* to do, shown before it has done anything.
 *
 * Deliberately in the future tense and hedged: at this point nothing has been
 * scanned, so the roster is a plan, not a claim about seats. Once the install
 * returns, [`crewRolesSeatedNote`] replaces it with what actually happened.
 */
export const INSTALL_CREW_ROLES_ROSTER_PLAN =
  "Seats are filled from this roster, in order: lead, architect, builder, runner. A role whose pack is not in the folder holds no seat. The poker, designer and verifier packs install as agents but are not seated: the poker drives the built app, the designer works before a team launches, and every seat of one launch runs on the single provider you select — so a verifier seated here would share its builders’ model vendor, which a launch refuses.";

export const INSTALL_CREW_ROLES_REFRESH_NOTE =
  "already installed from this pack — role and pack link refreshed";

/**
 * Marks a row whose identity this run renamed.
 *
 * Says the relay half out loud: the rename is only real to everyone else once
 * the identity's kind:0 profile carries it, and an install that renamed a seat
 * without republishing is exactly the state ledger 80 (e) found.
 */
export const INSTALL_CREW_ROLES_RENAMED_NOTE = "renamed; profile republished";

/**
 * Marks a renamed row on a run whose profile publishes did not all land.
 *
 * The backend reports one sentence for the whole run, not one per identity, so
 * no row can claim its own publish succeeded. Every renamed row hedges rather
 * than one of them lying.
 */
export const INSTALL_CREW_ROLES_RENAMED_UNPUBLISHED_NOTE =
  "renamed here; the relay may still know it by the old name";

/**
 * Marks a row that installed but holds no seat.
 *
 * Without it, `poker`, `designer` and `verifier` — installed on purpose and
 * seated on purpose never — read exactly like the roster roles above them.
 */
export const INSTALL_CREW_ROLES_UNSEATED_NOTE =
  "installed, but not seated in the team";

export const INSTALL_CREW_ROLES_NOTHING_FOUND =
  "No role packs in that folder. A role pack is a directory holding .plugin/plugin.json whose persona declares “role:” in its frontmatter.";

/** The menu entry that opens this dialog. */
export const INSTALL_CREW_ROLES_MENU_LABEL = "Install team roles…";

/** A folder that could not be read, rendered verbatim with its cause. */
export function crewRolesUnreadableFolder(error: string): string {
  const message = error.trim();
  return message.startsWith("That folder could not be read")
    ? message
    : `That folder could not be read: ${message}`;
}

/** The sentence that belongs to each failure stage. */
const CREW_ROLE_FAILURE_SENTENCE: Record<CrewRoleInstallFailureStage, string> =
  {
    folder: "That folder could not be read",
    keys: "No agent key could be minted, so nothing was installed",
    store: "The packs were read, but this computer could not save them",
  };

/** `true` when `value` is the structured failure the backend returns. */
function isCrewRoleInstallFailure(
  value: unknown,
): value is CrewRoleInstallFailure {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as { failure?: unknown; detail?: unknown };
  return (
    typeof candidate.detail === "string" &&
    typeof candidate.failure === "string" &&
    candidate.failure in CREW_ROLE_FAILURE_SENTENCE
  );
}

/** The structured failure behind a thrown value, if there is one. */
function readCrewRoleInstallFailure(
  cause: unknown,
): CrewRoleInstallFailure | null {
  if (isCrewRoleInstallFailure(cause)) return cause;
  // `invokeTauri` wraps a non-Error rejection in a `TauriInvokeError` whose
  // `payload` is the value Rust serialised.
  const payload = (cause as { payload?: unknown } | null)?.payload;
  if (isCrewRoleInstallFailure(payload)) return payload;
  return null;
}

/** A thrown value's own words, with the `Error:` the runtime prepends removed. */
function crewRolesCauseDetail(cause: unknown): string {
  if (cause instanceof Error) return cause.message.trim();
  const text = String(cause).trim();
  return text.startsWith("Error:") ? text.slice("Error:".length).trim() : text;
}

/**
 * What to tell the operator about a failed install.
 *
 * The stage comes from the backend, never from the shape of the message: the
 * dialog used to wrap *every* failure in "That folder could not be read:",
 * which sent an operator whose keychain was locked to go and look at their
 * folder. A failure that names no stage is reported as a failure, and blamed
 * on nothing.
 */
export function crewRolesFailureMessage(cause: unknown): string {
  const failure = readCrewRoleInstallFailure(cause);
  if (!failure) return `The install failed: ${crewRolesCauseDetail(cause)}`;
  return `${CREW_ROLE_FAILURE_SENTENCE[failure.failure]}: ${failure.detail.trim()}`;
}

/**
 * The seats the install actually wrote.
 *
 * Never the roster: an install missing a roster pack writes a smaller crew,
 * and a constant here would name a seat the team does not hold.
 */
export function crewRolesSeatedNote(
  result: InstallCrewRolePacksResponse,
): string {
  if (result.seated.length === 0) return "No seats: this team holds none.";
  return `Seated: ${result.seated.join(", ")}.`;
}

/** One line per roster role that was dropped for want of a pack. */
export function crewRolesDroppedNotes(
  result: InstallCrewRolePacksResponse,
): string[] {
  return result.dropped.map(
    (role) => `${role}: no pack installed, so it holds no seat.`,
  );
}

/** Title-cased role for display: `lead` → `Lead`. */
export function crewRoleLabel(role: string): string {
  if (!role) return role;
  return role.charAt(0).toUpperCase() + role.slice(1);
}

/** The success toast. `{n}` is the count, `{roles}` the roles in seat order. */
export function crewRolesInstalledToast(
  result: InstallCrewRolePacksResponse,
): string {
  const roles = result.installed.map((row) => row.role).join(", ");
  return `Installed ${result.installed.length} team roles into “${result.teamName}”: ${roles}.`;
}

/** One line of the result list. */
export type CrewRoleResultRow =
  | { kind: "installed"; text: string; note: string | null; seated: boolean }
  | { kind: "skipped"; text: string };

/**
 * The result list, in the order the dialog renders it: every installed role,
 * then every skipped path with its reason.
 *
 * A refreshed row says so — an operator who runs the installer twice must be
 * able to tell "nothing new happened" from "seven fresh agents appeared" — and
 * a row that installed without taking a seat says that too, so the two roles
 * that are deliberately unseated do not read like the five that are.
 */
export function crewRoleResultRows(
  result: InstallCrewRolePacksResponse,
): CrewRoleResultRow[] {
  const rows: CrewRoleResultRow[] = result.installed.map((row) => {
    const notes: string[] = [];
    // A rename subsumes the refresh note: an identity can only be renamed if
    // it was already installed, and stacking both says the same fact twice.
    if (row.renamed)
      notes.push(
        result.profileSyncError
          ? INSTALL_CREW_ROLES_RENAMED_UNPUBLISHED_NOTE
          : INSTALL_CREW_ROLES_RENAMED_NOTE,
      );
    else if (row.refreshed) notes.push(INSTALL_CREW_ROLES_REFRESH_NOTE);
    if (!row.seated) notes.push(INSTALL_CREW_ROLES_UNSEATED_NOTE);
    return {
      kind: "installed" as const,
      text: `${crewRoleLabel(row.role)} — ${row.agentName}`,
      note: notes.length > 0 ? notes.join("; ") : null,
      seated: row.seated,
    };
  });
  for (const skipped of result.skipped) {
    rows.push({ kind: "skipped", text: `${skipped.path}: ${skipped.reason}` });
  }
  return rows;
}

/**
 * One result row as a single line: `Designer — Banksy (renamed; profile
 * republished)`.
 *
 * Lives here rather than in the component so the sentence an operator reads is
 * a value a test can assert.
 */
export function crewRoleResultLine(row: CrewRoleResultRow): string {
  if (row.kind === "skipped") return row.text;
  return row.note ? `${row.text} (${row.note})` : row.text;
}

/** `true` when the folder held no role pack at all. */
export function crewRolesFoundNothing(
  result: InstallCrewRolePacksResponse,
): boolean {
  return result.installed.length === 0;
}
