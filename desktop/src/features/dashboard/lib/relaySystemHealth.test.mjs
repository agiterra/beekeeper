import assert from "node:assert/strict";
import test from "node:test";

import {
  classifyRelaySystemHealthError,
  formatBytes,
  formatPercent,
  parseRelaySystemHealth,
  relayContainerSentence,
  relayCpuSentence,
  relayDiskSentence,
  relayLoadSentence,
  relayMemorySentence,
  relaySampleAgeSentence,
  relaySystemHealthFailureSentence,
} from "./relaySystemHealth.ts";

const GIB = 1024 ** 3;

function wire(overrides = {}) {
  return {
    sampled_at: "2026-09-14T20:00:00Z",
    age_seconds: 3,
    interval_seconds: 10,
    host: {
      name: "agincus",
      os: "Linux 6.8",
      uptime_seconds: 86400,
      relay_uptime_seconds: 3600,
    },
    cpu: {
      cores: 4,
      machine_percent: 12.3,
      process_percent: 3.2,
      load_average: { one: 0.52, five: 0.4, fifteen: 0.31 },
    },
    memory: {
      machine_total_bytes: 16 * GIB,
      machine_used_bytes: 5.5 * GIB,
      machine_available_bytes: 10.5 * GIB,
      swap_total_bytes: 0,
      swap_used_bytes: 0,
      process_rss_bytes: 120 * 1024 * 1024,
      container: null,
    },
    disks: [
      {
        labels: ["git data", "root"],
        paths: ["/data/git", "/"],
        total_bytes: 100 * GIB,
        available_bytes: 40 * GIB,
      },
    ],
    ...overrides,
  };
}

test("parseRelaySystemHealth_readsTheWireShape", () => {
  const health = parseRelaySystemHealth(wire());
  assert.ok(health);
  assert.equal(health.host.name, "agincus");
  assert.equal(health.cpu.cores, 4);
  assert.deepEqual(health.cpu.loadAverage, {
    one: 0.52,
    five: 0.4,
    fifteen: 0.31,
  });
  assert.equal(health.memory.container, null);
  assert.equal(health.disks.length, 1);
  assert.deepEqual(health.disks[0].labels, ["git data", "root"]);
});

test("parseRelaySystemHealth_rejectsAMissingOrMalformedFigureWhole", () => {
  assert.equal(parseRelaySystemHealth(null), null);
  assert.equal(parseRelaySystemHealth("ok"), null);
  const missing = wire();
  delete missing.memory.machine_total_bytes;
  assert.equal(parseRelaySystemHealth(missing), null);
  assert.equal(
    parseRelaySystemHealth(
      wire({ cpu: { cores: "4", machine_percent: 1, process_percent: 1 } }),
    ),
    null,
  );
  assert.equal(
    parseRelaySystemHealth(
      wire({
        disks: [{ labels: [], paths: [], total_bytes: 1, available_bytes: 1 }],
      }),
    ),
    null,
  );
  assert.equal(parseRelaySystemHealth(wire({ age_seconds: Number.NaN })), null);
});

test("parseRelaySystemHealth_optionalFieldsDegradeToNull", () => {
  const health = parseRelaySystemHealth(
    wire({
      host: {
        name: null,
        os: null,
        uptime_seconds: 1,
        relay_uptime_seconds: 1,
      },
      cpu: {
        cores: 1,
        machine_percent: 0,
        process_percent: 0,
        load_average: null,
      },
    }),
  );
  assert.ok(health);
  assert.equal(health.host.name, null);
  assert.equal(health.cpu.loadAverage, null);
  const limited = parseRelaySystemHealth(
    wire({
      memory: {
        ...wire().memory,
        container: { limit_bytes: 2 * GIB, used_bytes: 1.2 * GIB },
      },
    }),
  );
  assert.ok(limited);
  assert.deepEqual(limited.memory.container, {
    limitBytes: 2 * GIB,
    usedBytes: 1.2 * GIB,
  });
});

test("formatBytes_binaryUnitsOneDecimalUnderTen", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(120 * 1024 * 1024), "120 MiB");
  assert.equal(formatBytes(5.5 * GIB), "5.5 GiB");
  assert.equal(formatBytes(16 * GIB), "16 GiB");
  assert.equal(formatBytes(9.96 * GIB), "10 GiB");
  assert.equal(formatBytes(2.5 * 1024 * GIB), "2.5 TiB");
  assert.equal(formatBytes(-1), "unknown");
  assert.equal(formatPercent(12.3), "12%");
  assert.equal(formatPercent(Number.NaN), "unknown");
});

test("sentences_nameWhatTheyMeasure", () => {
  const health = parseRelaySystemHealth(wire());
  assert.ok(health);
  assert.equal(
    relayCpuSentence(health.cpu),
    "12% of 4 cores · relay process 3%",
  );
  assert.equal(
    relayCpuSentence({ ...health.cpu, cores: 1 }),
    "12% of 1 core · relay process 3%",
  );
  assert.equal(
    relayLoadSentence(health.cpu.loadAverage),
    "load 0.52 · 0.40 · 0.31",
  );
  assert.equal(relayLoadSentence(null), null);
  assert.equal(
    relayMemorySentence(health.memory),
    "5.5 GiB of 16 GiB in use (34%) · relay process 120 MiB",
  );
  assert.equal(relayContainerSentence(health.memory), null);
  assert.equal(
    relayContainerSentence({
      ...health.memory,
      container: { limitBytes: 2 * GIB, usedBytes: 1.2 * GIB },
    }),
    "container 1.2 GiB of 2.0 GiB (60%)",
  );
  assert.equal(
    relayDiskSentence(health.disks[0]),
    "git data, root · 40 GiB free of 100 GiB (60% used)",
  );
});

test("relaySampleAgeSentence_marksAMissedSamplerAsStale", () => {
  assert.equal(relaySampleAgeSentence(2, 10), "sampled just now");
  assert.equal(relaySampleAgeSentence(12, 10), "sampled 12 s ago");
  assert.equal(relaySampleAgeSentence(31, 10), "sampled 31 s ago (stale)");
  assert.equal(relaySampleAgeSentence(180, 10), "sampled 3 min ago (stale)");
  assert.equal(relaySampleAgeSentence(180, 0), "sampled 3 min ago");
});

test("classifyRelaySystemHealthError_readsTheRelayPrefixes", () => {
  assert.equal(
    classifyRelaySystemHealthError(
      new Error("relay returned 403 Forbidden: restricted"),
    ),
    "forbidden",
  );
  assert.equal(
    classifyRelaySystemHealthError(new Error("relay returned 404 Not Found")),
    "unsupported",
  );
  assert.equal(
    classifyRelaySystemHealthError(
      new Error("relay returned 503 Service Unavailable: not sampled"),
    ),
    "not-sampled",
  );
  assert.equal(
    classifyRelaySystemHealthError(
      new Error("relay unreachable: request timed out"),
    ),
    "unreachable",
  );
  assert.equal(
    classifyRelaySystemHealthError(
      new Error("relay rate-limited: retry in 10s"),
    ),
    "rate-limited",
  );
  assert.equal(classifyRelaySystemHealthError("boom"), "failed");
  for (const failure of [
    "forbidden",
    "unsupported",
    "not-sampled",
    "unreachable",
    "rate-limited",
    "failed",
  ]) {
    assert.ok(relaySystemHealthFailureSentence(failure).length > 0);
  }
});
