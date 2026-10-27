import { translate, type Translator } from "./i18n";
import { loadAppSettings, saveAppSettings } from "./ipc/workspace";
import type { PenSideButton, PressureCalibration } from "./ink/pipeline";
import type { StrokePattern } from "./model";
import type { HistoryCorner } from "./notebook/chromeLayout";
import type { PaletteCommand } from "./notebook/palette";
import type { ShapeKind } from "./shape/geometry";
import { DEFAULT_LASSO_TARGETS, type LassoTargets } from "./notebook/lasso";

/**
 * A pen slot on the palette. The two tiles are assignable slots, not fixed pens: pick a type
 * from the library below and tune it. `type` selects the nib behaviour; `pressure` decides
 * whether stylus force varies the width at all, which is the difference between a fountain pen
 * and a technical pen.
 */
export type PenPreset = {
  /** Maximum end taper in nib widths; absent older settings use three. */
  taperLengthWidths?: number;
  widthPt: number;
  color: string;
  type: PenTypeId;
  pressure: boolean;
  pattern: StrokePattern;
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
export function penTypes(t: Translator = translate): PenType[] { return [
  {
    id: "fountain",
    label: t("settings.fountain"),
    description: t("settings.pressureVaryingHeavilySmoothed"),
    pressure: true,
    smoothing: 0.35,
    taper: 0.12,
    widthPt: 1.6,
  },
  {
    id: "ballpoint",
    label: t("settings.ballpoint"),
    description: t("settings.evenLineLightSmoothing"),
    pressure: false,
    smoothing: 0.15,
    taper: 0,
    widthPt: 1.4,
  },
  {
    id: "pencil",
    label: t("settings.pencil"),
    description: t("settings.pressureVaryingMinimalSmoothing"),
    pressure: true,
    smoothing: 0.08,
    taper: 0.05,
    widthPt: 1.2,
  },
  {
    id: "marker",
    label: t("settings.marker"),
    description: t("settings.broadAndEven"),
    pressure: false,
    smoothing: 0.2,
    taper: 0,
    widthPt: 3.2,
  },
  {
    id: "technical",
    label: t("settings.technical"),
    description: t("settings.exactWidthNoPressureForDiagrams"),
    pressure: false,
    smoothing: 0.05,
    taper: 0,
    widthPt: 0.8,
  },
]; }
export const PEN_TYPES = penTypes();

export function penType(id: PenTypeId, t?: Translator): PenType {
  const types = t ? penTypes(t) : PEN_TYPES;
  return types.find((type) => type.id === id) ?? types[0];
}
export type UndoScope = "page" | "notebook";
export type PaletteDockSetting = "left" | "right" | "top" | "bottom";

export type EraserSize = "small" | "medium" | "large";
export type EraserReach = "everything" | "highlighter";
/** Whole marks, or only the ink under the eraser (ADR 0037). */
export type EraserMode = "stroke" | "precise";

export type AppSettings = {
  uiLanguage: "en" | "de";
  /** Resolved into ordinary Typst source when a new movable text block is created. */
  inlineTextFont: string;
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
  /** Line pattern per pen width chip, keyed by width; a width with no entry is solid. */
  penWidthPatterns: Record<string, StrokePattern>;
  /** Gap factor per pen width chip; a width with no entry uses 1. */
  penWidthSpacings: Record<string, number>;
  highlighterWidths: number[];
  /** Most-recently-used colours, newest first. */
  recentColors: string[];
  /** Whether stylus pressure varies stroke width at all. */
  pressureEnabled: boolean;
  /** Highlighter-only; pens stay opaque so ink stays legible over figures. */
  highlighterOpacity: number;
  highlighterStraighten: boolean;
  highlighterBehindInk: boolean;
  /** Whether highlighter strokes narrow at both ends. Off keeps a flat, full-width band. */
  highlighterTaper: boolean;
  /** Explicit shape tool preferences; canonical shapes carry their own resolved style. */
  shapeKind: ShapeKind;
  shapeFill: boolean;
  /** Line pattern for shapes drawn with the Shapes tool. Held figures follow the pen instead. */
  shapePattern: StrokePattern;
  shapePatternSpacing: number;
  shapeConstrain: boolean;
  drawAndHoldShapes: boolean;
  heldLineMinimumPt: number;
  /** Where the undo and redo buttons sit; `auto` keeps them clear of the palette on their own. */
  historyCorner: HistoryCorner;
  eraserSize: EraserSize;
  calibration: PressureCalibration;
  undoScope: UndoScope;
  paletteDock: PaletteDockSetting;
  /** Which side the full-height Typst source view opens on. */
  sideEditorDock: "left" | "right";
  /** Width of that view in CSS pixels. */
  sideEditorWidth: number;
  reducedMotion: boolean;
  /** Multiplier applied to one-finger pan velocity after release. */
  touchGlide: number;
  /** Keep Page text headings and display equations on whole paper rows. */
  pageTextBaselineGrid: boolean;
  /** Visually wrap long Page text source lines without inserting line breaks. */
  pageTextLineWrap: boolean;
  /** Allow downloading Typst Universe packages on a cache miss. */
  remotePackages: boolean;
  /** Scrolling on past the last page, against resistance, adds a page in the notebook paper. */
  pullToAddPage: boolean;
  /** One finger draws with the active tool; two fingers still pan and pinch. */
  drawWithFinger: boolean;
  touchHistory: boolean;
  lassoTargets: LassoTargets;
  penSideButton: PenSideButton;
  /** After an erase picked from the bar, go back to the tool used before it. */
  eraserReturns: boolean;
  eraserReach: EraserReach;
  eraserMode: EraserMode;
  /** Palette tiles taken off the bar. Their keyboard shortcuts keep working. */
  hiddenTools: PaletteCommand[];
};

export const DEFAULT_SETTINGS: AppSettings = {
  uiLanguage: "en",
  inlineTextFont: "",
  penPresets: [
    { widthPt: 1.6, color: "#1e232b", type: "fountain", pressure: true, pattern: "solid" },
    { widthPt: 2.8, color: "#2f6fdb", type: "ballpoint", pressure: false, pattern: "solid" },
  ],
  highlighter: {
    widthPt: 3.78,
    color: "#e0912b",
    type: "marker",
    pressure: false,
    pattern: "solid",
  },
  penSwatches: ["#1e232b", "#4c8df0", "#e5645e"],
  highlighterSwatches: ["#e0912b", "#e9d636", "#57c08a"],
  penWidths: [1.6, 2.8],
  penWidthPatterns: {},
  penWidthSpacings: {},
  highlighterWidths: [3.78, 5.2],
  recentColors: [],
  pressureEnabled: true,
  highlighterOpacity: 0.6,
  highlighterStraighten: false,
  highlighterBehindInk: true,
  highlighterTaper: false,
  shapeKind: "line",
  shapeFill: false,
  shapePattern: "solid",
  shapePatternSpacing: 1,
  shapeConstrain: false,
  drawAndHoldShapes: true,
  heldLineMinimumPt: 6,
  historyCorner: "auto",
  eraserSize: "medium",
  calibration: { minimum: 0, maximum: 1, curve: 1, smoothing: 0.2 },
  undoScope: "page",
  paletteDock: "bottom",
  sideEditorDock: "left",
  sideEditorWidth: 420,
  reducedMotion: false,
  touchGlide: 2,
  pageTextBaselineGrid: true,
  pageTextLineWrap: true,
  remotePackages: true,
  pullToAddPage: true,
  drawWithFinger: false,
  touchHistory: true,
  lassoTargets: { ...DEFAULT_LASSO_TARGETS },
  penSideButton: "eraser",
  eraserReturns: true,
  eraserReach: "everything",
  eraserMode: "stroke",
  hiddenTools: [],
};

/** End taper for a highlighter that has it switched on, as a fraction of stroke length. */
export const HIGHLIGHTER_TAPER = 0.12;

/** Eraser hit-area radius in points for each size. */
export const ERASER_RADIUS_PT: Record<EraserSize, number> = {
  small: 5,
  medium: 8,
  large: 14,
};

/**
 * Colours the bar carries per tool.
 *
 * Six, not twelve. The row is two dots wide in a vertical bar, so twelve made a six-row block
 * taller than the tools, the divider and the width row together — the bar stopped being a bar.
 * Nothing is lost by keeping it short: the colour panel holds the presets and the eight most
 * recent, so a colour that is not on the row is one tap away rather than gone.
 */
export const MAX_SWATCHES = 6;

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
export function colorPresets(t: Translator = translate): { hex: string; name: string }[] { return [
  { hex: "#1e232b", name: t("settings.graphite") },
  { hex: "#4c8df0", name: t("settings.blueprint") },
  { hex: "#e5645e", name: t("settings.oxide") },
  { hex: "#e0912b", name: t("settings.amber") },
  { hex: "#e9d636", name: t("settings.sulphur") },
  { hex: "#57c08a", name: t("settings.verdigris") },
  { hex: "#8a6fd4", name: t("settings.iris") },
  { hex: "#6a727c", name: t("settings.slate") },
]; }
export const COLOR_PRESETS = colorPresets();

/** Nib widths are stored in points and shown to the writer in millimetres. */
export const MM_PER_PT = 1 / 2.835;

/** A readable name for a colour, so assistive tech never has to spell out a hex code. */
export function colorName(hex: string, t: Translator = translate): string {
  const known = colorPresets(t).find(
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
    return await loadAppSettings();
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
  return await saveAppSettings(settings);
}

export type RecentNotebook = {
  root: string;
  title: string;
  pinned: boolean;
  lastOpened: string;
  lastPageId: string | null;
};

export type SearchHit = {
  pageId: string;
  pageNumber: number;
  objectId: string;
  excerpt: string;
};

export type RecoveryCandidate = {
  fileName: string;
  pageId: string | null;
  confirmedRevision: number | null;
  candidateRevision: number | null;
  error: string | null;
};
