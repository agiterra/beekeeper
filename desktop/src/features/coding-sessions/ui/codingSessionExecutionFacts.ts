import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";

/**
 * The facts an execution card states about where it runs and what it may
 * do, each worded so it claims exactly what its wire fact proves.
 *
 * Three readers live here because the three answers share one rule: absent
 * evidence is reported as absent — "not reported", "unknown" — and never
 * replaced by the comfortable default. A runtime name a descriptor declared
 * renders by that name; a capability the catalog never mentioned renders as
 * unknown rather than as no; a credential label is shown only when a signed
 * 44223 carried one, and a 44223 without one is "not reported", never a
 * guess at the machine's login.
 */

/** The subset of a trusted 44223 entry these readers need. */
export type CodingSessionCredentialSourceFact = {
  signerPubkey: string;
  targetKey: string;
  metadata: { credentialSource?: string };
};

export type CodingSessionCredentialSourceDescription = {
  /** The value for a "Credentials" fact row. */
  label: string;
  /** The sentence behind it, for a `title`. */
  title: string;
  /** `true` only when a signed 44223 for this execution was read. */
  known: boolean;
};

/**
 * Labels that look like a filesystem path, a URL, or raw key material are
 * not operator names and are withheld. The wire contract already forbids
 * them (`coding_session_payload.rs` § credentialSource); this is the second
 * lock on the door, because the one thing worse than no label is a secret
 * on a card.
 */
export function codingSessionCredentialLabelLooksLikeSecret(
  label: string,
): boolean {
  const trimmed = label.trim();
  return (
    trimmed.includes("/") ||
    trimmed.includes("\\") ||
    trimmed.startsWith("~") ||
    /^[a-z]+:\/\//i.test(trimmed) ||
    /^[0-9a-f]{32,}$/i.test(trimmed) ||
    /^(sk|key|token|secret|bearer)[-_ ]/i.test(trimmed)
  );
}

/**
 * Which credentials this execution's runtime was given, from the newest
 * signed 44223 for its exact target.
 *
 * `entries` arrive newest-first from the ingress store, so the first match
 * is the current generation's word. A label is the operator's name for a
 * custody (`acme-vault`). Its absence on a 44223 that was read proves only
 * that the provider did not label one: `bee host commission
 * --credential-file X` without `--credential-source` is accepted and writes
 * no label, so the runtime may well be using a separate credential. The
 * row therefore says "not reported" in both cases and only the sentence
 * behind it differs — whether a 44223 was read at all.
 */
export function describeCodingSessionCredentialSource(
  entries: readonly CodingSessionCredentialSourceFact[],
  target: { signerPubkey: string; targetKey: string } | null,
): CodingSessionCredentialSourceDescription {
  if (target === null) {
    return {
      label: "not reported",
      title:
        "No signed session metadata names this execution's target yet, so its credential source is unknown.",
      known: false,
    };
  }
  const signer = target.signerPubkey.trim().toLowerCase();
  const entry = entries.find(
    (candidate) =>
      candidate.targetKey === target.targetKey &&
      candidate.signerPubkey.trim().toLowerCase() === signer,
  );
  if (entry === undefined) {
    return {
      label: "not reported",
      title:
        "No signed session metadata has arrived for this execution, so its credential source is unknown.",
      known: false,
    };
  }
  const label = entry.metadata.credentialSource?.trim();
  if (label === undefined || label.length === 0) {
    return {
      label: "credential source not reported",
      title:
        "The runtime descriptor carries no credentialSource label, so which credentials this runtime uses is not known here.",
      known: true,
    };
  }
  if (codingSessionCredentialLabelLooksLikeSecret(label)) {
    return {
      label: "named source (label withheld)",
      title:
        "The provider named a credential source, but the label looks like a path, URL or key and is not shown.",
      known: true,
    };
  }
  return {
    label,
    title: `The provider named "${label}" as the credential source for this runtime. Only the label is on the wire — never the credential.`,
    known: true,
  };
}

export type CodingSessionRuntimeName = {
  /** The runtime's display name, or "Runtime not reported". */
  label: string;
  /** The driver the command target names, when it adds a fact. */
  driver: string | null;
  /** `true` when no fact on the record named the runtime. */
  unreported: boolean;
};

/**
 * What to call the runtime under an execution: the name the 44223 gives
 * (`runtime`), else the provider's label, else the driver its command target
 * names, else a plain "not reported".
 *
 * A descriptor-declared runtime arrives with whatever name the operator gave
 * it (`my-harness`); it is title-cased like any other and never collapsed to
 * "unknown". `driver` is returned beside the name only when it says something
 * the name does not — `claude-agent-acp` under "Claude Code" is the same
 * fact twice and is dropped.
 */
export function codingSessionRuntimeName(record: {
  runtime: string | null;
  provider: string | null;
  commandTarget: { driver: string } | null;
}): CodingSessionRuntimeName {
  const driver = record.commandTarget?.driver.trim() || null;
  const named = record.runtime?.trim() || record.provider?.trim() || null;
  if (named === null) {
    return driver === null
      ? { label: "Runtime not reported", driver: null, unreported: true }
      : {
          label: formatCodingSessionRuntimeLabel(driver),
          driver: null,
          unreported: false,
        };
  }
  const label = formatCodingSessionRuntimeLabel(named);
  if (driver === null) return { label, driver: null, unreported: false };
  const sameFact =
    normalizeRuntimeToken(driver) === normalizeRuntimeToken(named) ||
    formatCodingSessionRuntimeLabel(driver) === label;
  return { label, driver: sameFact ? null : driver, unreported: false };
}

function normalizeRuntimeToken(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replaceAll("_", "-")
    .replace(/-(acp|agent-acp|code)$/, "");
}

/** The capability words an execution can be asked about. */
export type CodingSessionDeclaredCapabilities = {
  threadTurnStart: boolean;
  threadTurnInterrupt: boolean;
  threadSteer: boolean;
  context: boolean;
  diff: boolean;
  plan: boolean;
  promptImage?: boolean;
  modelSwitch?: boolean;
};

export type CodingSessionCapabilityWord = "yes" | "no" | "unknown";

export type CodingSessionCapabilityFact = {
  label: string;
  word: CodingSessionCapabilityWord;
};

/**
 * Every capability, each with a truthful word.
 *
 * The required six are booleans on the wire, so each is yes or no. Images and
 * model switch were added later and are optional: a catalog or metadata
 * published before they existed says nothing about them, and "unknown" is
 * the only honest reading of nothing — "no" would turn a silence into a
 * refusal.
 */
export function listCodingSessionCapabilities(
  capabilities: CodingSessionDeclaredCapabilities | null | undefined,
): CodingSessionCapabilityFact[] {
  if (!capabilities) return [];
  const required = (value: boolean): CodingSessionCapabilityWord =>
    value ? "yes" : "no";
  const optional = (value: boolean | undefined): CodingSessionCapabilityWord =>
    value === undefined ? "unknown" : value ? "yes" : "no";
  return [
    { label: "Turns", word: required(capabilities.threadTurnStart) },
    { label: "Steer", word: required(capabilities.threadSteer) },
    { label: "Interrupt", word: required(capabilities.threadTurnInterrupt) },
    { label: "Context", word: required(capabilities.context) },
    { label: "Diff", word: required(capabilities.diff) },
    { label: "Plan", word: required(capabilities.plan) },
    { label: "Images", word: optional(capabilities.promptImage) },
    { label: "Model switch", word: optional(capabilities.modelSwitch) },
  ];
}

/** `Turns yes · Steer no · … · Images unknown`, or "Not declared". */
export function describeCodingSessionCapabilities(
  capabilities: CodingSessionDeclaredCapabilities | null | undefined,
): string {
  const facts = listCodingSessionCapabilities(capabilities);
  if (facts.length === 0) return "Not declared";
  return facts.map((fact) => `${fact.label} ${fact.word}`).join(" · ");
}
