import { cx } from "./ui";
import type { Tone } from "../lib/media";

// Placeholder artwork drawn in code from a seed (album id or title). No image URLs, no network, no inline style:
// every colour is an SVG attribute. Six compositions, each with its own palette jitter, so no two tiles match.

const HUE: Record<Tone, number> = { slate: 222, violet: 278, amber: 36, rose: 352, moss: 142, ink: 240 };

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  h ^= h >>> 16;
  h = Math.imul(h, 0x45d9f3b);
  h ^= h >>> 16;
  return h >>> 0;
}

function rng(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const hsl = (h: number, s: number, l: number) => `hsl(${Math.round(((h % 360) + 360) % 360)} ${Math.round(s)}% ${Math.round(l)}%)`;

// Ambient glows, inside the Clamped Glow Rule: OKLCH L <= 0.25, C <= 0.10.
export const AMBIENT_BG: Record<Tone, string> = {
  slate: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.24_0.05_250),transparent_70%)]",
  violet: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.23_0.07_290),transparent_70%)]",
  amber: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.24_0.06_70),transparent_70%)]",
  rose: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.23_0.07_15),transparent_70%)]",
  moss: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.24_0.05_150),transparent_70%)]",
  ink: "bg-[radial-gradient(60%_60%_at_30%_30%,oklch(0.2_0.02_260),transparent_70%)]",
};

// Bottom scrim for text laid over artwork, tinted from the tile's tone.
export const SCRIM_BG: Record<Tone, string> = {
  slate: "bg-[linear-gradient(to_top,oklch(0.17_0.05_250/0.94)_0%,oklch(0.17_0.05_250/0.55)_38%,transparent_72%)]",
  violet: "bg-[linear-gradient(to_top,oklch(0.17_0.07_290/0.94)_0%,oklch(0.17_0.07_290/0.55)_38%,transparent_72%)]",
  amber: "bg-[linear-gradient(to_top,oklch(0.17_0.06_60/0.94)_0%,oklch(0.17_0.06_60/0.55)_38%,transparent_72%)]",
  rose: "bg-[linear-gradient(to_top,oklch(0.17_0.07_10/0.94)_0%,oklch(0.17_0.07_10/0.55)_38%,transparent_72%)]",
  moss: "bg-[linear-gradient(to_top,oklch(0.17_0.05_150/0.94)_0%,oklch(0.17_0.05_150/0.55)_38%,transparent_72%)]",
  ink: "bg-[linear-gradient(to_top,oklch(0.15_0.02_260/0.94)_0%,oklch(0.15_0.02_260/0.55)_38%,transparent_72%)]",
};

function Composition({ seed, tone }: { seed: string; tone: Tone }) {
  const code = hash(`lx50${seed}`);
  const kind = code % 6;
  const r = rng(hash(seed));
  const neutral = tone === "ink";
  const h0 = HUE[tone] + (r() - 0.5) * 44;
  const h1 = h0 + 16 + r() * 26;
  const s = neutral ? 9 : 34 + r() * 20;
  const light = hsl(h1, s, 68);
  const warm = hsl(h1 + 24, s + 8, 80);
  const mid = hsl(h0, s, 46);
  const deep = hsl(h0 - 8, s * 0.85, 24);
  const night = hsl(h0 - 14, s * 0.6, 9);
  const id = `art${code.toString(36)}`;
  const f = (n: number) => Math.round(n * 10) / 10;

  const defs = (
    <>
      <linearGradient id={`${id}a`} x1="0" y1="0" x2="0.4" y2="1">
        <stop offset="0" stopColor={light} />
        <stop offset="1" stopColor={mid} />
      </linearGradient>
      <linearGradient id={`${id}b`} x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor={deep} />
        <stop offset="1" stopColor={night} />
      </linearGradient>
      <linearGradient id={`${id}c`} x1="1" y1="0" x2="0" y2="1">
        <stop offset="0" stopColor={mid} />
        <stop offset="1" stopColor={night} />
      </linearGradient>
      <filter id={`${id}f`} x="-40%" y="-40%" width="180%" height="180%">
        <feGaussianBlur stdDeviation="11" />
      </filter>
    </>
  );

  if (kind === 0) {
    // Horizon: a single low sun behind three layered hills.
    const sunX = 22 + r() * 56;
    const y1 = 56 + r() * 8;
    const y2 = 70 + r() * 6;
    const y3 = 83 + r() * 4;
    return (
      <>
        <defs>{defs}</defs>
        <rect width="100" height="100" fill={`url(#${id}a)`} />
        <circle cx={f(sunX)} cy={f(y1 - 4)} r={f(11 + r() * 8)} fill={warm} />
        <path d={`M0 ${f(y1)} C 24 ${f(y1 - 10)}, 52 ${f(y1 + 6)}, 100 ${f(y1 - 6)} V100 H0Z`} fill={mid} />
        <path d={`M0 ${f(y2)} C 30 ${f(y2 - 12)}, 60 ${f(y2 + 8)}, 100 ${f(y2 - 5)} V100 H0Z`} fill={deep} />
        <path d={`M0 ${f(y3)} C 28 ${f(y3 - 8)}, 64 ${f(y3 + 5)}, 100 ${f(y3 - 4)} V100 H0Z`} fill={night} />
      </>
    );
  }

  if (kind === 1) {
    // Arches: three rounded-top columns at different heights.
    const fills = [light, mid, warm];
    return (
      <>
        <defs>{defs}</defs>
        <rect width="100" height="100" fill={`url(#${id}b)`} />
        {[0, 1, 2].map((i) => {
          const w = 25;
          const x = 7 + i * 29;
          const top = 18 + r() * 34;
          return (
            <path
              key={i}
              d={`M${x} 100 V${f(top + w / 2)} A${w / 2} ${w / 2} 0 0 1 ${x + w} ${f(top + w / 2)} V100Z`}
              fill={fills[(i + (code % 3)) % 3]}
              opacity={0.92 - i * 0.1}
            />
          );
        })}
      </>
    );
  }

  if (kind === 2) {
    // Slabs: overlapping tilted rectangles.
    const tilt = -30 + r() * 60;
    const fills = [light, warm, mid, deep];
    return (
      <>
        <defs>{defs}</defs>
        <rect width="100" height="100" fill={`url(#${id}c)`} />
        <g transform={`rotate(${f(tilt)} 50 50)`}>
          {fills.map((c, i) => (
            <rect
              key={i}
              x={f(-10 + r() * 50)}
              y={f(-5 + i * 24 + r() * 8)}
              width={f(60 + r() * 50)}
              height={f(16 + r() * 18)}
              rx="3"
              fill={c}
              opacity={0.55 + r() * 0.35}
            />
          ))}
        </g>
      </>
    );
  }

  if (kind === 3) {
    // Soft blobs: blurred colour fields, no hard shapes.
    return (
      <>
        <defs>{defs}</defs>
        <rect width="100" height="100" fill={night} />
        <g filter={`url(#${id}f)`}>
          <ellipse cx={f(25 + r() * 30)} cy={f(25 + r() * 25)} rx={f(28 + r() * 12)} ry={f(22 + r() * 12)} fill={light} />
          <ellipse cx={f(60 + r() * 30)} cy={f(55 + r() * 25)} rx={f(30 + r() * 12)} ry={f(26 + r() * 10)} fill={mid} />
          <ellipse cx={f(20 + r() * 50)} cy={f(82 + r() * 10)} rx={f(34 + r() * 10)} ry={f(16 + r() * 8)} fill={warm} opacity="0.85" />
        </g>
      </>
    );
  }

  if (kind === 4) {
    // Waves: five stacked ribbons darkening toward the bottom.
    const fills = [warm, light, mid, deep, night];
    return (
      <>
        <defs>{defs}</defs>
        <rect width="100" height="100" fill={`url(#${id}a)`} />
        {fills.map((c, i) => {
          const y = 26 + i * 16;
          const a = 5 + r() * 9;
          return <path key={i} d={`M0 ${f(y)} C 26 ${f(y - a)}, 54 ${f(y + a)}, 100 ${f(y - a / 2)} V100 H0Z`} fill={c} />;
        })}
      </>
    );
  }

  // Quarter tiles: a 2x2 grid of geometric cells.
  const cells = [0, 1, 2, 3].map((i) => {
    const x = (i % 2) * 50;
    const y = Math.floor(i / 2) * 50;
    const rot = Math.floor(r() * 4) * 90;
    const shape = Math.floor(r() * 3);
    const fill = [light, warm, mid, deep][Math.floor(r() * 4)];
    const d =
      shape === 0
        ? "M0 50 V0 A50 50 0 0 1 50 50Z"
        : shape === 1
          ? "M0 25 A25 25 0 0 1 50 25 V50 H0Z"
          : "M0 0 H50 V25 H0Z";
    return (
      <g key={i} transform={`translate(${x} ${y})`}>
        <rect width="50" height="50" fill={i % 2 === 0 ? deep : night} opacity="0.7" />
        <path d={d} fill={fill} transform={`rotate(${rot} 25 25)`} />
      </g>
    );
  });
  return (
    <>
      <defs>{defs}</defs>
      <rect width="100" height="100" fill={night} />
      {cells}
    </>
  );
}

export function Art({
  seed,
  tone,
  className,
  rounded = "rounded-[8px]",
  grain = true,
}: {
  seed: string;
  tone: Tone;
  className?: string;
  rounded?: string;
  grain?: boolean;
}) {
  const nid = `n${hash(seed).toString(36)}`;
  return (
    <div aria-hidden="true" className={cx(!/\babsolute\b/.test(className ?? "") && "relative", "isolate overflow-hidden bg-surface", rounded, className)}>
      <svg viewBox="0 0 100 100" preserveAspectRatio="xMidYMid slice" className="absolute inset-0 size-full">
        <Composition seed={seed} tone={tone} />
        {grain && (
          <>
            <filter id={nid} x="0" y="0" width="100%" height="100%">
              <feTurbulence type="fractalNoise" baseFrequency="1.6" numOctaves="2" stitchTiles="stitch" />
              <feColorMatrix type="saturate" values="0" />
            </filter>
            <rect width="100" height="100" filter={`url(#${nid})`} opacity="0.16" />
          </>
        )}
      </svg>
    </div>
  );
}
