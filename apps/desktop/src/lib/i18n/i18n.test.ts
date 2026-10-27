import { afterEach, describe, expect, it } from "vitest";
import { createInstance } from "i18next";
import { get } from "svelte/store";
import { render } from "svelte/server";
import { en } from "./en";
import { de } from "./de";
import { positionMessage, shapeMessage } from "./labels";
import { formatDate, formatNumber, locale, notice, renderMessage, setLanguage, t, translate } from ".";
import { settingGroups, searchSettings } from "../workspace/settingsSchema";
import { typstSnippets } from "../editor/snippets";
import { paletteTools } from "../notebook/palette";
import { DEFAULT_SETTINGS } from "../settings";
import SettingsPanel from "../workspace/SettingsPanel.svelte";
import ConflictDialog from "../workspace/ConflictDialog.svelte";
import { presetPresentation, type PresetSummary } from "../page/presets";

afterEach(() => setLanguage("en"));

function messages(value: object, prefix = ""): Record<string, string> {
  return Object.fromEntries(Object.entries(value).flatMap(([key, item]) =>
    typeof item === "string" ? [[prefix + key, item]] : Object.entries(messages(item, prefix + key + "."))));
}

describe("bundled English and German interface", () => {
  it("ships matching, non-empty messages and interpolation placeholders", () => {
    const english = messages(en), german = messages(de);
    expect(Object.keys(german).sort()).toEqual(Object.keys(english).sort());
    const placeholders = (value: string) => [...value.matchAll(/{{\s*([^}]+?)\s*}}/g)].map(match => match[1]).sort();
    for (const [key, text] of Object.entries(english)) {
      expect(text.trim(), key).not.toBe("");
      expect(german[key].trim(), key).not.toBe("");
      expect(placeholders(german[key]), key).toEqual(placeholders(text));
    }
  });

  it("uses an explicit language and falls back without a network loader", async () => {
    expect(get(locale)).toBe("en");
    expect(setLanguage("de")).toBe("de");
    expect(translate("library.settings")).toBe("Einstellungen");
    expect(setLanguage("de-AT")).toBe("en");
    expect(translate("library.settings")).toBe("Settings");
    const isolated = createInstance();
    await isolated.init({ lng: "de", fallbackLng: "en", resources: {
      en: { translation: { available: "English fallback" } }, de: { translation: {} },
    } });
    expect(isolated.t("available" as never)).toBe("English fallback");
  });

  it("selects complete singular and plural messages for zero, one and two", () => {
    for (const [language, expected] of [["en", ["0 pages", "1 page", "2 pages"]], ["de", ["0 Seiten", "1 Seite", "2 Seiten"]]] as const) {
      setLanguage(language);
      expect([0, 1, 2].map(count => translate("counts.pages", { count }))).toEqual(expected);
    }
    expect(translate("counts.restyled", { count: 1 })).toBe("1 ausgewähltes Element neu gestaltet");
    expect(translate("counts.pasted", { count: 2 })).toBe("2 Elemente eingefügt");
  });

  it("re-renders retained statuses including nested commit labels", () => {
    const message = notice("workspace.savedRevision", { label: notice("workspace.updatedInk"), revision: 8 });
    const held = notice("workspace.convertedHeldInkTo", { kind: shapeMessage("line") });
    const docked = notice("workspace.toolPaletteDockedToTheEdge", { next: positionMessage("left") });
    expect(renderMessage(message)).toBe("Updated ink; saved revision 8");
    setLanguage("de");
    expect(renderMessage(message)).toBe("Striche aktualisiert; Revision 8 gespeichert");
    expect(renderMessage(held)).toBe("Gehaltene Striche in Linie umgewandelt");
    expect(renderMessage(docked)).toContain("links");
  });

  it("formats display values while retaining ordinary source and settings data", () => {
    const original = JSON.stringify(DEFAULT_SETTINGS);
    const source = typstSnippets(get(t)).map(snippet => snippet.template);
    expect(formatNumber(12.5)).toBe("12.5");
    setLanguage("de");
    expect(formatNumber(12.5)).toBe("12,5");
    expect(formatDate(new Date(2026, 9, 22))).toContain("Okt.");
    expect(typstSnippets(get(t)).map(snippet => snippet.template)).toEqual(source);
    expect(JSON.stringify(DEFAULT_SETTINGS)).toBe(original);
    expect(paletteTools(get(t)).map(tool => tool.id)).toEqual(paletteTools(translate).map(tool => tool.id));
  });

  it("searches German labels and hidden keywords with stable setting IDs", () => {
    setLanguage("de");
    const groups = settingGroups(get(t));
    expect(searchSettings("Stabilisierung", groups).map(hit => hit.item.id)).toContain("smoothing");
    for (const query of ["Sprache", "language", "English", "Deutsch"]) {
      expect(searchSettings(query, groups).map(hit => hit.item.id)).toContain("uiLanguage");
    }
    expect(searchSettings("Handballen", groups).map(hit => hit.item.id)).toContain("drawWithFinger");
    const smoothing = groups.flatMap(group => group.items).find(item => item.id === "smoothing")!;
    if (smoothing.control.kind === "slider") expect(smoothing.control.format(0.2)).toBe("0,20");
  });

  it("renders German semantic controls and escapes user-provided details", () => {
    setLanguage("de");
    const { body } = render(SettingsPanel, { props: {
      settings: { ...DEFAULT_SETTINGS, uiLanguage: "de" }, actions: { openMetrics() {} }, onChange() {}, onClose() {},
    } });
    expect(body).toContain('aria-label="Einstellungen"');
    expect(body).toContain('role="radio"');
    expect(body).toContain("English");
    expect(body).toContain("Deutsch");
    const conflict = render(ConflictDialog, { props: { detail: '<img src=x onerror="alert(1)"> & source.typ', onReload() {}, onCancel() {} } }).body;
    expect(conflict).toContain("Notizbuch außerhalb von Goodtype geändert");
    expect(conflict).toContain("&lt;img");
    expect(conflict).not.toContain("<img");
  });

  it("localizes built-in help without replacing names read from user style files", () => {
    setLanguage("de");
    const builtin: PresetSummary = { id: "clean-notes", kind: "builtin", name: "Clean Notes", description: "Open headings and relaxed notes", importPath: null };
    expect(presetPresentation(builtin).name).toBe("Klare Notizen");
    const user = { ...builtin, id: "styles/lab.typ", kind: "custom" as const, name: "My English lab notes" };
    expect(presetPresentation(user).name).toBe(user.name);
  });
});
