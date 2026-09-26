/**
 * The Narrow mark: an ink square with a small square taken out of its
 * top-right corner. Never illustrated, never coloured: it is always ink on
 * the ground. The wordmark is lowercase "narrow"; in sentences the name is
 * Narrow, and the domain is narrow.fyi.
 */

// [corner radius, cutout size, cutout offset] per size, as drawn in the spec.
const GEOMETRY: Record<number, [number, number, number]> = {
  16: [4, 5, 3],
  18: [4, 6, 3],
  32: [7, 10, 6],
  36: [7, 11, 6],
  42: [9, 13, 7],
  64: [14, 19, 11],
};

function geometry(size: number): [number, number, number] {
  return GEOMETRY[size] ?? [Math.round(size * 0.22), Math.round(size * 0.31), Math.round(size * 0.17)];
}

/** The outline of the mark with the cutout as a hole (even-odd fill). */
function markPath(size: number): string {
  const [r, c, o] = geometry(size);
  const s = size;
  const outer = `M${r} 0H${s - r}A${r} ${r} 0 0 1 ${s} ${r}V${s - r}A${r} ${r} 0 0 1 ${s - r} ${s}H${r}A${r} ${r} 0 0 1 0 ${s - r}V${r}A${r} ${r} 0 0 1 ${r} 0Z`;
  const x = s - o - c;
  const inner = `M${x} ${o}H${x + c}V${o + c}H${x}Z`;
  return `${outer}${inner}`;
}

export function BrandMark({ size = 18, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      aria-hidden="true"
      focusable="false"
      className={`shrink-0 ${className}`}
    >
      <path d={markPath(size)} fill="var(--nr-mark)" fillRule="evenodd" />
    </svg>
  );
}

/** Mark and lowercase name, always ink. */
export function Wordmark({ size = 15, className = "" }: { size?: number; className?: string }) {
  const big = size >= 40;
  const mark = big ? Math.round(size * 0.875) : size <= 15 ? size + 1 : size + 2;
  const gap = big ? Math.round(size / 3) : Math.round(size * 0.6);
  return (
    <span
      className={`inline-flex items-center font-semibold leading-none whitespace-nowrap text-fg ${className}`}
      style={{ gap, fontSize: size, letterSpacing: big ? "-0.03em" : "-0.02em", fontStretch: big ? "96%" : undefined }}
    >
      <BrandMark size={mark} />
      narrow
    </span>
  );
}
