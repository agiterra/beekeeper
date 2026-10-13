import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  MutationObserver,
  QueryClient,
  QueryObserver,
} from "@tanstack/react-query";
import ts from "typescript";

// Execute the actual settlement callbacks using real TanStack cache/mutation
// lifecycles; backend outcomes are controlled independently of directory reads.
const source = readFileSync(new URL("./hooks.ts", import.meta.url), "utf8");
const tree = ts.createSourceFile(
  "hooks.ts",
  source,
  ts.ScriptTarget.Latest,
  true,
);
function settlementOptions(name, queryClient) {
  const declaration = tree.statements.find(
    (node) => ts.isFunctionDeclaration(node) && node.name?.text === name,
  );
  assert.ok(declaration);
  let property;
  function visit(node) {
    if (
      ts.isPropertyAssignment(node) &&
      node.name.getText(tree) === "onSettled"
    ) {
      property = node;
    }
    ts.forEachChild(node, visit);
  }
  visit(declaration);
  assert.ok(property);
  const callback = ts.transpileModule(
    `const options = {${property.getText(tree)}};`,
    {
      compilerOptions: { target: ts.ScriptTarget.ES2022 },
    },
  ).outputText;
  return new Function(
    "queryClient",
    "managedAgentsQueryKey",
    "relayAgentsQueryKey",
    `${callback}\nreturn options;`,
  )(queryClient, ["managed-agents"], ["relay-agents"]);
}

for (const name of [
  "useCreateManagedAgentMutation",
  "useProvisionChannelManagedAgentMutation",
]) {
  for (const failed of [true, false]) {
    test(`${name}: a ${failed ? "failed" : "successful"} backend result settles while directory refetch is pending`, async () => {
      const client = new QueryClient({
        defaultOptions: {
          queries: { retry: false },
          mutations: { retry: false },
        },
      });
      const pubkey = "a".repeat(64);
      const profileKey = ["relay-agents"];
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
      client.setQueryData(["managed-agents"], [{ pubkey, name: "old" }]);
      const options = {
        ...settlementOptions(name, client),
        mutationFn: async () => {
          if (failed) throw original;
          return { pubkey, accepted: true };
        },
      };
      const mutation = new MutationObserver(client, options);
      let timer;
      try {
        const pending = mutation.mutate({ pubkey }).then(
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
          assert.deepEqual(outcome.value, { pubkey, accepted: true });
          assert.equal(mutation.getCurrentResult().status, "success");
          assert.equal(
            client.getQueryState(["managed-agents"]).isInvalidated,
            true,
          );
        }
      } finally {
        clearTimeout(timer);
        unsubscribe();
        client.clear();
      }
    });
  }
}
