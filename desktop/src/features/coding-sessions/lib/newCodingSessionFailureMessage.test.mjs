import assert from "node:assert/strict";
import test from "node:test";

import { newCodingSessionFailureMessage } from "./newCodingSessionModel.ts";

// Reported 2026-08-24: "I got an error saying there can only be 4 concurrent
// Claude sessions?" It is Bee Keeper's own per-computer cap on live agent
// processes (`buzz-session-provider`'s `SESSION_LIMIT`), and nothing to do with
// the model provider — but the copy left the reader to work that out.
test("the session cap says whose cap it is", () => {
  const message = newCodingSessionFailureMessage({
    code: "SESSION_LIMIT",
    message:
      "this provider already holds its maximum of 4 running agent process(es); stop an execution you are finished with to free a slot, or set BUZZ_CSP_MAX_SESSIONS to raise the cap",
  });
  assert.match(message, /stop an execution you are finished with/);
  assert.match(message, /Bee Keeper's own cap on this computer/);
  assert.match(message, /not a limit from the model provider/);
});

test("an unknown code still passes the provider's own words through", () => {
  const message = newCodingSessionFailureMessage({
    code: "SOMETHING_NEW",
    message: "the provider said something we have never seen",
  });
  assert.equal(message, "the provider said something we have never seen");
});
