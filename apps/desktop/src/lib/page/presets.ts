import { translate, type Translator } from "../i18n";
import type { PageBackground, PageGeometry } from "../model";

export type PresetChoice =
  | { kind: "none" }
  | { kind: "builtin"; id: string }
  | { kind: "imported"; name: string; source: string };

export type PresetSummary = {
  id: string;
  name: string;
  description: string;
  importPath: string | null;
  kind: "builtin" | "default" | "custom";
};

export type NotebookSetup = {
  name: string;
  geometry: PageGeometry;
  background: PageBackground;
  preset: PresetChoice;
};

export const DEFAULT_PRESET_PATH = "/styles/default.typ";
const MANAGED_HEADER = /^#import "(\/styles\/[A-Za-z0-9._-]+\.typ)": preset\r?\n#show: preset\.with\(rhythm: goodtype_rhythm\)(?:\r?\n){1,2}/;
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

export function pagePresetState(source: string): PagePresetState {
  const managed = MANAGED_HEADER.exec(source);
  if (managed) return { kind: "managed", path: managed[1] };
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
    else if (!/^[\t ]*(?:\r?\n|$|\/\/[^\r\n]*(?:\r?\n|$)|#import[\t ]+"[^"\\\r\n]+"(?:[\t ]*:[\t A-Za-z0-9_,*]+)?[\t ]*(?:\/\/[^\r\n]*)?(?:\r?\n|$))$/.test(line)) break;
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
