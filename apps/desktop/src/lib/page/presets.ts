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
  const imported = new RegExp(PRESET_IMPORT_LINE, "m").exec(source);
  const shown = new RegExp(PRESET_SHOW_LINE, "m").test(source);
  return imported || shown
    ? { kind: "custom", path: imported?.[1] ?? null }
    : { kind: "none", path: null };
}

/**
 * A template either defines Goodtype's `preset(body, rhythm:)` function, or is an ordinary Typst
 * header — imports, `#let` definitions, `set`/`show` rules — that is meant to apply to the page
 * the way a preamble applies to a document. Typst does not carry an imported file's set and
 * show rules into the importer, so the second kind is placed in front of the page text instead.
 */
export function isPresetTemplate(source: string): boolean {
  return /^[\t ]*#let[\t ]+preset[\t ]*\(/m.test(source);
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

/** A preamble template, then the page handed back to Goodtype (`goodtype_page`, set in Rust). */
export function templatePreamble(source: string): string {
  return `${source}\n#set page(..goodtype_page)`;
}

/** `preset` is Goodtype's reserved page-template binding; other imports and show rules stay. */
export function withPagePreset(source: string, path: string | null): string {
  const body = source
    .replace(new RegExp(PRESET_IMPORT_LINE, "gm"), "")
    .replace(new RegExp(PRESET_SHOW_LINE, "gm"), "")
    .replace(/^(?:[\t ]*\r?\n)+/, "");
  return path ? `${presetHeader(path)}${body}` : body;
}
