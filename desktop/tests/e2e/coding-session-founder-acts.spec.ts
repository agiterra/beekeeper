import { expect, test } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import {
  BUILDER_ACTOR,
  CO_FOUNDER,
  HEAD_SHA,
  LAND_REFUSAL_NO_VERDICT,
  REPO_REF,
  founderActMission,
  landFounders,
  landFounderPushResponse,
  landReadyResponse,
  landRefusedResponse,
  openFounderActApp,
  openMissionLens,
  repoAnnouncementEvent,
} from "./helpers/codingSessionFounderActAssertions";

/**
 * The founder's two acts, on the screen that shows the facts they act on.
 *
 * Live run 2 had the founder answering three rulings from a terminal while this
 * queue displayed every one of them. Live run 3 ended with a verifier's FAIL on
 * the wire and the branch on `main` anyway. Every assertion below is against
 * the real components, driven through the mock Tauri bridge and the mock relay
 * — the publish path is the app's own build → keyring sign → relay publish.
 */

const SHOTS = "test-results/l8-founder-acts";

test("L8.1: an open ruling held on the viewer carries its own options and the Answer control", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, { foldResponse: mission.foldResponse });
  await openMissionLens(page, mission);

  const founderRow = page
    .getByTestId("mission-decision-row")
    .filter({ hasText: "Land the commit now" });
  await expect(founderRow).toHaveAttribute("data-decision-state", "open");
  await expect(founderRow).toContainText("Open · held on you");

  // The buttons are the request's own signed options, index for index.
  const options = founderRow.getByTestId("decision-answer-option");
  await expect(options).toHaveCount(3);
  await expect(options.nth(0)).toHaveText("Land it now");
  await expect(options.nth(2)).toHaveText(
    "Hold until the relay carries the condition key",
  );
  await expect(options.nth(0)).toBeEnabled();
  await expect(
    founderRow.getByTestId("decision-answer-recommendation"),
  ).toContainText("Asker recommends: Land it now");
  await expect(founderRow.getByTestId("decision-answer-submit")).toBeVisible();

  await founderRow.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await founderRow.screenshot({ path: `${SHOTS}/01-decision-answer-open.png` });
});

test("L8.1: a ruling held on somebody else is disabled with §1l's sentence, never hidden", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, { foldResponse: mission.foldResponse });
  await openMissionLens(page, mission);

  const actorRow = page
    .getByTestId("mission-decision-row")
    .filter({ hasText: "worktree kept after landing" });
  await expect(actorRow).toHaveAttribute("data-decision-state", "open");
  await expect(actorRow).toContainText("Open · held on Bob");
  // Present, and every control off.
  await expect(actorRow.getByTestId("decision-answer-form")).toBeVisible();
  await expect(
    actorRow.getByTestId("decision-answer-held-elsewhere"),
  ).toHaveText(
    "This ruling is held on Bob, so only they can answer it. You can read it here.",
  );
  for (const testId of [
    "decision-answer-option",
    "decision-answer-choice",
    "decision-answer-note",
    "decision-answer-submit",
  ]) {
    await expect(actorRow.getByTestId(testId).first()).toBeDisabled();
  }
  expect(BUILDER_ACTOR).toHaveLength(64);

  await actorRow.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await actorRow.screenshot({
    path: `${SHOTS}/02-decision-answer-disabled.png`,
  });
});

test("L8.4: the condition field appears only where this build's wire carries the key", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    supportsCondition: true,
  });
  await openMissionLens(page, mission);

  const founderRow = page
    .getByTestId("mission-decision-row")
    .filter({ hasText: "Land the commit now" });
  await expect(
    founderRow.getByTestId("decision-answer-condition"),
  ).toBeVisible();
  await expect(
    founderRow.getByTestId("decision-answer-condition-hint"),
  ).toHaveText(
    "Name the class this ruling covers, so it does not have to be asked again for the next commit.",
  );
  await expect(
    founderRow.getByTestId("decision-answer-condition-counter"),
  ).toHaveText("0/512 bytes");
  const condition =
    "every commit on lane/batch3-l8-founder that keeps the gate green";
  await founderRow.getByTestId("decision-answer-condition").fill(condition);
  // Bytes, not characters — the bound `buzz-core` enforces is a byte bound.
  const bytes = new TextEncoder().encode(condition).length;
  await expect(
    founderRow.getByTestId("decision-answer-condition-counter"),
  ).toHaveText(`${bytes}/512 bytes`);

  await founderRow.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await founderRow.screenshot({
    path: `${SHOTS}/03-decision-answer-condition.png`,
  });
});

test("L8.1: what the keyring signs is the native builder's own bytes", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, { foldResponse: mission.foldResponse });
  await openMissionLens(page, mission);

  const founderRow = page
    .getByTestId("mission-decision-row")
    .filter({ hasText: "Land the commit now" });
  await founderRow.getByTestId("decision-answer-option").nth(0).click();

  // The bridge records every object handed to `sign_event`. TypeScript never
  // serialises a 44244 body: what is signed is the string the native builder
  // returned, and the assertion is on that exact string.
  const signed = await page.waitForFunction(() => {
    const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    return events.find((event) => event.kind === 44244) ?? null;
  });
  const value = (await signed.jsonValue()) as {
    kind: number;
    tags: string[][];
    content: string;
  };
  expect(value.kind).toBe(44244);
  expect(value.tags).toContainEqual(["cstx-type", "decision.answer"]);
  const body = JSON.parse(value.content).body as Record<string, unknown>;
  expect(body.requestRef).toBe(mission.ids.founderRequest);
  expect(body.choice).toBe(0);
  // The key the wire does not carry on this build is absent, not null.
  expect(Object.keys(body)).toEqual(["requestRef", "choice", "note"]);

  // Nothing is asserted about the row *after* this publish, and that is a
  // property of the fixture rather than of the product: the answer this test
  // really publishes is a 44244 the pinned fold response does not list, so the
  // next projection refuses its own inputs and the queue unmounts. F8's
  // rendered receipt and the form going quiet are asserted against the real
  // components in `CodingSessionMissionDecisionQueue.test.mjs`; the refusal
  // path — which adds nothing to the wire and so is stable — is the test
  // below.
});

test("L8.1: a relay that refuses the answer leaves the row open and prints its own words", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    rejectPublishedKinds: [
      { kind: 44244, message: "blocked: not a member of this channel" },
    ],
  });
  await openMissionLens(page, mission);

  const founderRow = page
    .getByTestId("mission-decision-row")
    .filter({ hasText: "Land the commit now" });
  await founderRow.getByTestId("decision-answer-option").nth(1).click();

  await expect(founderRow.getByTestId("decision-answer-error")).toContainText(
    "The relay did not accept this answer:",
  );
  await expect(founderRow.getByTestId("decision-answer-error")).toContainText(
    "blocked: not a member of this channel",
  );
  await expect(founderRow).toHaveAttribute("data-decision-state", "open");
});

test("L8.2: a mission the rule refuses keeps the control and prints §1j's string", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: landRefusedResponse(mission),
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await expect(control).toHaveAttribute("data-land-state", "refused");
  await expect(control.getByTestId("mission-land-sentence")).toHaveText(
    `Not ready to land: ${LAND_REFUSAL_NO_VERDICT}`,
  );
  await expect(page.getByTestId("mission-land-open")).toHaveCount(0);

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/04-land-not-ready.png` });
});

test("L8.2: an admitted commit offers the exact command, and the app never runs it", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: landReadyResponse(mission),
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await expect(control).toHaveAttribute("data-land-state", "ready");
  await expect(control.getByTestId("mission-land-open")).toHaveText(
    `Land ${HEAD_SHA.slice(0, 7)} on main`,
  );
  await control.getByTestId("mission-land-open").click();

  await expect(control.getByTestId("mission-land-approval")).toContainText(
    "The relay's require-verdict rule admits this commit on refs/heads/main.",
  );
  await expect(control.getByTestId("mission-land-not-run")).toHaveText(
    "Beekeeper does not run this for you: the push is irreversible, this app holds no checkout, and your git credential lives in your shell. Run it there:",
  );
  // The commit, never the branch.
  await expect(control.getByTestId("mission-land-command")).toHaveText(
    `git push origin ${HEAD_SHA}:refs/heads/main`,
  );
  await expect(control.getByTestId("mission-land-copy")).toBeVisible();
  // F4 (2026-09-05 refuter): what the admission stood on. Both facts were on
  // the wire and on no screen — a founder about to run this command could not
  // see which policy record the arm read, nor whether the repository binding
  // was checked or assumed.
  await expect(control.getByTestId("land-policy-line")).toContainText(
    "Policy: present (",
  );
  await expect(control.getByTestId("land-binding-line")).toHaveText(
    "Bound repositories: read. The rule checked this mission's own binding.",
  );

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/05-land-ready.png` });
});

test("L21 arm (A): a founder's push is ready, and the screen claims no ruling", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: landFounderPushResponse(mission),
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await expect(control).toHaveAttribute("data-land-state", "ready");
  await control.getByTestId("mission-land-open").click();

  const approval = control.getByTestId("mission-land-approval");
  await expect(approval).toContainText("You are a founder of this repository");
  await expect(approval).toContainText(
    "admits your push with no verdict at all",
  );
  // The mission's newest ruling is `changes-requested`. The screen must not
  // borrow the word "approved" from an arm it is not standing on.
  await expect(approval).toContainText("Nothing here has ruled on this commit");
  await expect(approval).not.toContainText("Approved by");
  // Arm (A) reads no policy at all, and the line names the exception rather
  // than reading as "no policy was set".
  await expect(control.getByTestId("land-policy-line")).toHaveText(
    "Policy: not evaluated — founder_exception. A founder's landing is the deliberate exception; nothing here ruled on this commit.",
  );

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/09-land-founder-arm.png` });
});

test("L21 arm (C): a seat's ready push names the verifier that cleared it", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: landReadyResponse(mission),
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await control.getByTestId("mission-land-open").click();
  const approval = control.getByTestId("mission-land-approval");
  // Both records, because one without the other is not what admitted it.
  await expect(approval).toContainText("Approved by");
  await expect(approval).toContainText("did not refute it (refutation");
});

test("L18: the Land control names both founders, and says the viewer is one", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: landReadyResponse(mission),
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  const founders = control.getByTestId("mission-land-founders");
  await expect(founders).toHaveAttribute("data-founder", "viewer");
  // Both founders — the viewer by the name this surface knows them by, the
  // co-founder by its 8-hex — and the v1 residual: rules answer to the signer
  // alone. Full 64-hex twice was unreadable and clipped; the sentence the
  // screen composes is short, and `buzz-core` still owns the rule.
  await expect(founders).toContainText("Founders: the founder,");
  await expect(founders).toContainText(CO_FOUNDER.slice(0, 8));
  await expect(founders).toContainText("You are one of them.");
  await expect(founders).toContainText(
    "Any founder can set or remove a rule; the announcement's own rules stay with the founder.",
  );

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/08-land-founders.png` });
});

test("L18: a viewer who founds nothing is told so on the refused control", async ({
  page,
}) => {
  const mission = founderActMission();
  await openFounderActApp(page, {
    foldResponse: mission.foldResponse,
    landResponse: {
      ...landRefusedResponse(mission),
      ...landFounders(false),
    },
  });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await expect(control).toHaveAttribute("data-land-state", "refused");
  const founders = control.getByTestId("mission-land-founders");
  await expect(founders).toHaveAttribute("data-founder", "other");
  await expect(founders).toContainText(
    "You are not one of them, so this repository's rules do not answer to your key.",
  );

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/09-land-not-a-founder.png` });
});

test("L20: a create that names a repository resolves it, and the founders line names the resolved owner", async ({
  page,
}) => {
  // Finding 38: every app-created session used to sign `repoRef: null`
  // unconditionally, so Land could never resolve a repository no matter how
  // clearly the checkout named one. This mission's creates name REPO_REF —
  // exercising the real write path (`buildCodingSessionCreateEvent`'s own
  // `repoRef` field) and the real read path
  // (`useCodingSessionMissionLand`'s `readCodingSessionRepository`, which
  // fetches the announcement below through the mock relay, not a stub).
  const mission = founderActMission({ repoRef: REPO_REF });
  await page.addInitScript((event) => {
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [event];
  }, repoAnnouncementEvent());
  // No `landResponse` override: the mock's own default derives its answer
  // from the request's `repoOwnerPubkey`, which is only set when the relay
  // read above actually resolved — proving the whole chain, not just the
  // rendering.
  await openFounderActApp(page, { foldResponse: mission.foldResponse });
  await openMissionLens(page, mission);

  const control = page.getByTestId("mission-land-control");
  await expect(control).toHaveAttribute("data-land-state", "ungoverned");
  await expect(control.getByTestId("mission-land-sentence")).toContainText(
    "This repository has no require-verdict rule",
  );

  const founders = control.getByTestId("mission-land-founders");
  await expect(founders).toHaveAttribute("data-founder", "viewer");
  // `FOUNDER` is both this announcement's signer and the umbrella's founder,
  // so this surface's own resolver renders it as "the founder" — the same
  // shorthand the L18 tests above assert, and proof the owner pubkey the
  // relay read resolved is the one actually reaching the screen.
  await expect(founders).toContainText("Founders: the founder.");
  await expect(founders).toContainText("You are one of them.");

  await control.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await control.screenshot({ path: `${SHOTS}/10-land-repo-resolved.png` });
});

test("L8.3: an unverified completion and an unread policy each say so in the state panel", async ({
  page,
}) => {
  const mission = founderActMission({ withRefusedCompletion: true });
  await openFounderActApp(page, { foldResponse: mission.foldResponse });
  await openMissionLens(page, mission);

  const panel = page.getByTestId("mission-state-summary");
  await expect(panel.getByTestId("mission-completion-not-verified")).toHaveText(
    "Completed, but not verified: this session's policy requires a verifier's ruling and no active verifier has ruled on the approved report. The completion is not this mission's terminal until one does.",
  );
  // No 44245 reached this view, so the fold read no requirement — and says so.
  await expect(panel.getByTestId("mission-policy-record-unknown")).toHaveText(
    "No policy record reached this view, so the fold read no verifier requirement.",
  );
  // A mission whose completion the fold excluded is not a completed mission.
  await expect(panel).not.toContainText("Mission completed");

  await panel.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await panel.screenshot({ path: `${SHOTS}/06-completion-not-verified.png` });
});
