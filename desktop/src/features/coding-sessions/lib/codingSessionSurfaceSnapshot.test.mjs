import assert from "node:assert/strict";
import test from "node:test";

import {
  deriveRemotePreviewView,
  formatSurfaceAge,
  formatSurfaceCadence,
  parseSessionPreviewAnnounce,
  parseSurfaceSnapshot,
  parseSurfaceSnapshotToken,
  resolveSessionPreviewOwner,
  surfaceMachineName,
  surfaceSnapshotCommitLabel,
} from "./codingSessionSurfaceSnapshot.ts";
import {
  announce,
  CHANNEL,
  eventId,
  OTHER,
  PERSON,
  PROVIDER,
  SEAT,
  SESSION_REF,
  snapshot,
} from "./surfaceFixtures.testFixtures.mjs";

const NOW = 1_791_374_600_000;
const COMMIT = "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d";

test("a 44253 reads as a card: machine, time, commit, requested-by; page shown as local:/…", () => {
  const card = parseSurfaceSnapshot(
    snapshot({
      surface: "preview",
      key: SESSION_REF,
      requestedBy: SEAT,
      commit: { sha: COMMIT, dirty: true },
      title: "Settings",
    }),
  );
  assert.ok(card);
  assert.equal(card.surface, "preview");
  assert.equal(card.page, "local:/settings");
  assert.equal(card.machine, PROVIDER);
  assert.equal(card.requestedBy, SEAT);
  assert.equal(
    surfaceSnapshotCommitLabel(card.commit),
    "commit 1a2b3c4 · dirty",
  );
  assert.equal(
    surfaceSnapshotCommitLabel({ sha: COMMIT, dirty: false }),
    "commit 1a2b3c4",
  );
});

test("absent commit tag: commit not recorded", () => {
  const card = parseSurfaceSnapshot(snapshot());
  assert.ok(card);
  assert.equal(card.commit, null);
  assert.equal(surfaceSnapshotCommitLabel(card.commit), "commit not recorded");
});

test("strict 44253: a preview needs a page; a device forbids one; a loopback port or path is refused", () => {
  const noPage = snapshot({ surface: "preview", key: SESSION_REF });
  noPage.tags = noPage.tags.filter((tag) => tag[0] !== "page");
  assert.equal(parseSurfaceSnapshot(noPage), null);
  assert.equal(parseSurfaceSnapshot(snapshot({ page: "local:/x" })), null);
  assert.equal(
    parseSurfaceSnapshot(
      snapshot({
        surface: "preview",
        key: SESSION_REF,
        page: "http://localhost:5173/",
      }),
    ),
    null,
  );
  assert.equal(
    parseSurfaceSnapshot(
      snapshot({
        surface: "preview",
        key: SESSION_REF,
        page: "local:/Users/brian/app",
      }),
    ),
    null,
  );
  const reordered = snapshot();
  [reordered.tags[5], reordered.tags[6]] = [
    reordered.tags[6],
    reordered.tags[5],
  ];
  assert.equal(parseSurfaceSnapshot(reordered), null);
});

test("verdict tokens: snapshot:<id> and the preview:<id> alias", () => {
  const id = "f".repeat(64);
  assert.equal(parseSurfaceSnapshotToken(`snapshot:${id}`), id);
  assert.equal(parseSurfaceSnapshotToken(`preview:${id}`), id);
  assert.equal(parseSurfaceSnapshotToken(`snap:${id}`), null);
  assert.equal(parseSurfaceSnapshotToken("snapshot:abc"), null);
});

test("owner fold: the earliest open wins; that signer's later close hands it to the next", () => {
  const first = announce({ signer: PERSON, createdAt: 100 });
  const second = announce({ signer: OTHER, createdAt: 200 });
  assert.equal(
    resolveSessionPreviewOwner([second, first], CHANNEL, SESSION_REF).signer,
    PERSON,
  );
  const closed = announce({ signer: PERSON, createdAt: 300, status: "closed" });
  assert.equal(
    resolveSessionPreviewOwner([first, second, closed], CHANNEL, SESSION_REF)
      .signer,
    OTHER,
  );
  const allClosed = announce({
    signer: OTHER,
    createdAt: 400,
    status: "closed",
  });
  assert.equal(
    resolveSessionPreviewOwner(
      [first, second, closed, allClosed],
      CHANNEL,
      SESSION_REF,
    ),
    null,
  );
  // Tie on created_at: the lower id.
  const a = announce({ signer: PERSON, createdAt: 500, id: eventId(2) });
  const b = announce({ signer: OTHER, createdAt: 500, id: eventId(1) });
  assert.equal(
    resolveSessionPreviewOwner([a, b], CHANNEL, SESSION_REF).signer,
    OTHER,
  );
  // Another session's announce never counts.
  assert.equal(
    resolveSessionPreviewOwner(
      [first],
      CHANNEL,
      "00000000-0000-4000-8000-000000000000",
    ),
    null,
  );
});

test("strict 30626: a reordered tag or a bad page drops the announce", () => {
  assert.ok(parseSessionPreviewAnnounce(announce()));
  const reordered = announce();
  [reordered.tags[6], reordered.tags[7]] = [
    reordered.tags[7],
    reordered.tags[6],
  ];
  assert.equal(parseSessionPreviewAnnounce(reordered), null);
  const bad = announce();
  bad.tags[6] = ["page", "http://127.0.0.1/settings"];
  assert.equal(parseSessionPreviewAnnounce(bad), null);
});

test("strict 30626: a close carries no page or title; an open needs both", () => {
  const withStatus = (status) => {
    const event = announce();
    event.tags[3] = ["status", status];
    return event;
  };
  const without = (event, names) => ({
    ...event,
    tags: event.tags.filter((tag) => !names.includes(tag[0])),
  });
  const bareClose = parseSessionPreviewAnnounce(
    without(withStatus("closed"), ["page", "title"]),
  );
  assert.ok(bareClose, "a closed announce without page/title parses");
  assert.equal(bareClose.status, "closed");
  assert.equal(bareClose.page, null);
  assert.equal(bareClose.title, null);
  for (const leak of [
    withStatus("closed"),
    without(withStatus("closed"), ["page"]),
    without(withStatus("closed"), ["title"]),
  ]) {
    assert.equal(parseSessionPreviewAnnounce(leak), null);
  }
  for (const missing of [
    without(announce(), ["page"]),
    without(announce(), ["title"]),
  ]) {
    assert.equal(parseSessionPreviewAnnounce(missing), null);
  }
});

test("remote view: none, live, paused, stalled, not-streaming and snapshot words", () => {
  const nameOf = (pubkey) => (pubkey === PERSON ? "Brian" : null);
  const owner = parseSessionPreviewAnnounce(announce());
  const none = deriveRemotePreviewView({
    owner: null,
    observer: null,
    newestSnapshot: null,
    now: NOW,
    nameOf,
  });
  assert.equal(none.state, "none");
  assert.equal(none.text, "No preview is shared for this session.");
  assert.equal(
    none.hint,
    "The Browser runs in the Beekeeper app on the machine running the agent.",
  );
  const at = (status, frameAt = null) => ({
    status,
    frameAt,
    cadenceMs: 2000,
    actor: null,
  });
  const live = deriveRemotePreviewView({
    owner,
    observer: at("live", NOW - 4_000),
    newestSnapshot: null,
    now: NOW,
    nameOf,
  });
  assert.equal(live.state, "live");
  assert.equal(live.text, "Live from Brian's computer · every 2 s · 4 s ago");
  assert.equal(
    deriveRemotePreviewView({
      owner,
      observer: at("paused"),
      newestSnapshot: null,
      now: NOW,
      nameOf,
    }).text,
    "Paused by Brian",
  );
  const stalled = deriveRemotePreviewView({
    owner,
    observer: at("stalled", NOW - 40_000),
    newestSnapshot: null,
    now: NOW,
    nameOf,
  });
  assert.equal(stalled.text, "Stalled · last frame 40 s ago");
  assert.doesNotMatch(stalled.text, /Live/);
  const card = parseSurfaceSnapshot(
    snapshot({
      surface: "preview",
      key: SESSION_REF,
      takenAt: new Date(2026, 9, 7, 14, 2).getTime(),
    }),
  );
  const notStreaming = deriveRemotePreviewView({
    owner,
    observer: at("not-streaming"),
    newestSnapshot: card,
    now: NOW,
    nameOf,
  });
  assert.equal(notStreaming.state, "not-streaming");
  assert.equal(notStreaming.text, "Host not streaming");
  const snapshotsOnly = parseSessionPreviewAnnounce(
    announce({ stream: "snapshots" }),
  );
  const snap = deriveRemotePreviewView({
    owner: snapshotsOnly,
    observer: at("not-streaming"),
    newestSnapshot: card,
    now: NOW,
    nameOf,
  });
  assert.equal(snap.state, "snapshot");
  assert.equal(snap.text, "Snapshot · 14:02 · not live");
  // A name nobody gave falls back to the short key, never a hostname.
  const unnamed = deriveRemotePreviewView({
    owner,
    observer: at("paused"),
    newestSnapshot: null,
    now: NOW,
  });
  assert.match(unnamed.text, /^Paused by dddddddd…dddd$/);
});

test("labels: ages, cadence, machine names", () => {
  assert.equal(formatSurfaceAge(4_200), "4 s");
  assert.equal(formatSurfaceAge(120_000), "2 min");
  assert.equal(formatSurfaceCadence(3_000), "every 3 s");
  assert.equal(formatSurfaceCadence(500), "every 0.5 s");
  assert.equal(
    surfaceMachineName({
      providerPubkey: PROVIDER,
      localProviderPubkey: PROVIDER,
    }),
    "this computer",
  );
  assert.equal(
    surfaceMachineName({
      providerPubkey: PROVIDER,
      nameOf: () => "Andy's Mac",
    }),
    "Andy's Mac",
  );
  assert.equal(
    surfaceMachineName({ providerPubkey: PROVIDER }),
    "aaaaaaaa…aaaa",
  );
});
