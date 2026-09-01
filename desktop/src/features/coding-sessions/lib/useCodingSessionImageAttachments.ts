import * as React from "react";

import { uploadMediaFile } from "@/shared/api/tauriMedia";
import { useFilePicker } from "@/shared/hooks/useFilePicker";
import {
  CODING_SESSION_ATTACHMENT_MIMES,
  MAX_CODING_SESSION_ATTACHMENT_BYTES,
  MAX_CODING_SESSION_ATTACHMENTS,
} from "./codingSessionCommand";

/** One attachment the operator has staged on the current draft. */
export type CodingSessionAttachment = {
  /** Stable local id — the list key, and what remove addresses. */
  id: number;
  /** Object URL for the thumbnail, revoked when the entry leaves the list. */
  previewUrl: string;
  filename: string;
  /** Absent until the upload lands. */
  sha256?: string;
  /**
   * The relay media URL the draft references this image by. Absent until the
   * upload lands, at which point the draft's `uploading:` marker is swapped
   * for markdown pointing here.
   */
  url?: string;
  mime?: string;
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

function isSupportedImage(file: File): boolean {
  return (CODING_SESSION_ATTACHMENT_MIMES as readonly string[]).includes(
    file.type,
  );
}

/**
 * Staging for images attached to a coding-session turn: pick, paste or drop an
 * image, upload it to the relay's Blossom store, and hand the composer the
 * hashes its 44220 payload carries.
 *
 * Deliberately *not* `features/messages/lib/useMediaUpload`. That hook carries
 * the chat composer's world — imeta slots, spoilers, video poster frames,
 * annotation and revert, deferred-upload epochs — none of which a turn has. The
 * shared primitives underneath it (`uploadMediaFile`, `useFilePicker`) are what
 * is reused here.
 */
/**
 * How an image is referred to in the draft: `[Image #1]`, `[Image #2]`, …
 *
 * A readable token rather than the markdown it becomes on the wire. The URL
 * carries a 64-character hash, which mid-sentence is unreadable and tells the
 * person nothing about *which* picture they are looking at; a number matches
 * the thumbnail strip and survives the caret moving around it.
 *
 * Numbered by position in the attachment list, so removing one renumbers the
 * rest — the token a person reads is always the thumbnail they can see.
 */
export function imageToken(index: number): string {
  return `[Image #${index + 1}]`;
}

/**
 * Drop `[Image #removed]` from the draft and close the gap it leaves.
 *
 * Every later token shifts down one, so the number a person reads always
 * matches the thumbnail they can see. Renumbering **ascending** is what makes
 * that safe: each token moves onto a number this pass has already vacated, so
 * two images can never collide on one token.
 */
export function renumberDraftAfterRemoval(
  draft: string,
  removedIndex: number,
  total: number,
): string {
  let next = draft.replaceAll(imageToken(removedIndex), "");
  for (let later = removedIndex + 1; later < total; later++) {
    next = next.replaceAll(imageToken(later), imageToken(later - 1));
  }
  // Close the hole the token left without disturbing the person's line breaks.
  return next
    .replace(/[^\S\n]{2,}/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** The markdown the wire carries, which is what renders in the transcript. */
export function uploadedImageMarkdown(url: string): string {
  return `![image](${url})`;
}

/**
 * Swap every `[Image #N]` for the markdown of the Nth attachment.
 *
 * Applied on the way to the relay, never to the draft: what the person keeps
 * editing stays readable, and what the transcript renders is an image. A token
 * with no attachment behind it is left exactly as typed — someone writing
 * "[Image #7]" in prose is writing prose.
 */
export function expandImageTokens(
  text: string,
  attachments: readonly CodingSessionAttachment[],
): string {
  return text.replace(/\[Image #(\d+)\]/g, (whole, digits: string) => {
    const url = attachments[Number(digits) - 1]?.url;
    return url ? uploadedImageMarkdown(url) : whole;
  });
}

export function useCodingSessionImageAttachments(options?: {
  /** When false, every entry point is inert — see `canAttachImages`. */
  enabled?: boolean;
  /**
   * Write a token where the person is typing. The composer owns the caret, so
   * it owns this — the hook only says *what* to insert.
   */
  onInsertAtCaret?: (text: string) => void;
  /** Rewrite the whole draft, for removal and the renumbering it forces. */
  onTransformDraft?: (transform: (draft: string) => string) => void;
}) {
  const enabled = options?.enabled ?? true;
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

  const revokePreview = React.useCallback((url: string) => {
    if (url.startsWith("blob:")) URL.revokeObjectURL(url);
  }, []);

  // Release every object URL still held when the composer unmounts.
  React.useEffect(
    () => () => {
      for (const attachment of attachmentsRef.current) {
        if (attachment.previewUrl.startsWith("blob:")) {
          URL.revokeObjectURL(attachment.previewUrl);
        }
      }
    },
    [],
  );

  const addFiles = React.useCallback(
    (files: File[]) => {
      if (!enabled || files.length === 0) return;

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
        if (room <= 0) {
          setError(
            `A turn can carry at most ${MAX_CODING_SESSION_ATTACHMENTS} images.`,
          );
        }
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
      const firstIndex = attachmentsRef.current.length;
      staged.forEach((_, offset) => {
        insertAtCaretRef.current?.(imageToken(firstIndex + offset));
      });

      for (const { file, entry } of staged) {
        void (async () => {
          try {
            const descriptor = await uploadMediaFile(file);
            setAttachments((current) =>
              current.map((attachment) =>
                attachment.id === entry.id
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
            // Kept in the list rather than dropped: a thumbnail that vanishes
            // on failure looks like the image was sent.
            setAttachments((current) =>
              current.map((attachment) =>
                attachment.id === entry.id
                  ? { ...attachment, error: String(cause) }
                  : attachment,
              ),
            );
            setError(`Could not upload ${entry.filename}.`);
          } finally {
            setUploadingCount((count) => Math.max(0, count - 1));
          }
        })();
      }
    },
    [enabled],
  );

  const remove = React.useCallback(
    (id: number) => {
      const index = attachmentsRef.current.findIndex(
        (attachment) => attachment.id === id,
      );
      if (index < 0) return;
      const removed = attachmentsRef.current[index];
      revokePreview(removed.previewUrl);

      // Take the reference out with the image, and close the gap it leaves.
      transformDraftRef.current?.((draft) =>
        renumberDraftAfterRemoval(draft, index, attachmentsRef.current.length),
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
    if (!enabled) return;
    openFilePicker(
      { accept: CODING_SESSION_ATTACHMENT_MIMES.join(","), multiple: true },
      addFiles,
    );
  }, [addFiles, enabled, openFilePicker]);

  const handlePaste = React.useCallback(
    (event: React.ClipboardEvent<HTMLElement>) => {
      if (!enabled) return;
      // `getAsFile()` is null for string items, so pasting text never lands
      // here — only an actual image on the clipboard does.
      const files = Array.from(event.clipboardData?.items ?? [])
        .filter((item) => item.kind === "file")
        .map((item) => item.getAsFile())
        .filter((file): file is File => file !== null);
      if (files.length === 0) return;
      event.preventDefault();
      addFiles(files);
    },
    [addFiles, enabled],
  );

  const handleDrop = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      event.preventDefault();
      dragDepthRef.current = 0;
      setIsDragOver(false);
      if (!enabled) return;
      addFiles(Array.from(event.dataTransfer?.files ?? []));
    },
    [addFiles, enabled],
  );

  const handleDragEnter = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!enabled || !event.dataTransfer?.types.includes("Files")) return;
      event.preventDefault();
      dragDepthRef.current += 1;
      if (dragDepthRef.current === 1) setIsDragOver(true);
    },
    [enabled],
  );

  const handleDragLeave = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!enabled || !event.dataTransfer?.types.includes("Files")) return;
      event.preventDefault();
      dragDepthRef.current -= 1;
      if (dragDepthRef.current <= 0) {
        dragDepthRef.current = 0;
        setIsDragOver(false);
      }
    },
    [enabled],
  );

  const handleDragOver = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      if (!enabled) return;
      event.preventDefault();
    },
    [enabled],
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
      attachments.flatMap((attachment) =>
        attachment.sha256 && attachment.mime && attachment.size !== undefined
          ? [
              {
                dim: attachment.dim,
                filename: attachment.filename,
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

export type CodingSessionAttachmentController = ReturnType<
  typeof useCodingSessionImageAttachments
>;
