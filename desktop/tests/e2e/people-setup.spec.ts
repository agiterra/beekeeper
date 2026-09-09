import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import {
  assertReadOnly,
  assertScreenshotsDistinct,
  BUILDER_AGENT_PUBKEYS,
  CAPABILITY_SENTENCES,
  capture,
  commandCount,
  commandsSince,
  DIRECT_ENTRY_PUBKEY,
  EXHAUSTED_MESSAGE,
  expectAgentEvidenceDisclosure,
  expectNoPersonhoodClaim,
  expectUnidentified,
  FIXTURE_QUERY,
  FOUNDER_PUBKEY,
  loadMoreResults,
  NAMELESS_PUBKEY,
  NOMAD_AGENT_PUBKEY,
  NOMAD_NAME,
  openPeopleDialog,
  openPeopleSetupSession,
  openRecipientPicker,
  PEOPLE_KIND_NOTE,
  PRIYA_PUBKEY,
  PROVIDER_PUBKEY,
  recipientEmpty,
  recipientKindFilter,
  recipientKindGroup,
  recipientKindNote,
  recipientOption,
  recipientOptionMeta,
  recipientPopover,
  recipientSearch,
  rosterDetail,
  rosterKind,
  rosterRow,
  SAM_NAME,
  SAM_ONE_PUBKEY,
  SAM_TWO_PUBKEY,
  searchRecipients,
  SEATED_AGENT_PUBKEY,
  shortKey,
  signedAuthorityTransitions,
  UNRESOLVED_GRANTEE_PUBKEY,
  ZOE_NAME,
  ZOE_PUBKEY,
} from "./helpers/peopleSetupAssertions";

/**
 * Lane V — what the People and Agents surfaces are allowed to say, proved in a
 * browser against the real components.
 *
 * The population is the one Brian actually hit: 52 look-alike builder agents
 * ahead of the people, a provider authority key holding an operator grant, a
 * seated agent, and keys nothing resolves. Everything is driven through the
 * mock Tauri bridge and the mock relay — the roster is a genuine signed
 * NIP-CSAT chain, and the invite path signs a real kind:44228.
 *
 * The rule under test throughout: **nothing in this app positively establishes
 * that a key is a person.** Every scenario asserts that no surface claims
 * otherwise, and that a key with no resolved profile is offered as
 * *unidentified*.
 *
 * Named limit: this is mock-bridge coverage in Chromium. It is not native
 * Windows proof — no Tauri webview, no Windows text scaling, no real relay.
 */

// Each scenario founds a session, folds a five-link authority chain and seeds
// a 60-profile directory before it asserts anything. The default 30s budget is
// spent on setup alone in the longer ones.
test.beforeEach(() => {
  test.slow();
});

async function searchUsersCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      (window.__BUZZ_E2E_COMMANDS__ ?? []).filter(
        (command) => command === "search_users",
      ).length,
  );
}

/** Open the session, its People dialog, and the recipient picker. */
async function openPicker(
  page: Page,
  options: {
    viewport?: { width: number; height: number };
    textScale?: number;
  } = {},
): Promise<void> {
  await openPeopleSetupSession(page, options);
  await openPeopleDialog(page);
  await openRecipientPicker(page);
}

test("S0: the session invite picker offers People/Agents/All and opens on People", async ({
  page,
}) => {
  await openPicker(page);

  // §1: "The segmented control renders only when the caller passes `kind`" and
  // "Session invites pass `kind` and open on People." Both are asserted here
  // rather than inside a scenario so a missing prop names itself.
  await expect(recipientKindGroup(page)).toBeVisible();
  await expect(recipientKindFilter(page, "people")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(recipientKindFilter(page, "agents")).toBeVisible();
  await expect(recipientKindFilter(page, "all")).toBeVisible();

  // The sentence that keeps the tab honest, in the surface and not only in a
  // design document.
  await expect(recipientKindNote(page)).toHaveText(PEOPLE_KIND_NOTE);
  expectAgentEvidenceDisclosure(await recipientKindNote(page).innerText());
  await expectNoPersonhoodClaim(recipientPopover(page));
});

test("S1: an agent-heavy first page never claims to be empty, never auto-pages, and one press finds the person", async ({
  page,
}) => {
  await openPicker(page);
  await recipientKindFilter(page, "people").click();
  await searchRecipients(page, FIXTURE_QUERY);

  // Page one is 50 look-alike agents. The People view has nothing to show —
  // and must not say the results are exhausted, because they are not.
  const empty = recipientEmpty(page);
  await expect(empty).toBeVisible();
  const emptyText = await empty.innerText();
  expect(
    emptyText,
    "the People view claimed exhaustion while a page sat unfetched",
  ).not.toContain(EXHAUSTED_MESSAGE.people);
  expect(emptyText).toContain("in the results loaded so far");
  await expect(loadMoreResults(page)).toBeVisible();
  await expectNoPersonhoodClaim(recipientPopover(page));

  // No loop: nothing fetches another page while the view just sits there.
  const settled = await searchUsersCount(page);
  await page.waitForTimeout(2_000);
  expect(
    await searchUsersCount(page),
    "the picker fetched another page with no press",
  ).toBe(settled);

  // Switching the view is presentation, so it must not fetch either.
  await recipientKindFilter(page, "agents").click();
  await recipientKindFilter(page, "people").click();
  expect(
    await searchUsersCount(page),
    "switching the People/Agents filter fetched a page",
  ).toBe(settled);

  // Exactly one page per press.
  await loadMoreResults(page).click();
  await expect(recipientOption(page, ZOE_PUBKEY)).toBeVisible({
    timeout: 15_000,
  });
  expect(await searchUsersCount(page)).toBe(settled + 1);
  await expect(recipientOption(page, SAM_ONE_PUBKEY)).toBeVisible();
  await expect(recipientOption(page, SAM_TWO_PUBKEY)).toBeVisible();
  // The un-attested registry agent stays out of the People view.
  await expect(recipientOption(page, NOMAD_AGENT_PUBKEY)).toHaveCount(0);
  await expect(recipientOption(page, BUILDER_AGENT_PUBKEYS[0])).toHaveCount(0);

  await waitForAnimations(page);
  await capture(recipientPopover(page), "01-people-view-agents-present");
});

test("S2: identical display names on different keys stay distinguishable", async ({
  page,
}) => {
  await openPicker(page);
  await recipientKindFilter(page, "people").click();
  await searchRecipients(page, SAM_NAME);

  const first = recipientOption(page, SAM_ONE_PUBKEY);
  const second = recipientOption(page, SAM_TWO_PUBKEY);
  await expect(first).toBeVisible();
  await expect(second).toBeVisible();
  await expect(first).toContainText(SAM_NAME);
  await expect(second).toContainText(SAM_NAME);

  // Same name, different keys — the row must carry the key that tells them
  // apart, and the two short keys must actually differ.
  const firstShort = shortKey(SAM_ONE_PUBKEY);
  const secondShort = shortKey(SAM_TWO_PUBKEY);
  expect(firstShort).not.toBe(secondShort);
  await expect(recipientOptionMeta(page, SAM_ONE_PUBKEY)).toContainText(
    firstShort,
  );
  await expect(recipientOptionMeta(page, SAM_TWO_PUBKEY)).toContainText(
    secondShort,
  );
  // The accessible name carries the key too, so the distinction survives for
  // anyone selecting by keyboard or screen reader.
  await expect(first).toHaveAttribute("aria-label", new RegExp(firstShort));
  await expect(second).toHaveAttribute("aria-label", new RegExp(secondShort));

  await expectNoPersonhoodClaim(recipientPopover(page));
});

test("S3+S4: a key with no profile is offered as unidentified, by search and by direct entry", async ({
  page,
}) => {
  await openPicker(page);
  await recipientKindFilter(page, "people").click();

  // Searchable, but its profile resolves to no name at all.
  await searchRecipients(page, NAMELESS_PUBKEY.slice(0, 8));
  await expect(recipientOption(page, NAMELESS_PUBKEY)).toBeVisible();
  const namelessMeta = await recipientOptionMeta(
    page,
    NAMELESS_PUBKEY,
  ).innerText();
  expectUnidentified(namelessMeta, "a search result with no resolved profile");
  expect(namelessMeta).toContain("No profile on this relay");

  // Direct hex entry: a key the directory has never heard of.
  await searchRecipients(page, DIRECT_ENTRY_PUBKEY);
  const direct = recipientOption(page, DIRECT_ENTRY_PUBKEY);
  await expect(direct).toBeVisible();
  await expect(direct).toContainText("by public key");
  expectUnidentified(
    await recipientOptionMeta(page, DIRECT_ENTRY_PUBKEY).innerText(),
    "the direct-entry row",
  );
  await expectNoPersonhoodClaim(recipientPopover(page));

  // And it is still selectable — unidentified is a label, not a refusal.
  await direct.click();
  await expect(
    page.getByTestId(
      `coding-session-people-invite-recipient-chip-${DIRECT_ENTRY_PUBKEY}`,
    ),
  ).toBeVisible();
});

test("S5: yourself and everyone already granted are excluded from the picker", async ({
  page,
}) => {
  await openPicker(page);
  await recipientKindFilter(page, "all").click();

  for (const [label, query, pubkey] of [
    ["yourself", "Tyler", FOUNDER_PUBKEY],
    ["an already-granted collaborator", "Priya", PRIYA_PUBKEY],
    ["an already-seated agent", "Nova", SEATED_AGENT_PUBKEY],
  ] as const) {
    await searchRecipients(page, query);
    await expect(
      recipientOption(page, pubkey),
      `${label} was offered as an invitee`,
    ).toHaveCount(0);
  }
});

test("S6: an agent whose owner is unknown is filtered out of People but still reachable in Agents", async ({
  page,
}) => {
  await openPicker(page);
  await searchRecipients(page, NOMAD_NAME);

  // Its kind:0 carries no attestation and names no owner; only the relay's
  // agent registry knows what it is. Under People it is correctly absent...
  await recipientKindFilter(page, "people").click();
  await expect(recipientOption(page, NOMAD_AGENT_PUBKEY)).toHaveCount(0);
  await expect(recipientEmpty(page)).toContainText(EXHAUSTED_MESSAGE.people);

  // ...and under Agents it is present, with its ownership stated as unknown
  // rather than guessed from its display name.
  await recipientKindFilter(page, "agents").click();
  const nomad = recipientOption(page, NOMAD_AGENT_PUBKEY);
  await expect(nomad).toBeVisible();
  await expect(recipientOptionMeta(page, NOMAD_AGENT_PUBKEY)).toContainText(
    "Owner unknown",
  );

  // Filtering is presentation, not access: the All view reaches it too.
  await recipientKindFilter(page, "all").click();
  await expect(recipientOption(page, NOMAD_AGENT_PUBKEY)).toBeVisible();
  await expectNoPersonhoodClaim(recipientPopover(page));

  await waitForAnimations(page);
  await capture(recipientPopover(page), "02-agents-view");
});

test("S7: the session roster names a provider, a seat, and an unidentified key — and every row says what it may do", async ({
  page,
}) => {
  await openPeopleSetupSession(page);
  const dialog = await openPeopleDialog(page);

  // The founder: pinned Owner, no kind badge to add.
  await expect(rosterRow(page, FOUNDER_PUBKEY)).toBeVisible();

  // The defect Brian saw: a provider authority key rendered as a truncated
  // hex string labelled "Collaborator". It must say what it is.
  await expect(rosterKind(page, PROVIDER_PUBKEY)).toHaveText("Provider");
  const providerDetail = await rosterDetail(page, PROVIDER_PUBKEY).innerText();
  expect(providerDetail).toMatch(/provider runtime/i);
  expect(providerDetail).toContain("a computer, not a person");
  console.log(`[people-setup] provider row detail: ${providerDetail}`);

  // A receipt-backed seat names its role.
  await expect(rosterKind(page, SEATED_AGENT_PUBKEY)).toHaveText(
    "Seat · builder",
  );
  await expect(rosterDetail(page, SEATED_AGENT_PUBKEY)).toContainText(
    "Holds the builder seat in this session",
  );

  // A granted key nothing resolves is unidentified, never a person.
  await expect(rosterKind(page, UNRESOLVED_GRANTEE_PUBKEY)).toHaveText(
    "Unidentified",
  );
  // Read the whole row: the word is carried by the kind badge, and the detail
  // line beside it says why.
  expectUnidentified(
    await rosterRow(page, UNRESOLVED_GRANTEE_PUBKEY).innerText(),
    "an unresolved grantee",
  );
  await expect(rosterDetail(page, UNRESOLVED_GRANTEE_PUBKEY)).toContainText(
    "No profile and no agent evidence held for this key",
  );

  // A friendly display name is evidence of nothing: Priya resolves to a name
  // and still carries no personhood claim.
  await expect(rosterKind(page, PRIYA_PUBKEY)).toHaveText("Unidentified");
  await expect(rosterDetail(page, PRIYA_PUBKEY)).toContainText(
    "No agent evidence held for this key",
  );

  // Every row states its capability in ordinary words.
  for (const pubkey of [
    FOUNDER_PUBKEY,
    PROVIDER_PUBKEY,
    SEATED_AGENT_PUBKEY,
    UNRESOLVED_GRANTEE_PUBKEY,
    PRIYA_PUBKEY,
  ]) {
    const detail = await rosterDetail(page, pubkey).innerText();
    expect(
      CAPABILITY_SENTENCES.some((sentence) => detail.includes(sentence)),
      `row ${shortKey(pubkey)} never says what it may do: ${detail}`,
    ).toBe(true);
  }

  // People opens on the roster: no autofocus into the invite search, so the
  // directory popover is not covering the rows this dialog exists to show.
  // Captured here, before the geometry assertions below, so a layout failure
  // still leaves the picture that shows it.
  await expect(recipientPopover(page)).toHaveCount(0);
  await waitForAnimations(page);
  await capture(dialog, "03-session-roster");

  // A sentence that is rendered but unreadable is not disclosure. Two ways it
  // can go wrong, both measured at the dialog's own default width:
  //   1. clipped — the row truncates its own sentence to an ellipsis;
  //   2. spilled — the row wraps but the text escapes its <li> and lands on
  //      top of the row below, which is how removing the truncation regressed.
  for (const [label, pubkey] of [
    ["owner", FOUNDER_PUBKEY],
    ["provider", PROVIDER_PUBKEY],
    ["seat", SEATED_AGENT_PUBKEY],
    ["unidentified", UNRESOLVED_GRANTEE_PUBKEY],
    ["priya", PRIYA_PUBKEY],
  ] as const) {
    const geometry = await rosterRow(page, pubkey).evaluate((row) => {
      const detail = row.querySelector<HTMLElement>(
        '[data-testid^="coding-session-people-detail-"]',
      );
      const rowBox = row.getBoundingClientRect();
      const detailBox = (detail ?? row).getBoundingClientRect();
      const inner = detail?.querySelector("span") ?? detail ?? row;
      return {
        clipped: inner.scrollWidth > inner.clientWidth,
        client: inner.clientWidth,
        scroll: inner.scrollWidth,
        overflowPx: Math.round(detailBox.bottom - rowBox.bottom),
        // The mechanism, logged so the cause is in the record and not only in
        // a reviewer's head: the row is a flex child of a `max-h-72` flex
        // column, so it shrinks below its own content height unless it opts
        // out. `flexShrink: "1"` next to a rowHeight below rowScrollHeight is
        // that, exactly.
        rowHeight: Math.round(rowBox.height),
        rowScrollHeight: row.scrollHeight,
        flexShrink: getComputedStyle(row).flexShrink,
      };
    });
    console.log(
      `[people-setup] 100% ${label} detail ${geometry.client}/${geometry.scroll}px clipped=${geometry.clipped} overflow=${geometry.overflowPx}px rowHeight=${geometry.rowHeight}/${geometry.rowScrollHeight}px flexShrink=${geometry.flexShrink}`,
    );
    expect(
      geometry.clipped,
      `the ${label} row truncates its own sentence to an ellipsis`,
    ).toBe(false);
    // 1px of rounding slack; anything more is text sitting on the next row.
    expect(
      geometry.overflowPx,
      `the ${label} row's sentence escapes its own row by ${geometry.overflowPx}px and lands on the row below`,
    ).toBeLessThanOrEqual(1);
  }

  const dialogText = await expectNoPersonhoodClaim(dialog);
  expect(dialogText).toContain("nothing here changes project membership");
});

test("S8+S10a: keyboard selection carries exactly the selected key into the grant", async ({
  page,
}) => {
  await openPicker(page);
  await recipientKindFilter(page, "people").click();
  await searchRecipients(page, ZOE_NAME);
  await expect(recipientOption(page, ZOE_PUBKEY)).toBeVisible();

  // Nothing signed yet — browsing and filtering cost no authority.
  expect(await signedAuthorityTransitions(page)).toEqual([]);

  // The keyboard path: Enter selects the top-ranked row against a settled
  // ranking, with no pointer involved.
  await recipientSearch(page).press("Enter");
  await expect(
    page.getByTestId(
      `coding-session-people-invite-recipient-chip-${ZOE_PUBKEY}`,
    ),
  ).toBeVisible();
  expect(await signedAuthorityTransitions(page)).toEqual([]);

  // Measured before anything is dismissed: with a recipient chosen, does the
  // directory popover sit on top of the button that acts on the choice?
  const invite = page.getByTestId("coding-session-people-invite");
  const overlap = await page.evaluate(() => {
    const popover = document.querySelector(
      '[data-testid="coding-session-people-invite-recipient-popover"]',
    );
    const button = document.querySelector(
      '[data-testid="coding-session-people-invite"]',
    );
    if (!popover || !button) return null;
    const a = popover.getBoundingClientRect();
    const b = button.getBoundingClientRect();
    const overlapPx = Math.round(
      Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top),
    );
    const midpoint = document.elementFromPoint(
      b.left + b.width / 2,
      b.top + b.height / 2,
    );
    return {
      overlapPx: overlapPx > 0 ? overlapPx : 0,
      hitTest:
        midpoint?.closest('[data-testid="coding-session-people-invite"]') !==
        null,
    };
  });
  console.log(`[people-setup] invite overlap: ${JSON.stringify(overlap)}`);

  // Escape closes the directory. Done here so the grant bytes below are proved
  // whatever the answer to the overlap question is — that is asserted at the
  // end, so a layout defect cannot swallow the evidence this test exists for.
  await recipientSearch(page).press("Escape");
  await expect(recipientPopover(page)).toHaveCount(0);

  await invite.click();
  await expect
    .poll(async () => (await signedAuthorityTransitions(page)).length, {
      timeout: 15_000,
    })
    .toBe(1);

  // The bytes: exactly the selected key, and exactly one grant.
  const [grant] = await signedAuthorityTransitions(page);
  expect(grant.granteePubkey).toBe(ZOE_PUBKEY);
  expect(grant.type).toBe("grant-operator");

  // Choosing a recipient and then pressing Invite is this dialog's whole
  // purpose, and it must not require guessing that the directory has to be
  // dismissed first. A pointer aimed at the middle of the Invite button must
  // land on the Invite button.
  expect(overlap, "the invite popover or button was not on screen").not.toBe(
    null,
  );
  expect(
    overlap?.hitTest,
    `the directory popover covers the Invite button by ${overlap?.overlapPx}px, so a click on Invite lands on the popover instead`,
  ).toBe(true);
});

test("S9: opening, filtering and paging People mutates nothing", async ({
  page,
}) => {
  await openPeopleSetupSession(page);
  // The baseline is taken *before* the dialog opens, so opening it is inside
  // the window under test.
  const from = await commandCount(page);
  await openPeopleDialog(page);
  await openRecipientPicker(page);
  await recipientKindFilter(page, "people").click();
  await searchRecipients(page, FIXTURE_QUERY);
  await recipientKindFilter(page, "agents").click();
  await recipientKindFilter(page, "all").click();
  await recipientKindFilter(page, "people").click();
  await loadMoreResults(page).click();
  await expect(recipientOption(page, ZOE_PUBKEY)).toBeVisible({
    timeout: 15_000,
  });
  await recipientSearch(page).press("Escape");

  assertReadOnly(await commandsSince(page, from), "browse People");
  // Belt and braces: the authority chain was never touched.
  expect(await signedAuthorityTransitions(page)).toEqual([]);
});

test("S10b: a narrow Windows-shaped window keeps the roster and the picker usable", async ({
  page,
}) => {
  // 1024x576 CSS px = a 1280x720 window at the Windows default 125% display
  // scale, which is what a stock 1080p-and-below laptop ships with. Chosen
  // over 1280x720 because the scale factor is the part that actually squeezes
  // this dialog, and 125% is the setting most Windows machines are sold on.
  await openPeopleSetupSession(page, {
    viewport: { width: 1024, height: 576 },
  });
  const dialog = await openPeopleDialog(page);
  await expect(rosterRow(page, PROVIDER_PUBKEY)).toBeVisible();
  await expect(rosterKind(page, PROVIDER_PUBKEY)).toHaveText("Provider");

  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  console.log(
    `[people-setup] narrow viewport dialog box: ${JSON.stringify(box)}`,
  );
  expect(box?.x ?? -1).toBeGreaterThanOrEqual(0);
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(1024);

  await expect(recipientPopover(page)).toHaveCount(0);
  await waitForAnimations(page);
  await capture(dialog, "04-narrow-viewport");

  await openRecipientPicker(page);
  await recipientKindFilter(page, "people").click();
  await searchRecipients(page, SAM_NAME);
  await expect(recipientOption(page, SAM_ONE_PUBKEY)).toBeVisible();
  await expectNoPersonhoodClaim(recipientPopover(page));
  await recipientSearch(page).press("Escape");

  // The measured box is 645px tall in a 576px-high window, so the bottom of
  // this dialog — which is where the Invite button lives — is off-screen
  // unless something scrolls. Asserted rather than eyeballed: a roster you
  // can read but cannot act on is the same defect class as a wrong label.
  const invite = page.getByTestId("coding-session-people-invite");
  await invite.scrollIntoViewIfNeeded();
  await expect(invite).toBeInViewport();
});

test("S10c: at the largest zoom this app allows, the roster still reads", async ({
  page,
}) => {
  // 250% is asked for; the app's own Cmd+/- ceiling is 150%
  // (`useWebviewZoomShortcuts.ts` MAX_ZOOM_FACTOR), so a stored 2.5 is clamped
  // to 1.5. That clamp is asserted rather than worked around — if it ever
  // moves, this test says so. The remaining 100/150 of the magnification is
  // modelled the way an OS or webview zoom actually applies it: a 1280x960
  // window at 250% presents a 512x384 CSS viewport.
  await openPeopleSetupSession(page, {
    textScale: 2.5,
    viewport: { width: 512, height: 384 },
  });
  await expect
    .poll(() =>
      page.evaluate(() => getComputedStyle(document.documentElement).fontSize),
    )
    .toBe("24px");

  const dialog = await openPeopleDialog(page);
  await expect(rosterRow(page, PROVIDER_PUBKEY)).toBeVisible();
  await expect(rosterKind(page, PROVIDER_PUBKEY)).toHaveText("Provider");
  await expectNoPersonhoodClaim(dialog);
  const box = await dialog.boundingBox();
  console.log(`[people-setup] 250% dialog box: ${JSON.stringify(box)}`);

  // How much of each row's own sentence actually reaches the reader at this
  // size. Measured, not asserted: the truncation is Lane B's to weigh, and a
  // number in the log is what makes that a decision rather than an opinion.
  await expect(recipientPopover(page)).toHaveCount(0);
  await waitForAnimations(page);
  await capture(dialog, "05-zoom-250");

  for (const [label, pubkey, sentence] of [
    ["provider", PROVIDER_PUBKEY, "a computer, not a person"],
    ["seat", SEATED_AGENT_PUBKEY, "Holds the builder seat in this session"],
  ] as const) {
    const geometry = await rosterRow(page, pubkey).evaluate((row) => {
      const detail = row.querySelector<HTMLElement>(
        '[data-testid^="coding-session-people-detail-"]',
      );
      const inner = detail?.querySelector("span") ?? detail ?? row;
      return {
        client: inner.clientWidth,
        scroll: inner.scrollWidth,
        text: (inner.textContent ?? "").trim(),
        overflowPx: Math.round(
          (detail ?? row).getBoundingClientRect().bottom -
            row.getBoundingClientRect().bottom,
        ),
      };
    });
    console.log(
      `[people-setup] 250% ${label} detail ${geometry.client}/${geometry.scroll}px clipped=${geometry.scroll > geometry.client} overflow=${geometry.overflowPx}px "${geometry.text}"`,
    );
    // A one-line clamp reduced the seat sentence to zero pixels at this scale.
    // Asserted here so a future clamp fails in this spec rather than in
    // somebody's eyes.
    expect(
      geometry.scroll > geometry.client,
      `the ${label} row clamps its sentence at the app's maximum text scale`,
    ).toBe(false);
    expect(
      geometry.overflowPx,
      `the ${label} row's sentence escapes its own row by ${geometry.overflowPx}px at the app's maximum text scale`,
    ).toBeLessThanOrEqual(1);
    await expect(rosterDetail(page, pubkey)).toContainText(sentence);
  }
});

test("S12: People opens on the roster, and the invite is one Tab away", async ({
  page,
}) => {
  await openPeopleSetupSession(page);
  const dialog = await openPeopleDialog(page);

  // The surface used to autofocus the invite input, which opened the directory
  // popover over the rows — so People opened by covering the roster with a
  // page of agents, and fired the infinite user search on every open. The
  // roster is what this dialog is for; it must be what it shows.
  await expect(recipientPopover(page)).toHaveCount(0);
  const initial = await page.evaluate(() => {
    const active = document.activeElement as HTMLElement | null;
    return {
      testId: active?.getAttribute("data-testid") ?? null,
      tag: active?.tagName ?? null,
      label: active?.getAttribute("aria-label") ?? null,
    };
  });
  console.log(`[people-setup] initial focus: ${JSON.stringify(initial)}`);
  expect(
    initial.testId,
    "People autofocused the invite search and covered its own roster",
  ).not.toBe("coding-session-people-invite-recipient-search");
  await expect(dialog).toBeVisible();
  await expect(rosterRow(page, PROVIDER_PUBKEY)).toBeVisible();

  // And the invite is still reachable from the keyboard. The count is
  // measured, not assumed, and printed so a regression in tab order is a
  // number rather than a feeling.
  const search = recipientSearch(page);
  let presses = 0;
  for (; presses < 10; presses += 1) {
    await page.keyboard.press("Tab");
    if (await search.evaluate((el) => el === document.activeElement)) break;
  }
  console.log(`[people-setup] Tab presses to reach the invite: ${presses + 1}`);
  expect(
    presses,
    "the invite search was not reachable within 10 Tab presses",
  ).toBeLessThan(10);
  await expect(search).toBeFocused();
});

test("S11: every screenshot captured a different state", async () => {
  assertScreenshotsDistinct();
});
