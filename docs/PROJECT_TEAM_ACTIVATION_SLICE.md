# Project team activation — September 13 build brief

Brian authorized wiring publication, installation and lead handoff into the
existing project setup UI. Existing projects such as Tankloop need no existing
agents. Ordinary Solo sessions remain independent. Owner: Astra; implementation
delegated to Terra, verification to Sol. This is a build brief, not shipped state.

## Deliverable

From Roles → Set up project team, a person authors and checks a draft, selects
its saved version, publishes it as the project's shared source, installs the
project's roles on this computer, and opens a lead session grounded in that
project and the adopted revision. Report these as separate observed outcomes.
Never describe a saved draft as a published or installed team.

The first working path uses the publication contract's explicit
`output: {kind: "snapshot", snapshotId}`. The Publish action authorizes this
exact checked version. It does not pretend to be automatic model-completion
verification. Automatic readiness/correction orchestration is deferred; do not
implement the authoring-output protocol merely to support this selected-snapshot
path. Preserve the other applicable invariants in PROJECT_TEAM_PUBLICATION_IMPL:
owner/project/community binding, durable same-request retry, captured source
expectation, immutable snapshot bytes, isolated candidate ref, source CAS and
honest uncertain outcomes. No moving-main update or production deployment.

Existing-source replacement must not silently seed over project procedures.
Require matching source provenance or explain why this draft cannot replace the
current source. Never weaken that check to make the absent-source demo pass.

Installation must resolve the adopted repository/SHA/path and distinguish roles
available on this computer from shared role definitions. Reuse existing pack
installation and managed-identity/seat APIs. A role file grants no new relay
access. Lead handoff uses the project repository instructions and exact selected
role; retries must not mint duplicate agents or start duplicate lead sessions.
If the project lacks a session channel, offer the existing creation path with a
clear next action; do not hide the limitation behind a disabled button.

## Ownership

- Host lane: all new publication native modules and tests; necessary native
  setup snapshot/context accessors and handler registration. Send IPC projections
  to the UI lane early. Own native files exclusively.
- UI lane: roles setup UI/models/API and targeted tests; installation and lead
  handoff composition using existing APIs. Request missing native seams from the
  host lane, rather than editing their files. Own these frontend files exclusively.
- Astra: this brief, integration decisions and final review/commit.
- Sol: existing candidate CI/landing verification first; then this slice's bounded
  integration check when both lanes finish.

## Validation budget

Focused tests must cover exact bytes, retry identity, changed source/community,
uncertain publication, and no duplicate installation/lead launch. One browser
walkthrough covers draft → publication → installation → lead handoff plus Solo
remaining independent. Run focused checks as changes require. No full smoke run
or repeated repository-wide test cycles for this slice. Coordinate any build or
browser run so it cannot overwrite another run's bundle. The separate dense chat
history failure is accepted as unresolved and is outside this slice.

Do not modify live Tankloop configuration during fixture verification. No lane
commits, pushes or installs the application; one finalizer handles integration.
