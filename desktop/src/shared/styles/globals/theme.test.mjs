import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import tailwindConfig from "../../../../tailwind.config.js";

const themeCss = readFileSync(
  fileURLToPath(new URL("./theme.css", import.meta.url)),
  "utf8",
);

const SYSTEM_STACK = [
  "-apple-system",
  "BlinkMacSystemFont",
  '"Segoe UI"',
  "system-ui",
  "sans-serif",
];

test("SV-04 (D1): font-sans is the system UI stack, Inter is gone", () => {
  const sans = tailwindConfig.theme.extend.fontFamily.sans;
  assert.deepEqual(sans, SYSTEM_STACK);
  assert.ok(!sans.some((face) => /Inter|Avenir/.test(face)));
  // `font-mono` is left to Tailwind's default monospace stack.
  assert.equal(tailwindConfig.theme.extend.fontFamily.mono, undefined);
});

test("SV-04 (D1): body mirrors the same stack", () => {
  const body = [...themeCss.matchAll(/\n {2}body \{[^}]*\}/g)].find((match) =>
    match[0].includes("font-family"),
  );
  assert.ok(body, "body rule present");
  assert.match(
    body[0],
    /font-family:\s*-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;/,
  );
  assert.doesNotMatch(themeCss, /Inter Variable|"Avenir Next"/);
});
