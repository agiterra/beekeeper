import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  crewRoleResultRows,
  crewRolesDroppedNotes,
  crewRolesFailureMessage,
  crewRolesFoundNothing,
  crewRolesInstalledToast,
  crewRolesSeatedNote,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_REFRESH_NOTE,
  INSTALL_CREW_ROLES_ROSTER_PLAN,
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
  seated: !["poker", "designer"].includes(role),
  ...overrides,
});

describe("install crew roles — result list", () => {
  it("renders one row per installed role, in the order the backend returned", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Crew roles",
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
      teamName: "Crew roles",
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
      teamName: "Crew roles",
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
      teamName: "Crew roles",
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
    for (const role of ["lead", "architect", "builder", "verifier", "runner"]) {
      assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, new RegExp(role));
    }
    assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, /poker and designer/);
    assert.match(INSTALL_CREW_ROLES_ROSTER_PLAN, /holds no seat/);
  });

  it("marks an installed-but-unseated row on the row itself", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Crew roles",
      seated: ["lead"],
      dropped: [],
      installed: [row("lead"), row("poker")],
      skipped: [],
    });
    assert.equal(rows[0].note, null);
    assert.equal(rows[1].note, INSTALL_CREW_ROLES_UNSEATED_NOTE);
    assert.equal(rows[1].note, "installed, but not seated in the crew");
  });

  it("a refreshed row that is also unseated says both", () => {
    const rows = crewRoleResultRows({
      teamId: "t",
      teamName: "Crew roles",
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

describe("install crew roles — seats actually written", () => {
  it("names the seats the crew holds, not the roster", () => {
    assert.equal(
      crewRolesSeatedNote({
        teamId: "t",
        teamName: "Crew roles",
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
        teamName: "Crew roles",
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
      teamName: "Crew roles",
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
        teamName: "Crew roles",
        seated: [],
        dropped: ["lead", "architect", "builder", "verifier", "runner"],
        installed: [],
        skipped: [],
      }),
      "No seats: this team holds no crew.",
    );
  });
});

// ── Which failure the operator is shown ──────────────────────────────────────
//
// "That folder could not be read: Error: the keychain is locked…" sent an
// operator with a locked keychain to look at their folder (poke finding F3).
// The backend now names the stage; each stage gets its own sentence, and a
// failure that names no stage is never blamed on the folder.

describe("install crew roles — failure copy", () => {
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

describe("install crew roles — empty and failed folders", () => {
  it("an install that found nothing is reported as nothing found", () => {
    const result = {
      teamId: "t",
      teamName: "Crew roles",
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
        teamName: "Crew roles",
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

describe("install crew roles — success toast", () => {
  it("names the count, the team and the roles", () => {
    assert.equal(
      crewRolesInstalledToast({
        teamId: "t",
        teamName: "Crew roles",
        seated: ["lead"],
        dropped: [],
        installed: [row("lead"), row("builder")],
        skipped: [],
      }),
      "Installed 2 crew roles into “Crew roles”: lead, builder.",
    );
  });
});
