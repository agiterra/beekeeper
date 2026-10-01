import * as React from "react";

import { uploadMediaFile } from "@/shared/api/tauriMedia";
import { useFilePicker } from "@/shared/hooks/useFilePicker";
import {
  CODING_SESSION_IMAGE_ATTACHMENT_MIMES,
  CODING_SESSION_TEXT_ATTACHMENT_MIME,
  MAX_CODING_SESSION_ATTACHMENT_BYTES,
  MAX_CODING_SESSION_ATTACHMENTS,
  MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES,
} from "./codingSessionCommand";

/**
 * Which of the two things a turn can carry this entry is.
 *
 * Not cosmetic. An image reaches the agent as an ACP `image` block and needs a
 * runtime that advertised `promptImage`; text reaches it as a `text` block and
 * needs nothing, because a turn is already text. They are also referenced
 * differently in the draft, bounded differently in bytes, and rendered
 * differently in the strip.
 */
export type CodingSessionAttachmentKind = "image" | "text";

/** One attachment the operator has staged on the current draft. */
export type CodingSessionAttachment = {
  /** Stable local id — the list key, and what remove addresses. */
  id: number;
  kind: CodingSessionAttachmentKind;
  /**
   * Object URL for the thumbnail, revoked when the entry leaves the list.
   * Images only: a pasted file has nothing to show a picture of.
   */
  previewUrl?: string;
  /**
   * The image's own filename. Absent for text, whose name is derived from its
   * position by {@link attachmentLabel} so that it can never drift out of step
   * with the `[Pasted text #N]` token the person is reading.
   */
  filename?: string;
  /**
   * Text only: lines in the paste, for the chip. The header the agent reads
   * carries its own count, recomputed by the provider from the bytes it
   * actually fetched — so a chip and a prompt can never disagree about a file
   * one of them never saw.
   */
  lineCount?: number;
  /** Absent until the upload lands. */
  sha256?: string;
  /**
   * The relay media URL the draft references this attachment by. Absent until
   * the upload lands, at which point the draft's token is swapped for markdown
   * pointing here.
   */
  url?: string;
  mime?: string;
  /** Known up front for text, and from the descriptor for an image. */
  size?: number;
  dim?: string;
  /** Set when this attachment's upload failed; the row shows it and can be removed. */
  error?: string;
};

/** The wire form the 44220 payload carries. */
export type CodingSessionAttachmentRef = {
  sha256: string;
  mime: string;
  size: number;
  dim?: string;
  filename?: string;
};

/**
 * A paste of more than this many lines becomes an attachment.
 *
 * Trailing blank lines do not count — see {@link pastedTextLineCount}. Five is
 * about where a paste stops being part of the sentence a person is writing and
 * starts being a thing they are showing you.
 */
export const PASTED_TEXT_MAX_INLINE_LINES = 5;
/** A paste larger than this many UTF-8 bytes becomes an attachment. */
export const PASTED_TEXT_MAX_INLINE_BYTES = 600;

function isSupportedImage(file: File): boolean {
  return (CODING_SESSION_IMAGE_ATTACHMENT_MIMES as readonly string[]).includes(
    file.type,
  );
}

function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}

/**
 * Lines in a paste, ignoring the trailing newlines a copy almost always picks
 * up.
 *
 * Counting them would make a five-line selection copied with its final newline
 * read as six and be attached, which is the one case a person would most
 * clearly call wrong.
 */
export function pastedTextLineCount(text: string): number {
  const body = text.replace(/\n+$/, "");
  return body.length === 0 ? 0 : body.split("\n").length;
}

/**
 * Whether a paste is large enough to belong in the attachment strip rather than
 * in the middle of the draft.
 *
 * Either bound is enough on its own: five lines of short output and one very
 * long single line are both things that swamp a composer, and only one of them
 * is big in bytes.
 *
 * This is also what lifts the ceiling. A turn's own text is capped at
 * `MAX_CODING_SESSION_TEXT_BYTES` (12 KiB) by the signed command contract, so a
 * long log pasted inline does not merely read badly — past that size the turn
 * cannot be published at all. As an attachment it has its own budget.
 */
export function shouldAttachPastedText(text: string): boolean {
  if (text.length === 0) return false;
  return (
    pastedTextLineCount(text) > PASTED_TEXT_MAX_INLINE_LINES ||
    utf8Bytes(text) > PASTED_TEXT_MAX_INLINE_BYTES
  );
}

/**
 * How an attachment is referred to in the draft: `[Image #1]`, `[Pasted text
 * #2]`, …
 *
 * A readable token rather than the markdown it becomes on the wire. The URL
 * carries a 64-character hash, which mid-sentence is unreadable and tells the
 * person nothing about *which* thing they are looking at; a number matches the
 * strip and survives the caret moving around it.
 *
 * Numbered **within a kind**, so the first paste is always `[Pasted text #1]`
 * however many screenshots sit beside it. One shared numbering would have made
 * the first paste in an image-first turn read as `#2`, which is a number that
 * matches nothing the person can see.
 */
export function attachmentToken(
  kind: CodingSessionAttachmentKind,
  ordinal: number,
): string {
  return kind === "image" ? `[Image #${ordinal}]` : `[Pasted text #${ordinal}]`;
}

/** One-based position of `index` among the entries of its own kind. */
export function attachmentOrdinal(
  attachments: readonly CodingSessionAttachment[],
  index: number,
): number {
  const kind = attachments[index]?.kind;
  let ordinal = 0;
  for (let at = 0; at <= index; at++) {
    if (attachments[at]?.kind === kind) ordinal += 1;
  }
  return ordinal;
}

/** The filename a paste carries on the wire and in the strip. */
export function pastedTextFilename(ordinal: number): string {
  return `pasted-text-${ordinal}.txt`;
}

/**
 * The name to show for an entry, and the one its wire `filename` carries.
 *
 * Derived from the *current* position for text, so the link label in the
 * transcript, the `filename` the provider puts in the agent's header, and the
 * `[Pasted text #N]` token the person was editing are the same number. Deriving
 * it at stage time instead would leave `pasted-text-2.txt` labelled `#1` after
 * an earlier paste was removed.
 */
export function attachmentLabel(
  attachments: readonly CodingSessionAttachment[],
  index: number,
): string {
  const attachment = attachments[index];
  if (attachment === undefined) return "";
  if (attachment.kind === "image") return attachment.filename ?? "image";
  return pastedTextFilename(attachmentOrdinal(attachments, index));
}

/**
 * Drop the removed entry's token from the draft and close the gap it leaves.
 *
 * Every later token *of the same kind* shifts down one, so the number a person
 * reads always matches the row they can see; the other kind is untouched,
 * because its numbering never moved. Renumbering **ascending** is what makes
 * that safe: each token moves onto a number this pass has already vacated, so
 * two entries can never collide on one token.
 */
export function renumberDraftAfterRemoval(
  draft: string,
  attachments: readonly CodingSessionAttachment[],
  removedIndex: number,
): string {
  const kind = attachments[removedIndex]?.kind;
  if (kind === undefined) return draft;
  const removedOrdinal = attachmentOrdinal(attachments, removedIndex);
  const total = attachments.filter(
    (attachment) => attachment.kind === kind,
  ).length;
  let next = draft.replaceAll(attachmentToken(kind, removedOrdinal), "");
  for (let later = removedOrdinal + 1; later <= total; later++) {
    next = next.replaceAll(
      attachmentToken(kind, later),
      attachmentToken(kind, later - 1),
    );
  }
  // Close the hole the token left without disturbing the person's line breaks.
  return next
    .replace(/[^\S\n]{2,}/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/**
 * A paste's size, for the one line the strip has room for.
 *
 * Kilobytes and megabytes are the units a person reads here, so bytes are only
 * shown below a kilobyte, where "0.6 KB" would be less informative than the
 * number itself.
 */
export function formatAttachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The markdown the wire carries, which is what renders in the transcript. */
export function uploadedImageMarkdown(url: string): string {
  return `![image](${url})`;
}

/**
 * The markdown a pasted file carries: a plain link, not an image one.
 *
 * A link is what the transcript can offer a person — the blob is served as a
 * download — and it is also the reference the provider splits the prompt on, so
 * the agent reads the file where the sentence put it.
 */
export function uploadedTextMarkdown(filename: string, url: string): string {
  return `[${filename}](${url})`;
}

/**
 * Swap every `[Image #N]`/`[Pasted text #N]` for the markdown of the Nth
 * attachment of that kind.
 *
 * Applied on the way to the relay, never to the draft: what the person keeps
 * editing stays readable, and what the transcript renders is a picture or a
 * link. A token with no attachment behind it is left exactly as typed — someone
 * writing "[Image #7]" in prose is writing prose.
 */
export function expandAttachmentTokens(
  text: string,
  attachments: readonly CodingSessionAttachment[],
): string {
  return text.replace(
    /\[(Image|Pasted text) #(\d+)\]/g,
    (whole, label: string, digits: string) => {
      const kind: CodingSessionAttachmentKind =
        label === "Image" ? "image" : "text";
      const wanted = Number(digits);
      const index = attachments.findIndex(
        (attachment, at) =>
          attachment.kind === kind &&
          attachmentOrdinal(attachments, at) === wanted,
      );
      const url = index < 0 ? undefined : attachments[index]?.url;
      if (url === undefined) return whole;
      return kind === "image"
        ? uploadedImageMarkdown(url)
        : uploadedTextMarkdown(attachmentLabel(attachments, index), url);
    },
  );
}

/**
 * Staging for what an operator attaches to a coding-session turn: pick, paste or
 * drop an image, or paste a blob of text large enough to be a file. Each is
 * uploaded to the relay's Blossom store and the composer is handed the hashes
 * its 44220 payload carries.
 *
 * Deliberately *not* `features/messages/lib/useMediaUpload`. That hook carries
 * the chat composer's world — imeta slots, spoilers, video poster frames,
 * annotation and revert, deferred-upload epochs — none of which a turn has. The
 * shared primitives underneath it (`uploadMediaFile`, `useFilePicker`) are what
 * is reused here.
 */
export function useCodingSessionTurnAttachments(options?: {
  /**
   * This execution's advertised `promptImage`. When false every image entry
   * point is inert, because an image block would fail the whole turn — but
   * pasting text still works, since text needs no capability.
   */
  canAttachImages?: boolean;
  /** When false, a large paste stays inline. Defaults to on. */
  canAttachText?: boolean;
  /**
   * Write a token where the person is typing. The composer owns the caret, so
   * it owns this — the hook only says *what* to insert.
   */
  onInsertAtCaret?: (text: string) => void;
  /** Rewrite the whole draft, for removal and the renumbering it forces. */
  onTransformDraft?: (transform: (draft: string) => string) => void;
}) {
  const canAttachImages = options?.canAttachImages ?? true;
  const canAttachText = options?.canAttachText ?? true;
  const insertAtCaretRef = React.useRef(options?.onInsertAtCaret);
  insertAtCaretRef.current = options?.onInsertAtCaret;
  const transformDraftRef = React.useRef(options?.onTransformDraft);
  transformDraftRef.current = options?.onTransformDraft;
  const [attachments, setAttachments] = React.useState<
    CodingSessionAttachment[]
  >([]);
  const [uploadingCount, setUploadingCount] = React.useState(0);
  const [error, setError] = React.useState<string | null>(null);
  const [isDragOver, setIsDragOver] = React.useState(false);

  const nextIdRef = React.useRef(0);
  /**
   * Nested dragenter/dragleave pairs, so the highlight only clears when the
   * pointer truly leaves the composer rather than crossing a child element.
   */
  const dragDepthRef = React.useRef(0);
  const attachmentsRef = React.useRef(attachments);
  attachmentsRef.current = attachments;

  const revokePreview = React.useCallback((url: string | undefined) => {
    if (url?.startsWith("blob:")) URL.revokeObjectURL(url);
  }, []);

  // Release every object URL still held when the composer unmounts.
  React.useEffect(
    () => () => {
      for (const attachment of attachmentsRef.current) {
        if (attachment.previewUrl?.startsWith("blob:")) {
          URL.revokeObjectURL(attachment.previewUrl);
        }
      }
    },
    [],
  );

  /**
   * Upload one staged entry and fold the descriptor back into its row.
   *
   * `expectedMime` is the guard that keeps a relay which has not learned to
   * classify text uploads from producing an attachment the signed command would
   * be refused for. The relay never trusts a declared Content-Type — it sniffs
   * the bytes — so what it stored is the only authority on what the blob is,
   * and a paste stored as `application/octet-stream` is not attachable. Saying
   * so here is the difference between one clear row and an opaque relay
   * rejection after the person presses Send.
   */
  const upload = React.useCallback(
    (file: File, id: number, expectedMime?: string) => {
      void (async () => {
        try {
          const descriptor = await uploadMediaFile(file);
          if (expectedMime !== undefined && descriptor.type !== expectedMime) {
            throw new Error(
              `the relay stored this as ${descriptor.type} rather than ${expectedMime}, so it cannot be attached to a turn`,
            );
          }
          setAttachments((current) =>
            current.map((attachment) =>
              attachment.id === id
                ? {
                    ...attachment,
                    dim: descriptor.dim,
                    mime: descriptor.type,
                    sha256: descriptor.sha256,
                    size: descriptor.size,
                    url: descriptor.url,
                  }
                : attachment,
            ),
          );
        } catch (cause) {
          // Kept in the list rather than dropped: a row that vanishes on
          // failure looks like the attachment was sent.
          const message =
            cause instanceof Error ? cause.message : String(cause);
          setAttachments((current) =>
            current.map((attachment) =>
              attachment.id === id
                ? { ...attachment, error: message }
                : attachment,
            ),
          );
          setError(`Could not upload ${file.name}: ${message}`);
        } finally {
          setUploadingCount((count) => Math.max(0, count - 1));
        }
      })();
    },
    [],
  );

  const addFiles = React.useCallback(
    (files: File[]) => {
      if (!canAttachImages || files.length === 0) return;

      const images = files.filter(isSupportedImage);
      if (images.length < files.length) {
        setError(
          "Only PNG, JPEG, GIF and WebP images can be attached to a turn.",
        );
      }
      const oversized = images.filter(
        (file) => file.size > MAX_CODING_SESSION_ATTACHMENT_BYTES,
      );
      if (oversized.length > 0) {
        setError("Images must be 10 MB or smaller.");
      }

      const room =
        MAX_CODING_SESSION_ATTACHMENTS - attachmentsRef.current.length;
      const accepted = images
        .filter((file) => file.size <= MAX_CODING_SESSION_ATTACHMENT_BYTES)
        .slice(0, Math.max(0, room));
      if (accepted.length === 0) {
        if (room <= 0) setError(atCapacityMessage());
        return;
      }

      const staged = accepted.map((file) => {
        const id = nextIdRef.current;
        nextIdRef.current += 1;
        return {
          file,
          entry: {
            filename: file.name,
            id,
            kind: "image" as const,
            previewUrl: URL.createObjectURL(file),
          } satisfies CodingSessionAttachment,
        };
      });
      setAttachments((current) => [
        ...current,
        ...staged.map((item) => item.entry),
      ]);
      setUploadingCount((count) => count + staged.length);
      // Written at the caret, so the image sits where the person put it —
      // "when I do X I see this: [Image #1]" rather than a tray under the box.
      const firstOrdinal =
        attachmentsRef.current.filter(
          (attachment) => attachment.kind === "image",
        ).length + 1;
      staged.forEach((_, offset) => {
        insertAtCaretRef.current?.(
          attachmentToken("image", firstOrdinal + offset),
        );
      });

      for (const { file, entry } of staged) upload(file, entry.id);
    },
    [canAttachImages, upload],
  );

  /**
   * Stage a pasted blob as a text file.
   *
   * Returns whether it was taken, because the caller has to decide whether to
   * let the browser paste the text inline instead. Refusing *and* swallowing
   * the paste would lose the person's clipboard for them.
   */
  const addPastedText = React.useCallback(
    (text: string): boolean => {
      if (!canAttachText || text.length === 0) return false;
      const size = utf8Bytes(text);
      if (size > MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES) {
        setError(
          `A pasted file must be ${Math.floor(MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES / 1024)} KB or smaller. Put something this large in the repository instead.`,
        );
        return false;
      }
      if (attachmentsRef.current.length >= MAX_CODING_SESSION_ATTACHMENTS) {
        setError(atCapacityMessage());
        return false;
      }

      const id = nextIdRef.current;
      nextIdRef.current += 1;
      const ordinal =
        attachmentsRef.current.filter(
          (attachment) => attachment.kind === "text",
        ).length + 1;
      const filename = pastedTextFilename(ordinal);
      setAttachments((current) => [
        ...current,
        {
          id,
          kind: "text",
          lineCount: pastedTextLineCount(text),
          size,
        },
      ]);
      setUploadingCount((count) => count + 1);
      insertAtCaretRef.current?.(attachmentToken("text", ordinal));
      // A `File` so the one upload path serves both kinds. Its declared type is
      // advisory only — the relay classifies from the bytes — which is exactly
      // why the upload asserts what came back.
      upload(
        new File([text], filename, {
          type: CODING_SESSION_TEXT_ATTACHMENT_MIME,
        }),
        id,
        CODING_SESSION_TEXT_ATTACHMENT_MIME,
      );
      return true;
    },
    [canAttachText, upload],
  );

  const remove = React.useCallback(
    (id: number) => {
      const index = attachmentsRef.current.findIndex(
        (attachment) => attachment.id === id,
      );
      if (index < 0) return;
      const removed = attachmentsRef.current[index];
      revokePreview(removed.previewUrl);

      // Take the reference out with the attachment, and close the gap it
      // leaves. The list is captured here because `setAttachments` below is
      // what makes it stale.
      const before = attachmentsRef.current;
      transformDraftRef.current?.((draft) =>
        renumberDraftAfterRemoval(draft, before, index),
      );

      setAttachments((current) =>
        current.filter((attachment) => attachment.id !== id),
      );
    },
    [revokePreview],
  );

  const clear = React.useCallback(() => {
    setAttachments((current) => {
      for (const attachment of current) revokePreview(attachment.previewUrl);
      return [];
    });
    setError(null);
  }, [revokePreview]);

  const openFilePicker = useFilePicker();
  const pick = React.useCallback(() => {
    if (!canAttachImages) return;
    openFilePicker(
      {
        accept: CODING_SESSION_IMAGE_ATTACHMENT_MIMES.join(","),
        multiple: true,
      },
      addFiles,
    );
  }, [addFiles, canAttachImages, openFilePicker]);

  const handlePaste = React.useCallback(
    (event: React.ClipboardEvent<HTMLElement>) => {
      // `getAsFile()` is null for string items, so an image on the clipboard is
      // the only thing that lands in `files`. Checked first: a screenshot
      // copied out of a browser carries a text fallback beside it, and the
      // picture is what the person meant.
      const files = Array.from(event.clipboardData?.items ?? [])
        .filter((item) => item.kind === "file")
        .map((item) => item.getAsFile())
        .filter((file): file is File => file !== null);
      if (files.length > 0) {
        if (!canAttachImages) return;
        event.preventDefault();
        addFiles(files);
        return;
      }
      const pasted = event.clipboardData?.getData("text/plain") ?? "";
      if (!shouldAttachPastedText(pasted)) return;
      // Only once it has actually been taken. A refusal — no room, or larger
      // than a turn may carry — falls through to the browser's own paste, so
      // the words land in the draft rather than nowhere.
      if (addPastedText(pasted)) event.preventDefault();
    },
    [addFiles, addPastedText, canAttachImages],
  );

  const handleDrop = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      event.preventDefault();
      dragDepthRef.current = 0;
      setIsDragOver(false);
      if (!canAttachImages) return;
      addFiles(Array.from(event.dataTransfer?.files ?? []));
    },
    [addFiles, canAttachImages],
  );

  const handleDragEnter = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!canAttachImages || !event.dataTransfer?.types.includes("Files")) {
        return;
      }
      event.preventDefault();
      dragDepthRef.current += 1;
      if (dragDepthRef.current === 1) setIsDragOver(true);
    },
    [canAttachImages],
  );

  const handleDragLeave = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!canAttachImages || !event.dataTransfer?.types.includes("Files")) {
        return;
      }
      event.preventDefault();
      dragDepthRef.current -= 1;
      if (dragDepthRef.current <= 0) {
        dragDepthRef.current = 0;
        setIsDragOver(false);
      }
    },
    [canAttachImages],
  );

  const handleDragOver = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!canAttachImages) return;
      event.preventDefault();
    },
    [canAttachImages],
  );

  // A drag that ends anywhere else in the window must clear the highlight;
  // browsers do not reliably balance dragenter/dragleave across the boundary.
  React.useEffect(() => {
    function reset() {
      dragDepthRef.current = 0;
      setIsDragOver(false);
    }
    window.addEventListener("drop", reset);
    window.addEventListener("dragend", reset);
    return () => {
      window.removeEventListener("drop", reset);
      window.removeEventListener("dragend", reset);
    };
  }, []);

  /**
   * The refs the 44220 payload carries — settled uploads only.
   *
   * Send is gated on `isUploading` and `hasFailed`, so in practice this is
   * every staged attachment; filtering here means a bug in that gate cannot
   * publish a hash the relay has no blob for.
   */
  const attachmentRefs = React.useMemo<CodingSessionAttachmentRef[]>(
    () =>
      attachments.flatMap((attachment, index) =>
        attachment.sha256 && attachment.mime && attachment.size !== undefined
          ? [
              {
                dim: attachment.dim,
                filename: attachmentLabel(attachments, index),
                mime: attachment.mime,
                sha256: attachment.sha256,
                size: attachment.size,
              },
            ]
          : [],
      ),
    [attachments],
  );

  return {
    addPastedText,
    attachmentRefs,
    attachments,
    clear,
    error,
    handleDragEnter,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handlePaste,
    hasFailed: attachments.some((attachment) => attachment.error !== undefined),
    isDragOver,
    isUploading: uploadingCount > 0,
    pick,
    remove,
    setError,
  };
}

/** One sentence for the one cap both kinds share. */
function atCapacityMessage(): string {
  return `A turn can carry at most ${MAX_CODING_SESSION_ATTACHMENTS} attachments.`;
}

export type CodingSessionAttachmentController = ReturnType<
  typeof useCodingSessionTurnAttachments
>;
