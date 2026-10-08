/**
 * The floating mini-player's box: where a drag or a resize leaves it, kept
 * inside the window and never smaller than a usable page. Pure, so the unit
 * test pins the clamping.
 */
export type PreviewFloatBox = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export const PREVIEW_FLOAT_MIN = { width: 320, height: 220 } as const;
const MARGIN = 8;

export function defaultPreviewFloatBox(viewport: {
  width: number;
  height: number;
}): PreviewFloatBox {
  const width = Math.min(
    480,
    Math.max(PREVIEW_FLOAT_MIN.width, viewport.width * 0.4),
  );
  const height = Math.min(
    360,
    Math.max(PREVIEW_FLOAT_MIN.height, viewport.height * 0.45),
  );
  return clampPreviewFloatBox(
    {
      x: viewport.width - width - 24,
      y: viewport.height - height - 96,
      width,
      height,
    },
    viewport,
  );
}

export function clampPreviewFloatBox(
  box: PreviewFloatBox,
  viewport: { width: number; height: number },
): PreviewFloatBox {
  const width = Math.max(
    PREVIEW_FLOAT_MIN.width,
    Math.min(box.width, viewport.width - MARGIN * 2),
  );
  const height = Math.max(
    PREVIEW_FLOAT_MIN.height,
    Math.min(box.height, viewport.height - MARGIN * 2),
  );
  const x = Math.min(Math.max(MARGIN, box.x), viewport.width - width - MARGIN);
  const y = Math.min(
    Math.max(MARGIN, box.y),
    viewport.height - height - MARGIN,
  );
  return {
    x: Math.round(x),
    y: Math.round(y),
    width: Math.round(width),
    height: Math.round(height),
  };
}

/** Apply a pointer delta to the box a drag (`move`) or corner resize started from. */
export function previewFloatDrag(
  start: PreviewFloatBox,
  mode: "move" | "resize",
  dx: number,
  dy: number,
  viewport: { width: number; height: number },
): PreviewFloatBox {
  return clampPreviewFloatBox(
    mode === "move"
      ? { ...start, x: start.x + dx, y: start.y + dy }
      : { ...start, width: start.width + dx, height: start.height + dy },
    viewport,
  );
}
