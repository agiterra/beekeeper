/**
 * Who is allowed to author a coding-session event, resolved from the
 * `allowed-bridge-pubkeys` field on the global agent config.
 *
 * Fail-closed by construction: an empty or malformed list produces an
 * `invalid` authority, and ingress admits nothing under one. Trust is the
 * pubkey; `label` is display metadata only.
 */
export type CodingSessionIngressSource = {
  pubkey: string;
  label: string;
};

export type CodingSessionIngressAuthority =
  | {
      state: "valid";
      allowed: CodingSessionIngressSource[];
      byPubkey: Map<string, CodingSessionIngressSource>;
    }
  | {
      state: "invalid";
      errorMessage: string;
    };

/**
 * Resolve the trusted signer set.
 *
 * The whole list is rejected on the first bad entry rather than filtered down
 * to the good ones: a config the consumer cannot read exactly is a config it
 * must not partially honour.
 */
export function resolveCodingSessionIngressAuthority(
  entries: unknown,
): CodingSessionIngressAuthority {
  if (!Array.isArray(entries) || entries.length === 0) {
    return {
      state: "invalid",
      errorMessage:
        "Coding sessions disabled: no allowed provider pubkeys configured.",
    };
  }

  const allowed: CodingSessionIngressSource[] = [];
  const seen = new Set<string>();
  for (const entry of entries) {
    if (!isRecord(entry)) {
      return invalidAuthority();
    }
    const pubkey = normalizeConfigPubkey(entry.pubkey);
    if (pubkey === null || seen.has(pubkey)) {
      return invalidAuthority();
    }
    seen.add(pubkey);
    allowed.push({
      pubkey,
      label: normalizeConfigLabel(entry.label),
    });
  }

  return {
    state: "valid",
    allowed,
    byPubkey: new Map(allowed.map((entry) => [entry.pubkey, entry])),
  };
}

/**
 * A stable string identity for the resolved authority.
 *
 * Stores key their caches on this so a trust-list edit invalidates everything
 * admitted under the old list instead of leaving it on screen.
 */
export function buildCodingSessionIngressAuthorityIdentity(
  authority: CodingSessionIngressAuthority,
): string {
  if (authority.state !== "valid") {
    return `invalid:${authority.errorMessage}`;
  }
  return `valid:${authority.allowed
    .map((entry) => entry.pubkey)
    .sort()
    .join(",")}`;
}

function invalidAuthority(): CodingSessionIngressAuthority {
  return {
    state: "invalid",
    errorMessage:
      "Coding sessions disabled: allowed provider pubkeys config is invalid.",
  };
}

function normalizeConfigPubkey(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.trim().toLowerCase();
  return isLowercaseHexPubkey(normalized) ? normalized : null;
}

function normalizeConfigLabel(value: unknown): string {
  if (typeof value !== "string") {
    return "Provider";
  }
  const trimmed = safeMetadata(value.trim());
  return trimmed.length > 0 ? trimmed.slice(0, 80) : "Provider";
}

function isLowercaseHexPubkey(value: string): boolean {
  return /^[0-9a-f]{64}$/.test(value);
}

/**
 * Strip C0 controls and DEL so a hostile label cannot smuggle terminal escapes
 * or multi-line structure into a single-line identity. Written as a charCode
 * scan so this source never embeds raw control bytes.
 */
function safeMetadata(value: string): string {
  let result = "";
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    result += code <= 0x1f || code === 0x7f ? " " : value[i];
  }
  return result.slice(0, 120);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
