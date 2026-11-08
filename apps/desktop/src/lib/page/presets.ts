import { translate, type Translator } from "../i18n";
import type { PageBackground, PageGeometry } from "../model";

export type PresetChoice =
  | { kind: "none" }
  | { kind: "builtin"; id: string }
  | { kind: "imported"; name: string; source: string };

/** A structured Typst diagnostic as Rust reports it; `file` is set inside another file. */
export type PresetDiagnostic = {
  severity: "error" | "warning";
  message: string;
  line?: number;
  column?: number;
  file?: string;
};

/** Why a listed preset cannot be used. `detail` is the full producer text, shown on demand. */
export type PresetProblem = {
  code: "not_utf8" | "too_large" | "unreadable" | "invalid";
  detail: string;
};

export type PresetSummary = {
  id: string;
  name: string;
  description: string;
  importPath: string | null;
  kind: "builtin" | "default" | "custom";
  form?: "function" | "preamble";
  problem?: PresetProblem;
  /** Everything the representative-sample check reported, warnings included. */
  diagnostics?: PresetDiagnostic[];
};

/** Rust's notice after it wrote a preset file: no source text, only what changed. */
export type PresetChange = { root: string; path: string; revision: string };

/** The structured result of checking a preset against the shared representative sample. */
export type PresetCheck = {
  form: "function" | "preamble" | null;
  sampleRevision: string;
  rendered: boolean;
  widthPt: number | null;
  heightPt: number | null;
  diagnostics: PresetDiagnostic[];
};

export type NotebookSetup = {
  name: string;
  geometry: PageGeometry;
  background: PageBackground;
  preset: PresetChoice;
};

export const DEFAULT_PRESET_PATH = "/styles/default.typ";
// Mirrors `parse_managed_header` in goodtype-core: blank, `//` and single-line `/* */` lines may
// precede or separate the two directives; spaces/tabs, a trailing `//` comment and CRLF are fine.
const IGNORABLE_LINE = /^[\t ]*(?:\/\/[^\n]*|\/\*(?:(?!\*\/|\/\*)[^\n])*\*\/[\t ]*)?\r?$/;
const MANAGED_IMPORT_LINE = /^[\t ]*#import[\t ]+"(\/styles\/[A-Za-z0-9._-]+\.typ)"[\t ]*:[\t ]*preset[\t ]*(?:\/\/[^\n]*)?\r?$/;
const MANAGED_SHOW_LINE = /^[\t ]*#show[\t ]*:[\t ]*preset[\t ]*\.with[\t ]*\([\t ]*rhythm[\t ]*:[\t ]*goodtype_rhythm[\t ]*,?[\t ]*\)[\t ]*(?:\/\/[^\n]*)?\r?$/;
const PRESET_IMPORT_LINE = '^[\\t ]*#import[\\t ]+"([^"\\r\\n]+)"[\\t ]*:[\\t ]*preset[\\t ]*(?://[^\\r\\n]*)?(?:\\r?\\n|$)';
const PRESET_SHOW_LINE = '^[\\t ]*#show[\\t ]*:[\\t ]*preset(?:\\.with[\\t ]*\\([^\\r\\n]*\\))?[\\t ]*(?://[^\\r\\n]*)?(?:\\r?\\n|$)';

export type PagePresetState = {
  kind: "managed" | "custom" | "none";
  path: string | null;
};

export function presetHeader(path = DEFAULT_PRESET_PATH): string {
  return `#import "${path}": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n`;
}

export function pagePresetPath(source: string): string | null {
  return pagePresetState(source).path;
}

export function pagePresetOptions(presets: PresetSummary[]): PresetSummary[] {
  const installedBuiltins = new Set(
    presets
      .filter((preset) => preset.kind === "custom")
      .map((preset) => `${preset.importPath}\0${preset.name}`),
  );
  return presets.filter(
    (preset) =>
      preset.kind === "custom" ||
      (preset.kind === "builtin" &&
        !installedBuiltins.has(`/styles/${preset.id}.typ\0${preset.name}`)),
  );
}

/** The managed header's preset path, by the same rule Rust uses, or null. */
export function managedPresetPath(source: string): string | null {
  const lines = source.split("\n");
  let index = 0;
  const next = () => {
    while (index < lines.length && IGNORABLE_LINE.test(lines[index])) index += 1;
    return index < lines.length ? lines[index++] : null;
  };
  const importLine = next();
  const path = importLine === null ? null : MANAGED_IMPORT_LINE.exec(importLine)?.[1] ?? null;
  if (!path) return null;
  const showLine = next();
  return showLine !== null && MANAGED_SHOW_LINE.test(showLine) ? path : null;
}

/**
 * Whether Typst source may read a notebook file — an import, include or file-reading call — and
 * so must refresh when Rust reports a changed preset. Deliberately broad: a false positive costs
 * one recompile, a false negative a stale preview.
 */
export function readsNotebookFiles(source: string): boolean {
  return /#?\b(?:import|include)\b|\b(?:read|image|json|csv|yaml|toml|xml|cbor|plugin|bibliography)\s*\(/.test(source);
}

export function pagePresetState(source: string): PagePresetState {
  const managed = managedPresetPath(source);
  if (managed) return { kind: "managed", path: managed };
  const imported = new RegExp(PRESET_IMPORT_LINE, "m").exec(presetDirectives(source).join(""));
  const shown = new RegExp(PRESET_SHOW_LINE, "m").test(presetDirectives(source).join(""));
  return imported || shown
    ? { kind: "custom", path: imported?.[1] ?? null }
    : { kind: "none", path: null };
}

/**
 * A template's own name, from a `// Name: ...` comment among its first lines (the same rule the
 * Rust side lists templates by), or null when it has none.
 */
export function templateName(source: string): string | null {
  for (const line of source.split(/\r?\n/).slice(0, 5)) {
    const match = /^\s*\/\/\s*name\s*:\s*(.*?)\s*$/i.exec(line);
    if (match?.[1]) return match[1].slice(0, 60);
  }
  return null;
}

/** `preset` is Goodtype's reserved page-template binding; other imports and show rules stay. */
// Only the leading, single-line import header is owned by this picker. Stop before body syntax,
// including raw blocks, strings, nested code and block comments.
function presetDirectives(source: string): string[] {
  const directives: string[] = [];
  for (const line of source.match(/[^\n]*(?:\n|$)/g) ?? []) {
    if (new RegExp(PRESET_IMPORT_LINE).test(line) || new RegExp(PRESET_SHOW_LINE).test(line)) directives.push(line);
    else if (!/^[\t ]*(?:\r?\n|$|\/\/[^\r\n]*(?:\r?\n|$)|\/\*(?:(?!\*\/|\/\*)[^\r\n])*\*\/[\t ]*(?:\r?\n|$)|#import[\t ]+"[^"\\\r\n]+"(?:[\t ]*:[\t A-Za-z0-9_,*]+)?[\t ]*(?:\/\/[^\r\n]*)?(?:\r?\n|$))$/.test(line)) break;
  }
  return directives;
}

export function withPagePreset(source: string, path: string | null): string {
  const directives = presetDirectives(source);
  const owned = new Set(directives);
  let remaining = directives.length;
  const body = owned.size ? (source.match(/[^\n]*(?:\n|$)/g) ?? [])
    .filter(line => { if (remaining && owned.has(line)) { remaining -= 1; return false; } return true; }).join("").replace(/^(?:[\t ]*\r?\n)+/, "") : source;
  return path ? `${presetHeader(path)}${body}` : body;
}

/** Localize only application-owned presentation; names read from user style files stay intact. */
export function presetPresentation(preset: PresetSummary, t: Translator = translate): PresetSummary {
  if (preset.kind === "builtin") {
    const keys = {
      "clean-notes": ["presets.cleanNotes", "presets.cleanNotesHint"],
      "compact-stem": ["presets.compactStem", "presets.compactStemHint"],
      "formal-report": ["presets.formalReport", "presets.formalReportHint"],
    } as const;
    const pair = keys[preset.id as keyof typeof keys];
    if (pair) return { ...preset, name: t(pair[0]), description: t(pair[1]) };
  }
  return { ...preset, description: preset.kind === "default"
    ? t("editor.notebookDefault") : t("presets.notebookPreset") };
}

/** What a page preset change targets, captured before any await (P1). */
export type PresetTarget = { root: string; generation: number; pageId: string; pageTextId: string; source: string };

/**
 * Whether a pending preset change may still land: same notebook, generation, page, Page text and
 * unchanged source. Anything else aborts; the change is never redirected to another page.
 */
export function presetTargetCurrent(target: PresetTarget, live: {
  root: string | null; generation: number; pageId: string; pageText: { id: string; source: string } | null;
}): boolean {
  return live.root === target.root && live.generation === target.generation && live.pageId === target.pageId
    && live.pageText?.id === target.pageTextId && live.pageText.source === target.source;
}
