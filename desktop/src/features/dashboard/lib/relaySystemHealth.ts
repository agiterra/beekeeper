/**
 * The relay's machine health as the Dashboard shows it: the wire shape from
 * `GET /health/system` (relay `system_health.rs`, snake_case) validated into
 * a typed record, the sentences the card prints, and the reading of the
 * errors the relay can answer with.
 *
 * Nothing here is derived from a guess: a document missing a required field
 * is rejected whole rather than shown with zeros, and every sentence names
 * what it measures (the machine, the relay process, a container limit) so a
 * host figure is never mistaken for the relay's own.
 */

export type RelayLoadAverage = { one: number; five: number; fifteen: number };

export type RelayDiskHealth = {
  labels: string[];
  paths: string[];
  totalBytes: number;
  availableBytes: number;
};

export type RelaySystemHealth = {
  sampledAt: string;
  ageSeconds: number;
  intervalSeconds: number;
  host: {
    name: string | null;
    os: string | null;
    uptimeSeconds: number;
    relayUptimeSeconds: number;
  };
  cpu: {
    cores: number;
    machinePercent: number;
    processPercent: number;
    loadAverage: RelayLoadAverage | null;
  };
  memory: {
    machineTotalBytes: number;
    machineUsedBytes: number;
    machineAvailableBytes: number;
    swapTotalBytes: number;
    swapUsedBytes: number;
    processRssBytes: number;
    container: { limitBytes: number; usedBytes: number } | null;
  };
  disks: RelayDiskHealth[];
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function finiteNonNegative(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null;
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function stringList(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const out: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") return null;
    out.push(item);
  }
  return out;
}

function parseLoadAverage(value: unknown): RelayLoadAverage | null {
  if (!isRecord(value)) return null;
  const one = finiteNonNegative(value.one);
  const five = finiteNonNegative(value.five);
  const fifteen = finiteNonNegative(value.fifteen);
  if (one === null || five === null || fifteen === null) return null;
  return { one, five, fifteen };
}

function parseDisk(value: unknown): RelayDiskHealth | null {
  if (!isRecord(value)) return null;
  const labels = stringList(value.labels);
  const paths = stringList(value.paths);
  const totalBytes = finiteNonNegative(value.total_bytes);
  const availableBytes = finiteNonNegative(value.available_bytes);
  if (
    labels === null ||
    paths === null ||
    labels.length === 0 ||
    totalBytes === null ||
    availableBytes === null
  ) {
    return null;
  }
  return { labels, paths, totalBytes, availableBytes };
}

/**
 * Validate a `GET /health/system` body. `null` when any required figure is
 * missing or malformed — the card then says it could not read the relay
 * rather than printing a partial machine.
 */
export function parseRelaySystemHealth(
  value: unknown,
): RelaySystemHealth | null {
  if (!isRecord(value)) return null;
  const { host, cpu, memory } = value;
  if (!isRecord(host) || !isRecord(cpu) || !isRecord(memory)) return null;
  const sampledAt = optionalString(value.sampled_at);
  const ageSeconds = finiteNonNegative(value.age_seconds);
  const intervalSeconds = finiteNonNegative(value.interval_seconds);
  const uptimeSeconds = finiteNonNegative(host.uptime_seconds);
  const relayUptimeSeconds = finiteNonNegative(host.relay_uptime_seconds);
  const cores = finiteNonNegative(cpu.cores);
  const machinePercent = finiteNonNegative(cpu.machine_percent);
  const processPercent = finiteNonNegative(cpu.process_percent);
  const machineTotalBytes = finiteNonNegative(memory.machine_total_bytes);
  const machineUsedBytes = finiteNonNegative(memory.machine_used_bytes);
  const machineAvailableBytes = finiteNonNegative(
    memory.machine_available_bytes,
  );
  const swapTotalBytes = finiteNonNegative(memory.swap_total_bytes);
  const swapUsedBytes = finiteNonNegative(memory.swap_used_bytes);
  const processRssBytes = finiteNonNegative(memory.process_rss_bytes);
  if (
    sampledAt === null ||
    ageSeconds === null ||
    intervalSeconds === null ||
    uptimeSeconds === null ||
    relayUptimeSeconds === null ||
    cores === null ||
    machinePercent === null ||
    processPercent === null ||
    machineTotalBytes === null ||
    machineUsedBytes === null ||
    machineAvailableBytes === null ||
    swapTotalBytes === null ||
    swapUsedBytes === null ||
    processRssBytes === null
  ) {
    return null;
  }
  let container: RelaySystemHealth["memory"]["container"] = null;
  if (memory.container !== null && memory.container !== undefined) {
    if (!isRecord(memory.container)) return null;
    const limitBytes = finiteNonNegative(memory.container.limit_bytes);
    const usedBytes = finiteNonNegative(memory.container.used_bytes);
    if (limitBytes === null || usedBytes === null) return null;
    container = { limitBytes, usedBytes };
  }
  if (!Array.isArray(value.disks)) return null;
  const disks: RelayDiskHealth[] = [];
  for (const raw of value.disks) {
    const disk = parseDisk(raw);
    if (disk === null) return null;
    disks.push(disk);
  }
  return {
    sampledAt,
    ageSeconds,
    intervalSeconds,
    host: {
      name: optionalString(host.name),
      os: optionalString(host.os),
      uptimeSeconds,
      relayUptimeSeconds,
    },
    cpu: {
      cores,
      machinePercent,
      processPercent,
      loadAverage: parseLoadAverage(cpu.load_average),
    },
    memory: {
      machineTotalBytes,
      machineUsedBytes,
      machineAvailableBytes,
      swapTotalBytes,
      swapUsedBytes,
      processRssBytes,
      container,
    },
    disks,
  };
}

const BYTE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] as const;

/**
 * Binary units, one decimal under ten of a unit, whole numbers above:
 * `5.5 GiB`, `16 GiB`, `120 MiB`, `0 B`.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = unit === 0 ? Math.round(value) : Number(value.toFixed(1));
  const text =
    unit === 0 || rounded >= 10
      ? String(Math.round(rounded))
      : rounded.toFixed(1);
  return `${text} ${BYTE_UNITS[unit]}`;
}

/** Whole percent, `12%`; a value the machine cannot have reads `unknown`. */
export function formatPercent(value: number): string {
  if (!Number.isFinite(value) || value < 0) return "unknown";
  return `${Math.round(value)}%`;
}

/** `used` as a whole percent of `total`, or `null` when `total` is zero. */
export function usedPercent(used: number, total: number): number | null {
  if (total <= 0) return null;
  return Math.round((used / total) * 100);
}

/** "12% of 4 cores · relay process 3%". */
export function relayCpuSentence(cpu: RelaySystemHealth["cpu"]): string {
  const cores = cpu.cores === 1 ? "1 core" : `${cpu.cores} cores`;
  return `${formatPercent(cpu.machinePercent)} of ${cores} · relay process ${formatPercent(cpu.processPercent)}`;
}

/** "load 0.52 · 0.40 · 0.31", or `null` where the platform has none. */
export function relayLoadSentence(
  load: RelayLoadAverage | null,
): string | null {
  if (load === null) return null;
  return `load ${load.one.toFixed(2)} · ${load.five.toFixed(2)} · ${load.fifteen.toFixed(2)}`;
}

/** "5.5 GiB of 16 GiB in use (34%) · relay process 120 MiB". */
export function relayMemorySentence(
  memory: RelaySystemHealth["memory"],
): string {
  const percent = usedPercent(
    memory.machineUsedBytes,
    memory.machineTotalBytes,
  );
  const share = percent === null ? "" : ` (${percent}%)`;
  return `${formatBytes(memory.machineUsedBytes)} of ${formatBytes(memory.machineTotalBytes)} in use${share} · relay process ${formatBytes(memory.processRssBytes)}`;
}

/** "container 1.2 GiB of 2 GiB (60%)", or `null` when no limit applies. */
export function relayContainerSentence(
  memory: RelaySystemHealth["memory"],
): string | null {
  if (memory.container === null) return null;
  const percent = usedPercent(
    memory.container.usedBytes,
    memory.container.limitBytes,
  );
  const share = percent === null ? "" : ` (${percent}%)`;
  return `container ${formatBytes(memory.container.usedBytes)} of ${formatBytes(memory.container.limitBytes)}${share}`;
}

/** "git data, root · 40 GiB free of 100 GiB (60% used)". */
export function relayDiskSentence(disk: RelayDiskHealth): string {
  const used = disk.totalBytes - disk.availableBytes;
  const percent = usedPercent(used, disk.totalBytes);
  const share = percent === null ? "" : ` (${percent}% used)`;
  return `${disk.labels.join(", ")} · ${formatBytes(disk.availableBytes)} free of ${formatBytes(disk.totalBytes)}${share}`;
}

/**
 * "sampled just now" / "sampled 12 s ago" / "sampled 3 min ago", with
 * "(stale)" once the sample is older than three intervals — the sampler
 * has missed at least two beats, so the numbers describe the past.
 */
export function relaySampleAgeSentence(
  ageSeconds: number,
  intervalSeconds: number,
): string {
  const stale =
    intervalSeconds > 0 && ageSeconds > intervalSeconds * 3 ? " (stale)" : "";
  if (ageSeconds < 5) return `sampled just now${stale}`;
  if (ageSeconds < 90) return `sampled ${Math.round(ageSeconds)} s ago${stale}`;
  return `sampled ${Math.round(ageSeconds / 60)} min ago${stale}`;
}

/** Why a read did not produce numbers, from the Tauri error string. */
export type RelaySystemHealthReadFailure =
  | "forbidden"
  | "unsupported"
  | "not-sampled"
  | "unreachable"
  | "rate-limited"
  | "failed";

/**
 * Read the relay's answer from the error the Tauri command threw. The
 * prefixes are `relay_error_message`'s (`desktop/src-tauri/src/relay.rs`).
 */
export function classifyRelaySystemHealthError(
  error: unknown,
): RelaySystemHealthReadFailure {
  const message = error instanceof Error ? error.message : String(error);
  if (message.startsWith("relay returned 403")) return "forbidden";
  if (message.startsWith("relay returned 404")) return "unsupported";
  if (message.startsWith("relay returned 503")) return "not-sampled";
  if (message.startsWith("relay unreachable:")) return "unreachable";
  if (message.startsWith("relay rate-limited:")) return "rate-limited";
  return "failed";
}

/** The sentence the card prints for a read that produced no numbers. */
export function relaySystemHealthFailureSentence(
  failure: RelaySystemHealthReadFailure,
): string {
  switch (failure) {
    case "forbidden":
      return "Only this community's owners and admins can read the relay's machine.";
    case "unsupported":
      return "This relay does not report its machine yet — it predates the health endpoint.";
    case "not-sampled":
      return "The relay has not sampled its machine yet; it will within a few seconds of starting.";
    case "unreachable":
      return "Could not reach the relay.";
    case "rate-limited":
      return "The relay is rate-limiting this computer; retrying shortly.";
    case "failed":
      return "Could not read the relay's machine health.";
  }
}
