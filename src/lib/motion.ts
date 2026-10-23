import { useReducedMotion, type Transition, type Variants } from "motion/react";

// Motion tokens (DESIGN.md, Motion). One authored feel: short, quiet, exponential ease-out.
// The CSS side lives under `/* motion */` in app/globals.css and uses the same numbers.
export const DUR = { fast: 0.15, base: 0.22, slow: 0.32, exit: 0.12 } as const;
export const EASE = [0.16, 1, 0.3, 1] as const;
// Sheets (Now Playing, queue). Damping ratio is about 0.95, so overshoot stays far below 2%.
export const SPRING: Transition = { type: "spring", stiffness: 320, damping: 34, mass: 1 };

export const tween = (duration: number = DUR.base): Transition => ({ duration, ease: EASE });

// Shared variants. MotionConfig reducedMotion="user" strips x/y/scale and keeps opacity.
export const fade: Variants = {
  hidden: { opacity: 0 },
  show: { opacity: 1, transition: tween(DUR.base) },
  exit: { opacity: 0, transition: tween(DUR.exit) },
};
export const fadeUp: Variants = {
  hidden: { opacity: 0, y: 8 },
  show: { opacity: 1, y: 0, transition: tween(DUR.base) },
  exit: { opacity: 0, transition: tween(DUR.exit) },
};
export const scaleIn: Variants = {
  hidden: { opacity: 0, scale: 0.98 },
  show: { opacity: 1, scale: 1, transition: tween(0.16) },
  exit: { opacity: 0, scale: 0.98, transition: tween(DUR.exit) },
};
// 30ms between children, capped by callers to the first 12 items.
export const stagger: Variants = {
  hidden: {},
  show: { transition: { staggerChildren: 0.03 } },
};
export const STAGGER_MAX = 12;

// Small helper for components: the reduced flag plus ready-made transitions.
export function useMotion() {
  const reduce = useReducedMotion() === true;
  return { reduce, DUR, EASE, SPRING, tween };
}
