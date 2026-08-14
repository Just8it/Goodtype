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

/** `preset` is Goodtype's reserved page-template binding; other imports and show rules stay. */
export function withPagePreset(source: string, path: string | null): string {
  const body = source
    .replace(new RegExp(PRESET_IMPORT_LINE, "gm"), "")
    .replace(new RegExp(PRESET_SHOW_LINE, "gm"), "")
    .replace(/^(?:[\t ]*\r?\n)+/, "");
  return path ? `${presetHeader(path)}${body}` : body;
}
