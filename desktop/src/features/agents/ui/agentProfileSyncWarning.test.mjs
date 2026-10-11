import assert from "node:assert/strict";
import { test } from "node:test";
import { toast } from "sonner";
import { showAgentProfileSyncWarning } from "./agentProfileSyncWarning.ts";
import { renameUnpublishedText } from "./agentDirectoryCopy.ts";

test("accepted configuration with pending runtime application is not reported as relay failure", () => {
  const pending =
    "Configuration accepted; runtime application awaits the next supported execution boundary.";
  const messages = [];
  const original = toast.warning;
  toast.warning = (message) => messages.push(message);
  try {
    showAgentProfileSyncWarning("Mira", pending);
    showAgentProfileSyncWarning("Mira", "relay timeout");
    showAgentProfileSyncWarning("Mira", null);
  } finally {
    toast.warning = original;
  }
  assert.equal(messages[0], `Mira: ${pending}`);
  assert.doesNotMatch(messages[0], /relay sync failed|saved locally/);
  assert.match(messages[1], /relay sync failed: relay timeout/);
  assert.equal(messages.length, 2);
  assert.equal(renameUnpublishedText(pending), pending);
  assert.match(
    renameUnpublishedText("relay timeout"),
    /relay may still show the old name/,
  );
});
