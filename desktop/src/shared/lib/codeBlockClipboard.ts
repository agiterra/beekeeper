import { copyTextToSystemClipboard } from "@/shared/api/tauriMedia";

const BEEKEEPER_CODE_BLOCK_ATTRIBUTE = "data-beekeeper-code-block";

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function createBeekeeperCodeBlockHtml(code: string) {
  // Keep the code as one text node; the paste reader recovers it via textContent.
  return `<pre ${BEEKEEPER_CODE_BLOCK_ATTRIBUTE}="true"><code>${escapeHtml(code)}</code></pre>`;
}

export async function copyCodeBlockToClipboard(code: string) {
  const clipboard = navigator.clipboard;

  if (
    typeof ClipboardItem !== "undefined" &&
    typeof clipboard?.write === "function"
  ) {
    try {
      await clipboard.write([
        new ClipboardItem({
          "text/html": new Blob([createBeekeeperCodeBlockHtml(code)], {
            type: "text/html",
          }),
          "text/plain": new Blob([code], { type: "text/plain" }),
        }),
      ]);
      return;
    } catch (error) {
      console.warn("Failed to write rich code block clipboard data", error);
    }
  }

  await copyTextToSystemClipboard(code);
}

export function getBeekeeperCodeBlockClipboardText(
  clipboardData: DataTransfer | null | undefined,
) {
  const html = clipboardData?.getData("text/html");
  if (!html?.includes(BEEKEEPER_CODE_BLOCK_ATTRIBUTE)) {
    return null;
  }

  const document = new DOMParser().parseFromString(html, "text/html");
  const code = document.querySelector(
    `[${BEEKEEPER_CODE_BLOCK_ATTRIBUTE}] code`,
  );
  const fallback = document.querySelector(
    `[${BEEKEEPER_CODE_BLOCK_ATTRIBUTE}]`,
  );

  return code?.textContent ?? fallback?.textContent ?? null;
}
