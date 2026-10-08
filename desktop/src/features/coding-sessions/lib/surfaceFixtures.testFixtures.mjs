/** Shared fixtures for the C5 surface lib tests (44253/44254/44255/30626/24321). */
export const CHANNEL = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
export const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
export const PROVIDER = "a".repeat(64);
export const OTHER = "b".repeat(64);
export const SEAT = "c".repeat(64);
export const PERSON = "d".repeat(64);
export const SLOT = "9f2c4e1a7b3d5c80";
export const TARGET_KEY = "coding-session/v1|6:claude6:inst-11:S1:2";
export const SHA = "e".repeat(64);

let counter = 0;
export function eventId(n) {
  return n.toString(16).padStart(64, "0");
}

export function event(kind, tags, content = "", overrides = {}) {
  counter += 1;
  return {
    id: eventId(counter),
    pubkey: PROVIDER,
    created_at: 1_791_374_400,
    kind,
    tags,
    content,
    sig: "0".repeat(128),
    ...overrides,
  };
}

export function record(type, content, { slot, cmd, e, ...overrides } = {}) {
  const tags = [
    ["h", CHANNEL],
    ["sdv-v", "sdv1"],
    ["cs-target", TARGET_KEY],
    ["csl-command", "lc-1"],
    ["sdv-type", type],
  ];
  if (slot) tags.push(["sdv-slot", slot]);
  if (cmd) tags.push(["sdv-cmd", cmd]);
  if (e) tags.push(["e", e, "", "snapshot"]);
  return event(44255, tags, JSON.stringify({ type, ...content }), overrides);
}

export function availability(
  ios = { available: true },
  agentDevice = { installed: true, version: "0.21.12" },
  overrides = {},
) {
  return record(
    "availability",
    {
      platforms: {
        ios,
        android: { available: false, reason: "Android SDK not found" },
      },
      agentDevice,
    },
    overrides,
  );
}

export function state(stateWord, overrides = {}, extra = {}) {
  return record(
    "state",
    {
      state: stateWord,
      platform: "ios",
      model: "iPhone 17",
      osVersion: "27.0",
      drivers: ["agent", "host-owner"],
      capture: { mode: "snapshot-poll", maxIntervalMs: 3000 },
      ...extra,
    },
    { slot: SLOT, ...overrides },
  );
}

export function command(op, cmd, overrides = {}, slot = null) {
  const tags = [
    ["h", CHANNEL],
    ["sdv-v", "sdv1"],
    ["cs-target", TARGET_KEY],
    ["sdv-cmd", cmd],
  ];
  if (slot) tags.push(["sdv-slot", slot]);
  const content =
    op === "open" ? { op, platform: "ios", model: "iPhone 17" } : { op };
  return event(44254, tags, JSON.stringify(content), {
    pubkey: SEAT,
    ...overrides,
  });
}

export function snapshot({
  surface = "device",
  key = SLOT,
  signer = PROVIDER,
  takenAt = 1_791_374_512_345,
  requestedBy = null,
  commit = null,
  page = surface === "preview" ? "local:/settings" : null,
  title = null,
  createdAt = 1_791_374_512,
} = {}) {
  const tags = [
    ["h", CHANNEL],
    ["ssn-v", "1"],
    ["ssn-type", "snapshot"],
    ["surface", surface],
    ["d", key],
    ["x", SHA],
    ["url", `https://hive.agiterra.org/${SHA}.png`],
    ["m", "image/png"],
    ["dim", "1179x2556"],
    ["taken-at", String(takenAt)],
    ["provider", PROVIDER],
  ];
  if (requestedBy) tags.push(["p", requestedBy, "", "requested-by"]);
  if (commit)
    tags.push(["commit", commit.sha, commit.dirty ? "dirty" : "clean"]);
  if (page) tags.push(["page", page]);
  if (title !== null) tags.push(["title", title]);
  return event(44253, tags, "", { pubkey: signer, created_at: createdAt });
}

export function announce({
  signer = PERSON,
  status = "open",
  createdAt = 1_791_374_400,
  id,
  stream = "frames",
} = {}) {
  return event(
    30626,
    [
      ["h", CHANNEL],
      ["d", SESSION_REF],
      ["spa-v", "1"],
      ["status", status],
      ["cs-target", TARGET_KEY],
      ["provider", PROVIDER],
      // A close carries no page or title (NIP-SP).
      ...(status === "open"
        ? [
            ["page", "local:/settings"],
            ["title", "Settings"],
          ]
        : []),
      ["viewport", "1280x800"],
      ["stream", stream],
      ["input", "synthetic"],
    ],
    "",
    { pubkey: signer, created_at: createdAt, ...(id ? { id } : {}) },
  );
}

export function frame({
  surface = "device",
  key = SLOT,
  author = PROVIDER,
  t = "frame",
  seq = 1,
  epoch = 1_791_374_400_123,
  capturedAt,
  cadence = 3000,
} = {}) {
  return event(
    24321,
    [
      ["h", CHANNEL],
      ["surface", surface],
      ["d", key],
      ["t", t],
      ["seq", String(seq)],
      ["epoch", String(epoch)],
      ["cadence-ms", String(cadence)],
      ["dim", "900x1950"],
      ["captured-at", String(capturedAt)],
    ],
    t === "frame" ? "/9j/4AAQSkZJRgABAQ==" : "",
    { pubkey: author },
  );
}
