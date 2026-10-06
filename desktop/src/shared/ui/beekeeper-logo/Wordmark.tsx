import { useId } from "react";

export type WordmarkProps = {
  className?: string;
  /** The word(s) to set. Defaults to the product name. */
  text?: string;
};

/**
 * The product wordmark, set as live SVG text rather than a bitmap.
 *
 * This replaced a 332 KB `buzz-wordmark.png`. Text is the better primitive
 * here: it re-renders crisply at any zoom, inherits `currentColor` so it works
 * in both themes without a second asset, and a rename is a string edit rather
 * than a trip through a design tool.
 *
 * The grain filter is the same construction the animated bee mark uses
 * (`BeekeeperLogoAnimation`) — blur, then displace by fractal noise, then composite
 * the noise back as grain — minus the `<animate>` children, because the
 * wordmark sits still. Kept in sync by eye, not by code: they are deliberately
 * separate so the mark can animate without dragging the wordmark with it.
 *
 * The viewBox and the `userSpaceOnUse` filter region are sized for the longest
 * string this is expected to carry, with margin for the blur and displacement
 * to bleed into. A longer `text` will clip at the right edge rather than
 * scale — which is exactly what happened when "Buzz" became "Beekeeper".
 */
export default function Wordmark({
  className,
  text = "Beekeeper",
}: WordmarkProps) {
  const filterId = useId();
  return (
    <svg
      aria-label={text}
      className={className}
      role="img"
      viewBox="0 0 1400 300"
      xmlns="http://www.w3.org/2000/svg"
    >
      <title>{text}</title>
      <filter
        colorInterpolationFilters="sRGB"
        height="460"
        id={filterId}
        width="1560"
        x="-80"
        y="-80"
      >
        <feGaussianBlur in="SourceGraphic" result="soft" stdDeviation="7" />
        <feTurbulence
          baseFrequency="1.06"
          numOctaves="5"
          result="noise"
          seed="7"
          type="fractalNoise"
        />
        <feDisplacementMap
          in="soft"
          in2="noise"
          result="displaced"
          scale="8"
          xChannelSelector="R"
          yChannelSelector="G"
        />
        <feColorMatrix
          in="noise"
          result="grainAlpha"
          type="matrix"
          values="0 0 0 0 0
                  0 0 0 0 0
                  0 0 0 0 0
                  .5 .5 .5 0 0"
        />
        <feComposite
          in="displaced"
          in2="grainAlpha"
          operator="in"
          result="grain"
        />
        <feMerge>
          <feMergeNode in="displaced" />
          <feMergeNode in="grain" />
        </feMerge>
      </filter>
      <text
        dominantBaseline="middle"
        fill="currentColor"
        fontFamily="inherit"
        fontSize="200"
        fontWeight="800"
        filter={`url(#${filterId})`}
        letterSpacing="-6"
        textAnchor="middle"
        x="700"
        y="160"
      >
        {text}
      </text>
    </svg>
  );
}
