/**
 * H-08 — Buzz transcript-export engine: the pure serialization/planning half
 * of the standalone export.
 *
 * The behavioral contract is banked in `conformance/transcript-export/`
 * (donor: Hive `src/server/standalone-export.ts` at pin `e0b8198bd144`,
 * re-derived — never copied or executed). The bundle law, attachment-mode
 * matrix, share-path rewrite, and naming law implemented here are asserted
 * against the corpus fixtures by
 * `conformance/transcript-export/implementation.test.mjs`.
 *
 * LOAD-BEARING CONSTRAINT: this module must keep ZERO runtime imports and
 * use erasable TypeScript syntax only (no enums, no namespaces, no parameter
 * properties). The conformance bridge imports it under plain `node --test`
 * with Node's native type stripping — a runtime import or non-erasable
 * construct breaks the conformance gate loudly.
 *
 * The filesystem half (directory creation, viewer copy, attachment copy,
 * refusal when the viewer dist is absent) lives Rust-side in
 * `desktop/src-tauri/src/transcript_export/` — this engine only *plans*;
 * filesystem facts (`takenDirectoryNames`, `attachmentSourceExists`) arrive
 * as inputs.
 */

export type TranscriptExportAttachment = {
  id: string;
  displayName: string;
  mimeType: string;
  size: number;
  absolutePath: string;
  relativePath: string;
  contentUrl: string;
};

export type TranscriptExportMessage =
  | {
      kind: "user_prompt";
      id: string;
      text: string;
      timestamp: string;
      attachments: TranscriptExportAttachment[];
    }
  | {
      kind: "assistant_text";
      id: string;
      text: string;
      timestamp: string;
    }
  | {
      kind: "thought" | "plan" | "lifecycle";
      id: string;
      title: string;
      text: string;
      timestamp: string;
    }
  | {
      kind: "tool_call";
      id: string;
      title: string;
      toolName: string;
      status: string;
      isError: boolean;
      text: string;
      timestamp: string;
    };

export type TranscriptExportAttachmentMode = "metadata" | "bundle";

export type TranscriptExportTheme = "light" | "dark";

export const TRANSCRIPT_BUNDLE_VERSION = 1;

/** The share placeholder that replaces the real workspace path everywhere. */
export const SHARE_WORKSPACE_PATH = "/workspace";

/**
 * Trim, collapse every run of characters outside `[A-Za-z0-9_.-]` to a
 * single dash, strip leading/trailing dashes. ASCII by construction: JS `\w`
 * is ASCII-only, matching the banked law (`"üñïçode"` → `"ode"`).
 */
export function sanitizeFileNameSegment(value: string): string {
  return value
    .trim()
    .replace(/[^\w.-]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** ISO instant with `:` → `-` and the `.mmm` milliseconds stripped. */
export function formatExportTimestamp(isoInstant: string): string {
  return isoInstant.replace(/:/g, "-").replace(/\.\d{3}Z$/u, "Z");
}

/**
 * `<sanitized(title || chatId) or "chat">-<timestamp>`, then `-2`, `-3`, …
 * until the candidate is free. A truthy title that sanitizes to nothing
 * falls to the literal `"chat"`, never to chatId.
 */
export function resolveUniqueExportDirName(
  title: string,
  chatId: string,
  isoInstant: string,
  taken: ReadonlySet<string>,
): string {
  const base = `${sanitizeFileNameSegment(title || chatId) || "chat"}-${formatExportTimestamp(isoInstant)}`;
  let candidate = base;
  for (let suffix = 2; taken.has(candidate); suffix += 1) {
    candidate = `${base}-${suffix}`;
  }
  return candidate;
}

export type AppliedAttachmentMode = {
  fields: { absolutePath: string; relativePath: string; contentUrl: string };
  bundled: boolean;
  exportedFileName: string | null;
};

const EMPTIED_ATTACHMENT_FIELDS = {
  absolutePath: "",
  relativePath: "",
  contentUrl: "",
};

/**
 * The banked attachment-mode matrix: `metadata` empties all three reference
 * fields; `bundle` rewrites them to one `./attachments/…` path; `bundle`
 * with a missing or path-less source falls back to the metadata rewrite —
 * never a broken reference.
 */
export function applyAttachmentMode(
  attachment: Pick<
    TranscriptExportAttachment,
    "id" | "displayName" | "absolutePath"
  >,
  mode: TranscriptExportAttachmentMode,
  sourceExists: boolean,
): AppliedAttachmentMode {
  if (mode === "metadata" || !attachment.absolutePath || !sourceExists) {
    return {
      fields: { ...EMPTIED_ATTACHMENT_FIELDS },
      bundled: false,
      exportedFileName: null,
    };
  }
  const base = posixBasename(attachment.displayName || attachment.absolutePath);
  const exportedFileName = `${sanitizeFileNameSegment(attachment.id)}-${sanitizeFileNameSegment(base)}`;
  const relative = `./attachments/${exportedFileName}`;
  return {
    fields: {
      absolutePath: relative,
      relativePath: relative,
      contentUrl: relative,
    },
    bundled: true,
    exportedFileName,
  };
}

function posixBasename(value: string): string {
  const segments = value.split("/");
  return segments[segments.length - 1] ?? value;
}

/**
 * Deep, PURE share rewrite: every occurrence of the workspace path in every
 * string — including a top-level string argument — is replaced with the
 * share placeholder. Returns a new value; never mutates the input. (The
 * donor mutates in place and discards the return value, which would miss a
 * top-level string; the corpus records that shape as one not to copy.)
 */
export function rewriteLocalPathsForShare<T>(
  value: T,
  workspacePath: string,
): T {
  if (!workspacePath) return value;
  return rewriteValue(value, workspacePath) as T;
}

function rewriteValue(value: unknown, workspacePath: string): unknown {
  if (typeof value === "string") {
    return value.split(workspacePath).join(SHARE_WORKSPACE_PATH);
  }
  if (Array.isArray(value)) {
    return value.map((item) => rewriteValue(item, workspacePath));
  }
  if (!value || typeof value !== "object") return value;
  const rewritten: Record<string, unknown> = {};
  for (const [key, nested] of Object.entries(value)) {
    rewritten[key] = rewriteValue(nested, workspacePath);
  }
  return rewritten;
}

export type TranscriptExportPlan = {
  directoryName: string;
  /** The serialized bundle, trailing newline included. Written verbatim. */
  transcriptJson: string;
  attachmentCopies: Array<{
    sourceAbsolutePath: string;
    exportedFileName: string;
  }>;
  totalAttachmentCount: number;
  bundledAttachmentCount: number;
};

export type TranscriptExportPlanArgs = {
  chatId: string;
  title: string;
  /** Empty string when no workspace path exists — the rewrite is a no-op. */
  workspacePath: string;
  theme: TranscriptExportTheme;
  attachmentMode: TranscriptExportAttachmentMode;
  messages: TranscriptExportMessage[];
  nowIso: string;
  viewerVersion: string;
  takenDirectoryNames: readonly string[];
  /** Filesystem facts keyed by the attachment's original absolutePath. */
  attachmentSourceExists: Readonly<Record<string, boolean>>;
};

/**
 * Build the complete export plan: the unique directory name, the serialized
 * bundle (attachment matrix applied, then the deep share rewrite over the
 * whole messages array, `localPath` pinned to the placeholder), and the
 * attachment copy list with its two counters.
 */
export function buildTranscriptExportPlan(
  args: TranscriptExportPlanArgs,
): TranscriptExportPlan {
  const attachmentCopies: TranscriptExportPlan["attachmentCopies"] = [];
  let totalAttachmentCount = 0;
  let bundledAttachmentCount = 0;

  const preparedMessages = args.messages.map((message) => {
    if (message.kind !== "user_prompt") return message;
    const attachments = message.attachments.map((attachment) => {
      totalAttachmentCount += 1;
      const applied = applyAttachmentMode(
        attachment,
        args.attachmentMode,
        args.attachmentSourceExists[attachment.absolutePath] === true,
      );
      if (applied.bundled && applied.exportedFileName) {
        bundledAttachmentCount += 1;
        attachmentCopies.push({
          sourceAbsolutePath: attachment.absolutePath,
          exportedFileName: applied.exportedFileName,
        });
      }
      return { ...attachment, ...applied.fields };
    });
    return { ...message, attachments };
  });

  const bundle = {
    version: TRANSCRIPT_BUNDLE_VERSION,
    chatId: args.chatId,
    title: args.title,
    localPath: SHARE_WORKSPACE_PATH,
    exportedAt: args.nowIso,
    viewerVersion: args.viewerVersion,
    theme: args.theme,
    attachmentMode: args.attachmentMode,
    messages: rewriteLocalPathsForShare(preparedMessages, args.workspacePath),
  };

  return {
    directoryName: resolveUniqueExportDirName(
      args.title,
      args.chatId,
      args.nowIso,
      new Set(args.takenDirectoryNames),
    ),
    transcriptJson: `${JSON.stringify(bundle, null, 2)}\n`,
    attachmentCopies,
    totalAttachmentCount,
    bundledAttachmentCount,
  };
}
