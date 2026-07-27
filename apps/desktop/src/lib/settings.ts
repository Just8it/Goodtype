import { invoke } from "@tauri-apps/api/core";
import type { PressureCalibration } from "./ink/pipeline";

/**
 * A pen slot on the palette. The two tiles are assignable slots, not fixed pens: pick a type
 * from the library below and tune it. `type` selects the nib behaviour; `pressure` decides
 * whether stylus force varies the width at all, which is the difference between a fountain pen
 * and a technical pen.
 */
export type PenPreset = {
  widthPt: number;
  color: string;
  type: PenTypeId;
  pressure: boolean;
};

export type PenTypeId = "fountain" | "ballpoint" | "pencil" | "marker" | "technical";

export type PenType = {
  id: PenTypeId;
  label: string;
  description: string;
  /** Whether this nib normally responds to stylus pressure. */
  pressure: boolean;
  /** Default smoothing for the nib, 0–1. A pencil wants less than a fountain pen. */
  smoothing: number;
  /**
   * Fraction of the stroke's length over which each end narrows to a point, 0–1. This is what
   * separates nibs that lift cleanly off the page (a fountain pen) from ones that stop dead
   * (a technical pen); it is a property of the nib, not something a slot tunes.
   */
  taper: number;
  widthPt: number;
};

/** The pen library. Slots start from one of these, then diverge as the writer tunes them. */
export const PEN_TYPES: PenType[] = [
  {
    id: "fountain",
    label: "Fountain",
    description: "Pressure-varying, heavily smoothed",
    pressure: true,
    smoothing: 0.35,
    taper: 0.12,
    widthPt: 1.6,
  },
  {
    id: "ballpoint",
    label: "Ballpoint",
    description: "Even line, light smoothing",
    pressure: false,
    smoothing: 0.15,
    taper: 0,
    widthPt: 1.4,
  },
  {
    id: "pencil",
    label: "Pencil",
    description: "Pressure-varying, minimal smoothing",
    pressure: true,
    smoothing: 0.08,
    taper: 0.05,
    widthPt: 1.2,
  },
  {
    id: "marker",
    label: "Marker",
    description: "Broad and even",
    pressure: false,
    smoothing: 0.2,
    taper: 0,
    widthPt: 3.2,
  },
  {
    id: "technical",
    label: "Technical",
    description: "Exact width, no pressure — for diagrams",
    pressure: false,
    smoothing: 0.05,
    taper: 0,
    widthPt: 0.8,
  },
];

export function penType(id: PenTypeId): PenType {
  return PEN_TYPES.find((type) => type.id === id) ?? PEN_TYPES[0];
}
export type UndoScope = "page" | "notebook";
export type PaletteDockSetting = "left" | "right" | "top" | "bottom";

export type EraserSize = "small" | "medium" | "large";

export type AppSettings = {
  penPresets: PenPreset[];
  highlighter: PenPreset;
  /**
   * The swatch rows the writer curates. A pen preset's `color` is only *which* colour that pen
   * currently uses; keeping the two apart is what lets a swatch be added or edited without
   * silently retargeting a pen — and what lets a custom colour actually appear on the bar.
   */
  penSwatches: string[];
  highlighterSwatches: string[];
  /** Width chips offered per tool, in points. */
  penWidths: number[];
  highlighterWidths: number[];
  /** Most-recently-used colours, newest first. */
  recentColors: string[];
  /** Whether stylus pressure varies stroke width at all. */
  pressureEnabled: boolean;
  /** Highlighter-only; pens stay opaque so ink stays legible over figures. */
  highlighterOpacity: number;
  highlighterStraighten: boolean;
  highlighterBehindInk: boolean;
  eraserSize: EraserSize;
  calibration: PressureCalibration;
  undoScope: UndoScope;
  paletteDock: PaletteDockSetting;
  /** Which side the full-height Typst source view opens on. */
  sideEditorDock: "left" | "right";
  /** Width of that view in CSS pixels. */
  sideEditorWidth: number;
  reducedMotion: boolean;
  /** Allow downloading Typst Universe packages on a cache miss. */
  remotePackages: boolean;
};

export const DEFAULT_SETTINGS: AppSettings = {
  penPresets: [
    { widthPt: 1.6, color: "#1e232b", type: "fountain", pressure: true },
    { widthPt: 2.8, color: "#2f6fdb", type: "ballpoint", pressure: false },
  ],
  highlighter: { widthPt: 3.78, color: "#e0912b", type: "marker", pressure: false },
  penSwatches: ["#1e232b", "#4c8df0", "#e5645e"],
  highlighterSwatches: ["#e0912b", "#e9d636", "#57c08a"],
  penWidths: [1, 1.6, 2.8],
  highlighterWidths: [2.6, 3.78, 5.2],
  recentColors: [],
  pressureEnabled: true,
  highlighterOpacity: 0.6,
  highlighterStraighten: false,
  highlighterBehindInk: true,
  eraserSize: "medium",
  calibration: { minimum: 0, maximum: 1, curve: 1, smoothing: 0.2 },
  undoScope: "page",
  paletteDock: "bottom",
  sideEditorDock: "left",
  sideEditorWidth: 420,
  reducedMotion: false,
  remotePackages: true,
};

/** Eraser hit-area radius in points for each size. */
export const ERASER_RADIUS_PT: Record<EraserSize, number> = {
  small: 5,
  medium: 8,
  large: 14,
};

export const MAX_SWATCHES = 12;

/**
 * How many stroke widths a tool's row holds.
 *
 * Far fewer than colours: the row is a ladder read at a glance, and the tiles are told apart
 * only by the thickness of the line drawn on them. Four steps are distinguishable at a glance;
 * more of them turn a ladder into a list of numbers, which the panel already does better.
 */
export const MAX_WIDTHS = 4;
export const MAX_RECENT_COLORS = 8;

/** Colours offered in the colour panel before the writer has curated any. */
export const COLOR_PRESETS: { hex: string; name: string }[] = [
  { hex: "#1e232b", name: "Graphite" },
  { hex: "#4c8df0", name: "Blueprint" },
  { hex: "#e5645e", name: "Oxide" },
  { hex: "#e0912b", name: "Amber" },
  { hex: "#e9d636", name: "Sulphur" },
  { hex: "#57c08a", name: "Verdigris" },
  { hex: "#8a6fd4", name: "Iris" },
  { hex: "#6a727c", name: "Slate" },
];

/** A readable name for a colour, so assistive tech never has to spell out a hex code. */
export function colorName(hex: string): string {
  const known = COLOR_PRESETS.find(
    (preset) => preset.hex.toLowerCase() === hex.toLowerCase(),
  );
  return known ? known.name : hex.toUpperCase();
}

const HEX_PATTERN = /^#[0-9a-f]{6}$/i;

/**
 * Only `#rrggbb` is accepted. Alpha never rides inside a colour string: highlighter
 * translucency is its own opacity setting, so a stricter reader can never silently drop it.
 */
export function isValidColor(hex: string): boolean {
  return HEX_PATTERN.test(hex);
}

/** Normalises loose input ("1e232b", "#1E232B", "#abc") to `#rrggbb`, or null if unusable. */
export function normalizeColor(input: string): string | null {
  const raw = input.trim().replace(/^#/, "").toLowerCase();
  const expanded =
    raw.length === 3
      ? raw
          .split("")
          .map((character) => character + character)
          .join("")
      : raw;
  const candidate = `#${expanded}`;
  return isValidColor(candidate) ? candidate : null;
}

/** Adds a colour to the front of the recent list, de-duplicated and capped. */
export function withRecentColor(recent: string[], color: string): string[] {
  const normalized = normalizeColor(color);
  if (!normalized) return recent;
  return [normalized, ...recent.filter((item) => item.toLowerCase() !== normalized)].slice(
    0,
    MAX_RECENT_COLORS,
  );
}

export async function loadSettings(tauriAvailable: boolean): Promise<AppSettings> {
  if (!tauriAvailable) return structuredClone(DEFAULT_SETTINGS);
  try {
    return await invoke<AppSettings>("load_app_settings");
  } catch {
    return structuredClone(DEFAULT_SETTINGS);
  }
}

/** Rust clamps every field to its documented bounds and returns the sanitized value. */
export async function saveSettings(
  tauriAvailable: boolean,
  settings: AppSettings,
): Promise<AppSettings> {
  if (!tauriAvailable) return settings;
  return await invoke<AppSettings>("save_app_settings", { settings });
}

export type RecentNotebook = {
  root: string;
  title: string;
  pinned: boolean;
  lastOpened: string;
};

export type SearchHit = {
  pageId: string;
  pageNumber: number;
  objectId: string;
  excerpt: string;
};

export type RecoveryCandidate = {
  fileName: string;
  pageId: string;
  confirmedRevision: number;
  candidateRevision: number;
};
