import { get } from "svelte/store";
import { locale } from "../i18n";
import { invoke } from "@tauri-apps/api/core";
import type { PresetChoice, PresetSummary } from "../page/presets";

export const listTypstPresets = (root: string | null = null) =>
  invoke<PresetSummary[]>("list_typst_presets", { root });

export const pickTypstPreset = () =>
  invoke<PresetChoice | null>("pick_typst_preset", { uiLanguage: get(locale) });

export const validateTypstPreset = (choice: PresetChoice) =>
  invoke<void>("validate_typst_preset", { choice });

export const setDefaultTypstPreset = (root: string, choice: PresetChoice, expectedSource: string | null) =>
  invoke<PresetSummary | null>("set_default_typst_preset", { root, choice, expectedSource });

export const installPageTypstPreset = (root: string, choice: PresetChoice) =>
  invoke<PresetSummary | null>("install_page_typst_preset", { root, choice });

/** A new `template-N.typ` with a starter, ready to edit. */
export const createTypstPreset = (root: string) =>
  invoke<PresetSummary>("create_typst_preset", { root });

/** `path` is the import path pages use, e.g. `/styles/template-1.typ`. */
export const readTypstPreset = (root: string, path: string) =>
  invoke<string>("read_typst_preset", { root, path });

export const saveTypstPreset = (root: string, path: string, source: string, expectedSource: string) =>
  invoke<void>("save_typst_preset", { root, path, source, expectedSource });
