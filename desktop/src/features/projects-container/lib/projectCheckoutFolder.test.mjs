import assert from "node:assert/strict";
import test from "node:test";

import {
  checkoutParentFromPath,
  projectCheckoutPath,
} from "./projectCheckoutFolder.ts";
import { slugFromName } from "../useCreateProjectContainer.ts";

/**
 * The create dialog's "Repository folder" row is `<parent>/<slug>` with the
 * slug derived from the typed name exactly as the create derives the
 * project's `d` tag — so the folder the row shows is the folder the host
 * clones to (`project_agents_init` names the clone after the repository,
 * which is the slug).
 */
test("the folder row derives <default repository folder>/<slug> from the typed name", () => {
  assert.equal(
    projectCheckoutPath("/Users/andy/Code", slugFromName("RPG Test")),
    "/Users/andy/Code/rpg-test",
  );
  assert.equal(
    projectCheckoutPath("/Users/andy/Code/", slugFromName("  Action RPG!  ")),
    "/Users/andy/Code/action-rpg",
  );
  // Nothing typed yet, or no folder known yet: an empty row, never a bare
  // `/rpg-test` or a parent with no leaf that would read as a destination.
  assert.equal(projectCheckoutPath("/Users/andy/Code", slugFromName("")), "");
  assert.equal(projectCheckoutPath(null, "rpg-test"), "");
  assert.equal(projectCheckoutPath("", "rpg-test"), "");
});

test("the parent handed to the host is read back from whatever the row holds", () => {
  // The pre-filled destination: strip the slug, keep the parent.
  assert.equal(
    checkoutParentFromPath("/Users/andy/Code/rpg-test", "rpg-test"),
    "/Users/andy/Code",
  );
  assert.equal(
    checkoutParentFromPath("/Users/andy/Code/rpg-test/", "rpg-test"),
    "/Users/andy/Code",
  );
  // A parent typed on its own is the parent; the clone still lands under it.
  assert.equal(
    checkoutParentFromPath("/Users/andy/Projects", "rpg-test"),
    "/Users/andy/Projects",
  );
  // A folder that merely ends in the slug's letters is not the destination.
  assert.equal(
    checkoutParentFromPath("/Users/andy/my-rpg-test", "rpg-test"),
    "/Users/andy/my-rpg-test",
  );
  // A destination directly under the root keeps the root as its parent.
  assert.equal(checkoutParentFromPath("/rpg-test", "rpg-test"), "/");
  // Blank means the host's default root, not an empty string the host
  // would treat as a path.
  assert.equal(checkoutParentFromPath("   ", "rpg-test"), null);
  // Round trip: parent → path → parent is stable for every parent.
  for (const parent of ["/a", "/a/b", "C:\\Code", "/Users/x/Code"]) {
    const path = projectCheckoutPath(parent, "rpg-test");
    assert.equal(checkoutParentFromPath(path, "rpg-test"), parent, path);
  }
});
