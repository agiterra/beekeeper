import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  crewRoleNameFields,
  crewRoleNamesMap,
  crewRoleResultLine,
  crewRoleResultRows,
  crewRolesDroppedNotes,
  crewRolesFailureMessage,
  crewRolesFoundNothing,
  crewRolesInstalledToast,
  crewRolesSeatedNote,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_REFRESH_NOTE,
  INSTALL_CREW_ROLES_RENAMED_NOTE,
  INSTALL_CREW_ROLES_RENAMED_UNPUBLISHED_NOTE,
  INSTALL_CREW_ROLES_ROSTER_PLAN,
  INSTALL_CREW_ROLES_TEAM_NAMES_HINT,
  INSTALL_CREW_ROLES_TEAM_NAMES_LABEL,
  INSTALL_CREW_ROLES_UNSEATED_NOTE,
} from "./installCrewRolesCopy.ts";

const row = (role, overrides = {}) => ({
  personaId: `crew-role:${role}`,
  personaName: role,
  role,
  agentPubkey: "0".repeat(64),
  agentName: role.charAt(0).toUpperCase() + role.slice(1),
  packDir: `/packs/${role}`,
  refreshed: false,
  renamed: false,
  seated: !["poker", "designer"].includes(role),
  ...overrides,
});

describe("install team roles — result list", () => {
  it("renders one row per installed role, in the order the backend returned", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead"), row("builder")],
      skipped: [],
    });
    assert.deepEqual(
      rows.map((entry) => entry.text),
      ["Lead — Lead", "Builder — Builder"],
    );
    assert.ok(rows.every((entry) => entry.kind === "installed"));
    assert.ok(rows.every((entry) => entry.note === null));
  });

  it("says so when a row was refreshed rather than minted", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead", { refreshed: true }), row("builder")],
      skipped: [],
    });
    assert.equal(rows[0].note, INSTALL_CREW_ROLES_REFRESH_NOTE);
    assert.equal(
      rows[0].note,
      "already installed from this pack — role and pack link refreshed",
    );
    assert.equal(rows[1].note, null);
  });

  it("renders skipped paths after the installed rows, with their reason", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead")],
      skipped: [
        {
          path: "/packs/notes",
          reason: "no persona in this pack declares a role, so it was skipped.",
        },
      ],
    });
    assert.equal(rows.length, 2);
    assert.equal(rows[1].kind, "skipped");
    assert.equal(
      rows[1].text,
      "/packs/notes: no persona in this pack declares a role, so it was skipped.",
    );
  });

  it("marks the unseated roles as unseated, so the roster note is not the only claim", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead"), row("poker"), row("designer")],
      skipped: [],
    });
    assert.deepEqual(
      rows.map((entry) => entry.seated),
      [true, false, false],
    );
  });

  it("the pre-install plan names the roster and says a missing pack holds no seat", () => {
    for (const role of ["lead", "architect", "builder", "runner"]) {
      assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, new RegExp(role));
    }
    assert.match(
      INSTALL_CREW_ROLES_ROSTER_PLAN,
      /poker, designer and verifier packs install as agents but are not seated/,
    );
    // A verifier is not seated *and the plan says why*: the launch would
    // refuse it for sharing its builders' vendor (SESSION_STATE item 77, F7).
    assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, /model vendor/);
    assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, /holds no seat/);
  });

  it("marks an installed-but-unseated row on the row itself", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead"), row("poker")],
      skipped: [],
    });
    assert.equal(rows[0].note, null);
    assert.equal(rows[1].note, INSTALL_CREW_ROLES_UNSEATED_NOTE);
    assert.equal(rows[1].note, "installed, but not seated in the team");
  });

  it("a refreshed row that is also unseated says both", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("designer", { refreshed: true })],
      skipped: [],
    });
    assert.equal(
      rows[0].note,
      `${INSTALL_CREW_ROLES_REFRESH_NOTE}; ${INSTALL_CREW_ROLES_UNSEATED_NOTE}`,
    );
  });
});

// ── What a partial install is allowed to claim ───────────────────────────────
//
// The dialog used to print "Seated by default: lead, architect, builder,
// verifier, runner." verbatim under a result list holding no verifier row
// (SESSION_STATE item 76, poke finding F2). Both sentences below come off the
// backend's own answer, so neither can outlive the install that produced it.

describe("install team roles — seats actually written", () => {
  it("names the seats the team holds, not the roster", () => {
    assert.equal(
      crewRolesSeatedNote({
        teamId: "t",
        teamName: "Team roles",
        seated: ["lead", "architect", "builder", "runner"],
        dropped: ["verifier"],
        installed: [],
        skipped: [],
      }),
      "Seated: lead, architect, builder, runner.",
    );
  });

  it("says plainly that a dropped roster role holds no seat", () => {
    assert.deepEqual(
      crewRolesDroppedNotes({
        teamId: "t",
        teamName: "Team roles",
        seated: ["lead"],
        dropped: ["verifier", "runner"],
        installed: [],
        skipped: [],
      }),
      [
        "verifier: no pack installed, so it holds no seat.",
        "runner: no pack installed, so it holds no seat.",
      ],
    );
  });

  it("a full roster drops nothing and says nothing about drops", () => {
    const result = {
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead", "architect", "builder", "verifier", "runner"],
      dropped: [],
      installed: [],
      skipped: [],
    };
    assert.deepEqual(crewRolesDroppedNotes(result), []);
    assert.equal(
      crewRolesSeatedNote(result),
      "Seated: lead, architect, builder, verifier, runner.",
    );
  });

  it("an install that seated nobody says that, rather than naming seats", () => {
    assert.equal(
      crewRolesSeatedNote({
        teamId: "t",
        teamName: "Team roles",
        seated: [],
        dropped: ["lead", "architect", "builder", "verifier", "runner"],
        installed: [],
        skipped: [],
      }),
      "No seats: this team holds none.",
    );
  });
});

// ── Which failure the operator is shown ──────────────────────────────────────
//
// "That folder could not be read: Error: the keychain is locked…" sent an
// operator with a locked keychain to look at their folder (poke finding F3).
// The backend now names the stage; each stage gets its own sentence, and a
// failure that names no stage is never blamed on the folder.

describe("install team roles — failure copy", () => {
  it("blames the folder only for a folder failure", () => {
    assert.equal(
      crewRolesFailureMessage({
        failure: "folder",
        detail: "permission denied",
      }),
      "That folder could not be read: permission denied",
    );
  });

  it("names the keychain for a key failure, and never the folder", () => {
    const message = crewRolesFailureMessage({
      failure: "keys",
      detail: "the keychain is locked, so a new agent key could not be minted",
    });
    assert.match(
      message,
      /^No agent key could be minted, so nothing was installed: /,
    );
    assert.match(message, /the keychain is locked/);
    assert.doesNotMatch(message, /folder/);
  });

  it("names the store for a save failure, and never the folder", () => {
    const message = crewRolesFailureMessage({
      failure: "store",
      detail: "no space left on device",
    });
    assert.match(
      message,
      /^The packs were read, but this computer could not save them: /,
    );
    assert.doesNotMatch(message, /folder/);
  });

  it("reads the stage off a thrown Tauri error's payload", () => {
    const error = new Error('{"failure":"keys","detail":"keychain is locked"}');
    error.payload = { failure: "keys", detail: "keychain is locked" };
    assert.match(
      crewRolesFailureMessage(error),
      /^No agent key could be minted/,
    );
  });

  it("a failure that names no stage is not blamed on anything", () => {
    const message = crewRolesFailureMessage(new Error("the bridge went away"));
    assert.equal(message, "The install failed: the bridge went away");
    assert.doesNotMatch(message, /folder/);
  });

  it("never leaks the thrown value's Error: prefix", () => {
    assert.doesNotMatch(
      crewRolesFailureMessage(new Error("boom")),
      /Error:/,
      "String(cause) put 'Error:' in front of the operator's sentence",
    );
  });
});

describe("install team roles — empty and failed folders", () => {
  it("an install that found nothing is reported as nothing found", () => {
    const result = {
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [],
      skipped: [{ path: "/packs/x", reason: "no persona…" }],
    };
    assert.equal(crewRolesFoundNothing(result), true);
    assert.match(
      INSTALL_CREW_ROLES_NOTHING_FOUND,
      /^No role packs in that folder\./,
    );
    assert.match(INSTALL_CREW_ROLES_NOTHING_FOUND, /\.plugin\/plugin\.json/);
  });

  it("an install that produced roles is not 'nothing found'", () => {
    assert.equal(
      crewRolesFoundNothing({
        teamId: "t",
        teamName: "Team roles",
        seated: ["lead"],
        dropped: [],
        installed: [row("lead")],
        skipped: [],
      }),
      false,
    );
  });

  it("an unreadable folder keeps its cause and is never doubled up", () => {
    assert.equal(
      crewRolesUnreadableFolder("permission denied"),
      "That folder could not be read: permission denied",
    );
    assert.equal(
      crewRolesUnreadableFolder(
        "That folder could not be read: permission denied",
      ),
      "That folder could not be read: permission denied",
    );
  });
});

describe("install team roles — success toast", () => {
  it("names the count, the team and the roles", () => {
    assert.equal(
      crewRolesInstalledToast({
        teamId: "t",
        teamName: "Team roles",
        seated: ["lead"],
        dropped: [],
        installed: [row("lead"), row("builder")],
        skipped: [],
      }),
      "Installed 2 team roles into “Team roles”: lead, builder.",
    );
  });
});

// ── Naming your team (plan D11, ledger 84) ───────────────────────────────────
//
// The installer asked for one name — the lead's — so the designer identity a
// person addresses as "Banksy" was `designer` on this computer and `designer`
// on the relay. The dialog now asks a name per role pack the scan found.

const choice = (role, overrides = {}) => ({
  role,
  personaName: role,
  packDir: `/packs/${role}`,
  defaultName: role,
  installed: false,
  ...overrides,
});

describe("install team roles — naming your team", () => {
  it("renders one field per pack the scan found, in the scan's order", () => {
    const fields = crewRoleNameFields([
      choice("lead"),
      choice("builder"),
      choice("designer"),
    ]);
    assert.deepEqual(
      fields.map((field) => field.role),
      ["lead", "builder", "designer"],
    );
    assert.deepEqual(
      fields.map((field) => field.label),
      ["Lead", "Builder", "Designer"],
    );
  });

  it("a field defaults to the name that identity already carries here", () => {
    const fields = crewRoleNameFields([
      choice("lead", { defaultName: "Keystone", installed: true }),
      choice("designer"),
    ]);
    assert.equal(fields[0].defaultName, "Keystone");
    assert.equal(fields[0].installed, true);
    assert.equal(fields[1].defaultName, "designer");
    assert.equal(fields[1].installed, false);
  });

  it("an empty scan renders no fields at all, rather than a lead field", () => {
    assert.deepEqual(crewRoleNameFields([]), []);
  });

  it("submits a role→name map covering every field", () => {
    const packs = [
      choice("lead", { defaultName: "Keystone", installed: true }),
      choice("designer"),
      choice("builder"),
    ];
    assert.deepEqual(crewRoleNamesMap(packs, { designer: "Banksy" }), {
      lead: "Keystone",
      designer: "Banksy",
      builder: "builder",
    });
  });

  it("a blank or whitespace field falls back to the default, never to nothing", () => {
    const packs = [choice("lead", { defaultName: "Keystone" })];
    assert.deepEqual(crewRoleNamesMap(packs, { lead: "   " }), {
      lead: "Keystone",
    });
    assert.deepEqual(crewRoleNamesMap(packs, { lead: "" }), {
      lead: "Keystone",
    });
    assert.deepEqual(crewRoleNamesMap(packs, { lead: "  Banksy " }), {
      lead: "Banksy",
    });
  });

  it("the label and hint are about the team, not only the lead", () => {
    assert.equal(INSTALL_CREW_ROLES_TEAM_NAMES_LABEL, "Name your team");
    assert.match(INSTALL_CREW_ROLES_TEAM_NAMES_HINT, /rename/i);
    // The rename is on the wire, not only on this computer — the sentence has
    // to say so, because that is the fix ledger 80 (e) is about.
    assert.match(INSTALL_CREW_ROLES_TEAM_NAMES_HINT, /profile|relay/i);
    assert.doesNotMatch(INSTALL_CREW_ROLES_TEAM_NAMES_HINT, /\bcrew\b/i);
  });
});

// ── What a rename is allowed to claim ────────────────────────────────────────

describe("install team roles — a renamed identity says so", () => {
  it("reads 'Designer — Banksy (renamed; profile republished)'", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [
        row("designer", {
          agentName: "Banksy",
          refreshed: true,
          renamed: true,
          seated: true,
        }),
      ],
      skipped: [],
    });
    assert.equal(
      crewRoleResultLine(rows[0]),
      "Designer — Banksy (renamed; profile republished)",
    );
    assert.equal(
      INSTALL_CREW_ROLES_RENAMED_NOTE,
      "renamed; profile republished",
    );
  });

  it("a rename replaces the refresh note rather than stacking on it", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [
        row("lead", { agentName: "Keystone", refreshed: true, renamed: true }),
      ],
      skipped: [],
    });
    assert.equal(rows[0].note, INSTALL_CREW_ROLES_RENAMED_NOTE);
    assert.doesNotMatch(rows[0].note, /already installed/);
  });

  it("a renamed unseated role still says it holds no seat", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [
        row("designer", {
          agentName: "Banksy",
          refreshed: true,
          renamed: true,
        }),
      ],
      skipped: [],
    });
    assert.equal(
      rows[0].note,
      `${INSTALL_CREW_ROLES_RENAMED_NOTE}; ${INSTALL_CREW_ROLES_UNSEATED_NOTE}`,
    );
  });

  it("an identity nobody renamed never claims a republish", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead"), row("builder", { refreshed: true })],
      skipped: [],
    });
    assert.equal(crewRoleResultLine(rows[0]), "Lead — Lead");
    assert.equal(
      crewRoleResultLine(rows[1]),
      `Builder — Builder (${INSTALL_CREW_ROLES_REFRESH_NOTE})`,
    );
  });

  it("a skipped row is rendered verbatim, with no note bracket", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Team roles",
      seated: [],
      dropped: [],
      installed: [],
      skipped: [{ path: "/packs/notes", reason: "no persona…" }],
    });
    assert.equal(crewRoleResultLine(rows[0]), "/packs/notes: no persona…");
  });
});

// ── A publish that did not land ──────────────────────────────────────────────
//
// The install writes the stores first and republishes the identities' kind:0
// profiles after. When a publish fails the stores are still correct and the
// relay is not — so a row claiming "profile republished" would be the exact
// untruth ledger 80 (e) is about.

describe("install team roles — a rename whose publish failed", () => {
  const failed = (installed) => ({
    teamId: "t",
    teamName: "Team roles",
    seated: ["lead"],
    dropped: [],
    installed,
    skipped: [],
    profileSyncError:
      "these identities were installed but the relay still knows them by " +
      "their previous name — Banksy: relay unreachable",
  });

  it("a renamed row hedges instead of claiming a republish", () => {
    const rows = crewRoleResultRows(
      failed([
        row("designer", {
          agentName: "Banksy",
          refreshed: true,
          renamed: true,
          seated: true,
        }),
      ]),
    );
    assert.equal(rows[0].note, INSTALL_CREW_ROLES_RENAMED_UNPUBLISHED_NOTE);
    assert.doesNotMatch(rows[0].note, /republished/);
    assert.equal(
      crewRoleResultLine(rows[0]),
      "Designer — Banksy (renamed here; the relay may still know it by the old name)",
    );
  });

  it("a row nobody renamed is unaffected by the publish failure", () => {
    const rows = crewRoleResultRows(failed([row("lead", { refreshed: true })]));
    assert.equal(rows[0].note, INSTALL_CREW_ROLES_REFRESH_NOTE);
  });

  it("the two rename notes are different sentences", () => {
    assert.notEqual(
      INSTALL_CREW_ROLES_RENAMED_NOTE,
      INSTALL_CREW_ROLES_RENAMED_UNPUBLISHED_NOTE,
    );
  });
});
