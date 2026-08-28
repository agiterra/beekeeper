import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  crewRoleResultRows,
  crewRolesLeadName,
  crewRolesDroppedNotes,
  crewRolesFailureMessage,
  crewRolesFoundNothing,
  crewRolesInstalledToast,
  crewRolesSeatedNote,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_LEAD_NAME_DEFAULT,
  INSTALL_CREW_ROLES_LEAD_NAME_HINT,
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

// ── Naming the lead (plan D11) ───────────────────────────────────────────────
//
// A lead is an identity a person names once ("Keystone"), not a role label.
// The installer named every identity after its role, so every lead on every
// computer was "Lead".

describe("install team roles — naming the lead", () => {
  it("defaults to the lead pack's own name and trims what is typed", () => {
    assert.equal(crewRolesLeadName(""), INSTALL_CREW_ROLES_LEAD_NAME_DEFAULT);
    assert.equal(
      crewRolesLeadName("   "),
      INSTALL_CREW_ROLES_LEAD_NAME_DEFAULT,
    );
    assert.equal(crewRolesLeadName("  Keystone "), "Keystone");
    assert.equal(INSTALL_CREW_ROLES_LEAD_NAME_DEFAULT, "Lead");
  });

  it("says the field is only about the lead", () => {
    assert.match(INSTALL_CREW_ROLES_LEAD_NAME_HINT, /lead/i);
    // The other roles are renamed where every other agent is renamed.
    assert.match(INSTALL_CREW_ROLES_LEAD_NAME_HINT, /rename/i);
  });
});
