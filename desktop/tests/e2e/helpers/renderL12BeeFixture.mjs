// Renders the two L12 components to static markup in a plain Node process,
// using the project's own TypeScript/JSX loader (`test-loader.mjs`) — the
// same one `pnpm test` uses.
//
// Why this file exists at all, and is not just a call from the spec: a
// Playwright spec's own `.tsx` imports are compiled by Playwright's *own*
// TypeScript transform, whose automatic JSX runtime resolves to
// `playwright/jsx-runtime` rather than `react/jsx-runtime` — every element a
// `.tsx` component's own JSX produces comes back tagged `{__pw_type: "jsx",
// ...}` instead of a real React element, and `react-dom/server` refuses it
// with "Objects are not valid as a React child". `CodingSessionParticipantBar`
// and `PulseStaleBeeCard` are correct — proved by the many `.test.mjs` files
// next to them that render the same way through the same real loader — this
// is a Playwright-transform artifact, not a product bug. Running the render
// in a separate `node --experimental-strip-types --import ./test-loader.mjs`
// process sidesteps Playwright's transform entirely, so the markup really is
// what the app's own toolchain produces.
import { CodingSessionParticipantBar } from "../../../src/features/coding-sessions/ui/CodingSessionParticipantBar.tsx";
import { PulseStaleBeeCard } from "../../../src/features/project-pulse/ui/PulseStaleBeeCard.tsx";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

function seatItems() {
  return [
    {
      executionKey: "builder",
      label: "Bob · Builder",
      secondaryLabel: "Claude Code · sonnet",
      role: "builder",
      status: { kind: "working", label: "Working" },
      disposition: "live",
      activity: null,
      lastTurnLabel: "last turn just now",
    },
  ];
}

const [, , caseName, payloadJson] = process.argv;
const payload = payloadJson ? JSON.parse(payloadJson) : {};

let element;
switch (caseName) {
  case "participant-bar": {
    element = React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: seatItems(),
      onFocus() {},
      seatBeeStamps: new Map(Object.entries(payload.stamps ?? {})),
      // Only set when the caller passed packs at all (LANE-L23) — an
      // omitted key here must stay byte-identical to every pre-L23 harness
      // case, which never plumbed pack data.
      ...(payload.packs
        ? { seatPackRefs: new Map(Object.entries(payload.packs)) }
        : {}),
    });
    break;
  }
  case "pulse-card": {
    element = React.createElement(PulseStaleBeeCard, {
      reading: payload.reading,
    });
    break;
  }
  default:
    throw new Error(`renderL12BeeFixture: unknown case "${caseName}"`);
}

process.stdout.write(renderToStaticMarkup(element));
