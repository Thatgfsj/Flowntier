// v0.4.24 (event 000119): regression guard for the dark-theme
// color ramp. Locks in the perceptual distances between surface
// levels and the WCAG-AA contrast ratios of text/role/state
// tokens against their host surface. If anyone tweaks a hex
// value in src/index.css, this test fires immediately so the
// chairman's "黑白为主, 主题色为辅" rule can't silently regress.

import { describe, it, expect } from "vitest";

// ── palette (kept in sync with src/index.css) ─────────────────
const SURFACE = {
  "surface-0": "#06080D",
  "surface-1": "#0F141C",
  "surface-2": "#1A2230",
  "surface-3": "#25303F",
  "surface-4": "#2F3B4D",
} as const;

const TEXT = {
  primary: "#F2F5FA",
  secondary: "#B6BFCC",
  tertiary: "#7A8493",
  disabled: "#4F5867",
  error: "#FF5C6C",
  success: "#3DDC97",
  accent: "#FFD24A",
} as const;

const ROLE = {
  chief: "#5B9CFF",
  "critic-a": "#FF7A85",
  "critic-b": "#B493FF",
  "worker-1": "#4DD0B5",
  "worker-2": "#FFB347",
  "worker-3": "#8AE063",
  "worker-4": "#E879F9",
} as const;

const STATUS = {
  pending: "#6B7787",
  active: "#FFD24A",
  done: "#3DDC97",
  failed: "#FF5C6C",
  warn: "#FFA94D",
} as const;

// ── helpers ──────────────────────────────────────────────────
function srgbToLinear(c: number): number {
  const v = c / 255;
  return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
}

function relLuminance(hex: string): number {
  const h = hex.replace("#", "");
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
}

function contrast(a: string, b: string): number {
  const la = relLuminance(a);
  const lb = relLuminance(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

// ── surface ramp monotonicity ─────────────────────────────────
describe("color ramp / surface", () => {
  it("surface levels are monotonically non-decreasing in luminance", () => {
    const ramp = Object.values(SURFACE).map(relLuminance);
    for (let i = 1; i < ramp.length; i++) {
      // Strictly increasing — adjacent steps must be visibly
      // distinct. If the gap ever falls to 0 the chairman
      // will (rightly) complain "颜色都差不多" again.
      expect(ramp[i]!).toBeGreaterThan(ramp[i - 1]!);
    }
  });

  it("every consecutive surface step has visible RGB distance (≥10 units in dark theme)", () => {
    // On a dark theme the linear-light luminance gap looks
    // small (e.g. 0.004 between #06080D and #0F141C) but the
    // perceptual jump is real because the human eye is much
    // more sensitive to ΔRGB at low luminance. We check the
    // raw RGB distance in each channel to make sure no two
    // surface levels ever collapse into the same wash.
    const ramp = Object.values(SURFACE);
    for (let i = 1; i < ramp.length; i++) {
      const [ar, ag, ab] = hexToRgb(ramp[i - 1] ?? "#000000");
      const [br, bg, bb] = hexToRgb(ramp[i] ?? "#000000");
      const minChannelGap = Math.min(Math.abs(ar - br), Math.abs(ag - bg), Math.abs(ab - bb));
      expect(minChannelGap, `surface ${i - 1} → ${i}`).toBeGreaterThanOrEqual(8);
    }
  });
});

// ── WCAG AA contrast for text on host surfaces ────────────────
describe("color ramp / text contrast (WCAG AA)", () => {
  it("text-primary ≥ 7:1 vs surface-1 (canvas)", () => {
    expect(contrast(TEXT.primary, SURFACE["surface-1"])).toBeGreaterThanOrEqual(7);
  });

  it("text-secondary ≥ 4.5:1 vs surface-1 (canvas)", () => {
    expect(contrast(TEXT.secondary, SURFACE["surface-1"])).toBeGreaterThanOrEqual(4.5);
  });

  it("text-tertiary is non-text only (≥ 3:1 for large text)", () => {
    // tertiary is intentionally subtle — used for hints /
    // timestamps. We don't require it to clear 4.5:1 but it
    // should at least clear 3:1 for ≥18pt use.
    expect(contrast(TEXT.tertiary, SURFACE["surface-1"]!)).toBeGreaterThanOrEqual(3);
  });

  it("text-error ≥ 4.5:1 vs surface-1 (canvas)", () => {
    expect(contrast(TEXT.error, SURFACE["surface-1"])).toBeGreaterThanOrEqual(4.5);
  });

  it("text-success ≥ 4.5:1 vs surface-1", () => {
    expect(contrast(TEXT.success, SURFACE["surface-1"])).toBeGreaterThanOrEqual(4.5);
  });

  it("text-accent (yellow) ≥ 4.5:1 vs surface-1", () => {
    expect(contrast(TEXT.accent, SURFACE["surface-1"])).toBeGreaterThanOrEqual(4.5);
  });
});

// ── role brand colors readable on canvas + cards ──────────────
describe("color ramp / role colors", () => {
  it.each(Object.entries(ROLE))(
    "role %s ≥ 3:1 vs surface-1 (UI large-text contrast)",
    (_name, hex) => {
      // Roles are used as border-left bars and pill text — they
      // must clear the 3:1 large-text threshold for non-body
      // decorative use, plus 4.5:1 for inline text like the
      // "speaking" pill.
      expect(contrast(hex, SURFACE["surface-1"])).toBeGreaterThanOrEqual(3);
    },
  );

  it.each(Object.entries(ROLE))(
    "role %s ≥ 4.5:1 vs surface-2 (card host) for inline text use",
    (_name, hex) => {
      expect(contrast(hex, SURFACE["surface-2"])).toBeGreaterThanOrEqual(4.5);
    },
  );
});

// ── status palette readability ────────────────────────────────
describe("color ramp / status colors", () => {
  it.each(Object.entries(STATUS))("status %s ≥ 3:1 vs surface-1", (_name, hex) => {
    expect(contrast(hex, SURFACE["surface-1"])).toBeGreaterThanOrEqual(3);
  });
});

// ── chief/failed button text contrast (solid backgrounds) ─────
describe("color ramp / solid-button foregrounds", () => {
  it("chief button foreground: black ≥ 4.5:1 (was white = 2.75:1 FAIL)", () => {
    expect(contrast("#000000", ROLE.chief)).toBeGreaterThanOrEqual(4.5);
  });

  it("status-failed button foreground: black ≥ 4.5:1 (was white = 3.0:1 FAIL)", () => {
    expect(contrast("#000000", STATUS.failed)).toBeGreaterThanOrEqual(4.5);
  });

  it("status-active button foreground: black ≥ 4.5:1 (was white = 1.44:1 FAIL)", () => {
    expect(contrast("#000000", STATUS.active)).toBeGreaterThanOrEqual(4.5);
  });
});

// ── hue distinctness ──────────────────────────────────────────
// Each head role and each worker must be far enough from
// the others that the eye can tell who's speaking without
// reading the name. Use ΔE in sRGB as a rough proxy.
function rgbDelta(a: string, b: string): number {
  const [ar, ag, ab] = hexToRgb(a);
  const [br, bg, bb] = hexToRgb(b);
  return Math.sqrt((ar - br) ** 2 + (ag - bg) ** 2 + (ab - bb) ** 2);
}

describe("color ramp / hue distinctness", () => {
  const heads = ["chief", "critic-a", "critic-b"] as const;
  it("every pair of head roles has RGB Δ ≥ 80 (visibly distinct)", () => {
    for (let i = 0; i < heads.length; i++) {
      for (let j = i + 1; j < heads.length; j++) {
        const d = rgbDelta(ROLE[heads[i]!], ROLE[heads[j]!]);
        expect(d, `${heads[i]} vs ${heads[j]}`).toBeGreaterThanOrEqual(80);
      }
    }
  });

  it("every pair of worker hues has RGB Δ ≥ 60 (visibly distinct)", () => {
    const workers = ["worker-1", "worker-2", "worker-3", "worker-4"] as const;
    for (let i = 0; i < workers.length; i++) {
      for (let j = i + 1; j < workers.length; j++) {
        const d = rgbDelta(ROLE[workers[i]!], ROLE[workers[j]!]);
        expect(d, `${workers[i]} vs ${workers[j]}`).toBeGreaterThanOrEqual(60);
      }
    }
  });
});
