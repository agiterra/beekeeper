import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  MutationObserver,
  QueryClient,
  QueryObserver,
} from "@tanstack/react-query";
import ts from "typescript";

// Execute the actual hook callbacks with React's two context hooks substituted;
// the cache and mutation lifecycle below are the real TanStack implementation.
const source = readFileSync(new URL("./hooks.ts", import.meta.url), "utf8");
const tree = ts.createSourceFile(
  "hooks.ts",
  source,
  ts.ScriptTarget.Latest,
  true,
);
const declaration = tree.statements.find(
  (node) =>
    ts.isFunctionDeclaration(node) &&
    node.name?.text === "useUpdateManagedAgentMutation",
);
assert.ok(declaration);
const hook = ts.transpileModule(
  declaration.getText(tree).replace("export ", ""),
  {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  },
).outputText;

for (const failed of [true, false]) {
  test(`a ${failed ? "failed" : "successful"} backend update settles while profile refetch is pending`, async () => {
    const client = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const pubkey = "a".repeat(64);
    const profileKey = ["user-profile", pubkey];
    client.setQueryData(profileKey, { name: "old" });
    let fetching = false;
    const profile = new QueryObserver(client, {
      queryKey: profileKey,
      staleTime: Infinity,
      queryFn: () => {
        fetching = true;
        return new Promise(() => {});
      },
    });
    const unsubscribe = profile.subscribe(() => {});
    const original = new Error("original backend refusal before intent");
    const agent = { pubkey, name: "candidate" };
    client.setQueryData(["managed-agents"], [{ pubkey, name: "old" }]);
    const build = new Function(
      "useQueryClient",
      "useMutation",
      "updateManagedAgent",
      "evictUsersBatchEntries",
      "updateCachedChannelMemberDisplayName",
      "managedAgentsQueryKey",
      "relayAgentsQueryKey",
      "personasQueryKey",
      `${hook}\nreturn useUpdateManagedAgentMutation();`,
    );
    const options = build(
      () => client,
      (options) => options,
      async () => {
        if (failed) throw original;
        return { agent, profileSyncError: null };
      },
      () => {},
      async () => {},
      ["managed-agents"],
      ["relay-agents"],
      ["personas"],
    );
    const mutation = new MutationObserver(client, options);
    let timer;
    try {
      const pending = mutation.mutate({ pubkey, ownedPersonaUpdate: {} }).then(
        (value) => ({ value }),
        (error) => ({ error }),
      );
      const outcome = await Promise.race([
        pending,
        new Promise((resolve) => {
          timer = setTimeout(() => resolve({ timedOut: true }), 200);
        }),
      ]);
      assert.equal(
        outcome.timedOut,
        undefined,
        "unrelated refetch must not hide backend outcome",
      );
      assert.equal(fetching, true, "profile refresh is still requested");
      assert.equal(client.getQueryState(profileKey).isInvalidated, true);
      assert.equal(client.getQueryState(profileKey).fetchStatus, "fetching");
      if (failed) {
        assert.equal(outcome.error, original);
        assert.equal(mutation.getCurrentResult().status, "error");
        assert.equal(client.getQueryData(["managed-agents"])[0].name, "old");
      } else {
        assert.equal(outcome.value.agent, agent);
        assert.equal(mutation.getCurrentResult().status, "success");
        assert.deepEqual(client.getQueryData(["managed-agents"])[0], agent);
      }
    } finally {
      clearTimeout(timer);
      unsubscribe();
      client.clear();
    }
  });
}
