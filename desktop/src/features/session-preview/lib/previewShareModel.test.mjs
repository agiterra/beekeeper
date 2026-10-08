import assert from "node:assert/strict";
import test from "node:test";

import {
  sessionPreviewShareStripText,
  sessionPreviewShowsRemote,
} from "./previewShareModel.ts";

test("host strip words: shared with watchers, local only, or the unavailable sentence", () => {
  assert.equal(
    sessionPreviewShareStripText({
      share: true,
      watchers: 2,
      unavailableSentence: null,
    }),
    "Live on this computer · shared with the session · 2 watching",
  );
  assert.equal(
    sessionPreviewShareStripText({
      share: false,
      watchers: 2,
      unavailableSentence: null,
    }),
    "Local only · not shared",
  );
  const sentence =
    "This computer's provider key is not known, so snapshots cannot name the machine.";
  assert.equal(
    sessionPreviewShareStripText({
      share: true,
      watchers: 0,
      unavailableSentence: sentence,
    }),
    sentence,
  );
});

test("remote routing: agent elsewhere, or someone else owns it; never without a sessionRef", () => {
  const me = "a".repeat(64);
  const base = {
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    currentUserPubkey: me,
  };
  assert.equal(
    sessionPreviewShowsRemote({ ...base, isLocalProvider: false, owner: null }),
    true,
  );
  assert.equal(
    sessionPreviewShowsRemote({ ...base, isLocalProvider: true, owner: null }),
    false,
  );
  assert.equal(
    sessionPreviewShowsRemote({
      ...base,
      isLocalProvider: true,
      owner: { signer: me },
    }),
    false,
  );
  assert.equal(
    sessionPreviewShowsRemote({
      ...base,
      isLocalProvider: null,
      owner: { signer: "b".repeat(64) },
    }),
    true,
  );
  assert.equal(
    sessionPreviewShowsRemote({
      ...base,
      sessionRef: null,
      isLocalProvider: false,
      owner: null,
    }),
    false,
  );
});
