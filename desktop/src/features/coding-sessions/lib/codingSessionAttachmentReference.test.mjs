import assert from "node:assert/strict";
import test from "node:test";

import { containsAttachmentReference } from "./codingSessionAttachmentReference.ts";

const sha = "a".repeat(64);

test("an image reference counts, whatever its alt text", () => {
  assert.equal(
    containsAttachmentReference(
      `see this:\n\n![image](http://relay/media/${sha}.png)`,
    ),
    true,
  );
  assert.equal(containsAttachmentReference("![](http://relay/x.png)"), true);
});

test("a pasted file's plain link counts only when it addresses a blob", () => {
  assert.equal(
    containsAttachmentReference(
      `fix the crash in [pasted-text-1.txt](http://relay/media/${sha}.txt)`,
    ),
    true,
  );
  // An ordinary link in someone's prose references nothing this turn carried,
  // so it must not suppress the only evidence an attachment existed.
  assert.equal(
    containsAttachmentReference("see [the docs](http://example.com/guide)"),
    false,
  );
  // A link that looks like a media path but is not 64 hex is not one either.
  assert.equal(
    containsAttachmentReference("[x](http://relay/media/abc.txt)"),
    false,
  );
});

test("prose with no links at all references nothing", () => {
  assert.equal(containsAttachmentReference(""), false);
  assert.equal(containsAttachmentReference("look at this"), false);
  assert.equal(containsAttachmentReference("[unclosed](http://relay"), false);
});
