import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { useSmoothCorners } from "@/shared/ui/smoothCorners";

/**
 * Grid layout for a paragraph that resolved to two or more block images.
 *
 * Lifted out of `markdown.tsx` unchanged. That module sits well over the
 * repository's 1000-line ceiling, so the differential size gate forbids it
 * growing by even one line; the answer the repo mandates is to split the file,
 * and this component was its most self-contained piece.
 */
export function ImageMosaic({ children }: { children: React.ReactNode[] }) {
  const mosaicRef = React.useRef<HTMLDivElement | null>(null);
  const isTriptych = children.length === 3;
  const hasOddTail = children.length > 3 && children.length % 2 === 1;
  useSmoothCorners(mosaicRef);

  return (
    <div
      className={cn(
        // Columns carry a floor rather than `grid-cols-2`'s bare
        // `minmax(0, 1fr)`. The cells below are forced to `!w-full`, which
        // erases the intrinsic width their image frames would otherwise
        // contribute, so against a shrink-to-fit parent — a chat or
        // coding-session bubble sized to its content — every column resolves
        // to zero and the whole mosaic collapses to the width of its gaps. It
        // renders as a thin vertical sliver of background with the pictures
        // clipped out of existence: loaded, laid out, and invisible (observed
        // live, 2026-09-01, at 6px wide for a two-image prompt). The floor
        // gives max-content something real to report; `1fr` still fills the
        // width wherever the parent has one, so nothing changes when it does.
        "mt-1 grid w-full min-w-0 max-w-lg [grid-template-columns:repeat(2,minmax(6rem,1fr))] gap-1.5 overflow-hidden rounded-2xl [&_br]:hidden [&_[data-block-media]]:min-h-0 [&_[data-block-media]]:max-w-none [&_[data-block-media]]:overflow-hidden [&_[data-block-media]>button]:m-0 [&_[data-block-media]>button]:h-full [&_[data-block-media]>button]:w-full [&_[data-block-media]>button]:max-w-none [&_[data-block-media]>button]:rounded-none [&_[data-block-media]_[data-progressive-image-frame]]:!h-full [&_[data-block-media]_[data-progressive-image-frame]]:!w-full [&_[data-block-media]_img]:!h-full [&_[data-block-media]_img]:!max-h-none [&_[data-block-media]_img]:!w-full [&_[data-block-media]_img]:!max-w-none [&_[data-block-media]_img]:rounded-none [&_[data-block-media]_img]:object-cover",
        isTriptych
          ? "h-80 grid-rows-2 [&_[data-block-media]]:h-auto [&_[data-block-media]:first-child]:row-span-2"
          : "[&_[data-block-media]]:h-48",
        hasOddTail && "[&_[data-block-media]:last-child]:col-span-2",
      )}
      data-image-mosaic=""
      data-image-mosaic-count={children.length}
      ref={mosaicRef}
    >
      {children}
    </div>
  );
}
