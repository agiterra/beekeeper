import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  crewRoleResultRows,
  crewRolesFoundNothing,
  crewRolesInstalledToast,
  crewRolesUnreadableFolder,
  INSTALL_CREW_ROLES_NOTHING_FOUND,
  INSTALL_CREW_ROLES_REFRESH_NOTE,
  INSTALL_CREW_ROLES_ROSTER_NOTE,
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
      installed: [row("lead"), row("poker"), row("designer")],
      skipped: [],
    });
    assert.deepEqual(
      rows.map((entry) => entry.seated),
      [true, false, false],
    );
  });

  it("the roster note names exactly the five seated roles and both unseated ones", () => {
    for (const role of ["lead", "architect", "builder", "verifier", "runner"]) {
      assert.match(INSTALL_CREW_ROLES_ROSTER_NOTE, new RegExp(role));
    }
    assert.match(INSTALL_CREW_ROLES_ROSTER_NOTE, /poker and designer/);
    assert.match(INSTALL_CREW_ROLES_ROSTER_NOTE, /not seated/);
  });
});

describe("install crew roles — empty and failed folders", () => {
  it("an install that found nothing is reported as nothing found", () => {
    const result = {
      teamId: "t",
      teamName: "Crew roles",
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
        installed: [row("lead"), row("builder")],
        skipped: [],
      }),
      "Installed 2 crew roles into “Crew roles”: lead, builder.",
    );
  });
});
