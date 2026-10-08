import type { SessionPreviewRect } from "@/shared/api/tauriSessionPreview";

/**
 * Occlusion leases for the native preview (BRIEF § 1 "Z-order").
 *
 * The preview is an NSView drawn above this window's whole DOM, so no CSS can
 * put a menu or dialog over it. While any app overlay intersects the slot, the
 * slot asks Rust to hide the view (`session_preview_set_occluded(true)`) and
 * paints the freeze frame Rust hands back; the last overlay leaving shows the
 * view again.
 *
 * A lease is a DOM marker, not a registration: an overlay element carrying
 * {@link NATIVE_VIEW_OCCLUDER_ATTR} holds a lease for exactly as long as it is
 * in the document. The shared Radix wrappers (`shared/ui/`) carry the
 * attribute on their content and overlay elements, which is one line each and
 * needs no import from this feature. Overlays that skip the wrappers are still
 * caught by the fallback selectors below (Radix's popper wrapper, dialog and
 * menu roles, toasts). Nothing here is community data and nothing is cached
 * at module level.
 */
export const NATIVE_VIEW_OCCLUDER_ATTR = "data-native-view-occluder";

/** What counts as an overlay: the explicit lease, then the fallbacks. */
export const NATIVE_VIEW_OCCLUDER_SELECTOR = [
  `[${NATIVE_VIEW_OCCLUDER_ATTR}]`,
  "[data-radix-popper-content-wrapper]",
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[role="menu"]',
  '[role="listbox"]',
  "[data-sonner-toast]",
].join(", ");

/** Marks the preview's own chrome, which never occludes its own slot. */
export const SESSION_PREVIEW_CHROME_ATTR = "data-session-preview-chrome";

export type PreviewOccluderCandidate = {
  rect: SessionPreviewRect;
  /**
   * The overlay hosts the slot (a sheet holding the panel), or is the
   * backdrop beneath that host: ignored.
   */
  containsSlot: boolean;
  /** Part of the preview's own floating chrome: ignored. */
  isPreviewChrome: boolean;
};

/** Strict overlap: rects that only touch along an edge do not intersect. */
export function previewRectsIntersect(
  left: SessionPreviewRect,
  right: SessionPreviewRect,
): boolean {
  if (left.width <= 0 || left.height <= 0) return false;
  if (right.width <= 0 || right.height <= 0) return false;
  return (
    left.x < right.x + right.width &&
    right.x < left.x + left.width &&
    left.y < right.y + right.height &&
    right.y < left.y + left.height
  );
}

/**
 * Whether the slot is occluded: any lease, other than one that contains the
 * slot or is the preview's own chrome, intersecting it. A lease not laid out
 * yet (zero size) does not count; the caller re-checks every frame while any
 * candidate is mounted, so it counts as soon as it has a box.
 */
export function previewSlotOccluded(
  slot: SessionPreviewRect | null,
  candidates: readonly PreviewOccluderCandidate[],
): boolean {
  if (slot === null) return false;
  return candidates.some(
    (candidate) =>
      !candidate.containsSlot &&
      !candidate.isPreviewChrome &&
      previewRectsIntersect(slot, candidate.rect),
  );
}

function domRect(element: Element): SessionPreviewRect {
  const box = element.getBoundingClientRect();
  return { x: box.left, y: box.top, width: box.width, height: box.height };
}

/** Lease value a modal's full-window backdrop carries (dialog, sheet). */
export const NATIVE_VIEW_OCCLUDER_BACKDROP = "backdrop";

const MODAL_SELECTOR = '[role="dialog"], [role="alertdialog"]';

/**
 * A backdrop drawn *beneath* the modal that hosts the slot (the narrow
 * layout puts the whole surface host in a sheet) does not cover the slot:
 * it precedes the slot's own modal in document order, so that modal paints
 * above it. A backdrop that follows it (a dialog opened over the sheet) does.
 */
function isBackdropBelowSlot(element: Element, slot: Element): boolean {
  if (
    element.getAttribute(NATIVE_VIEW_OCCLUDER_ATTR) !==
    NATIVE_VIEW_OCCLUDER_BACKDROP
  ) {
    return false;
  }
  const hostModal = slot.closest(MODAL_SELECTOR);
  if (hostModal === null) return false;
  return (
    (element.compareDocumentPosition(hostModal) &
      Node.DOCUMENT_POSITION_FOLLOWING) !==
    0
  );
}

/** Read every mounted overlay in `root` as a candidate (DOM side). */
export function readPreviewOccluders(
  root: ParentNode,
  slot: Element,
): PreviewOccluderCandidate[] {
  const seen = new Set<Element>();
  const candidates: PreviewOccluderCandidate[] = [];
  for (const element of root.querySelectorAll(NATIVE_VIEW_OCCLUDER_SELECTOR)) {
    if (seen.has(element)) continue;
    seen.add(element);
    candidates.push({
      rect: domRect(element),
      containsSlot:
        element.contains(slot) || isBackdropBelowSlot(element, slot),
      isPreviewChrome:
        element.closest(`[${SESSION_PREVIEW_CHROME_ATTR}]`) !== null,
    });
  }
  return candidates;
}
