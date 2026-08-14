import { invoke } from "@tauri-apps/api/core";
import type { PresetChoice, PresetSummary } from "../page/presets";

export const listTypstPresets = (root: string | null = null) =>
  invoke<PresetSummary[]>("list_typst_presets", { root });

export const pickTypstPreset = () =>
  invoke<PresetChoice | null>("pick_typst_preset");

export const validateTypstPreset = (choice: PresetChoice) =>
  invoke<void>("validate_typst_preset", { choice });

export const setDefaultTypstPreset = (root: string, choice: PresetChoice) =>
  invoke<PresetSummary | null>("set_default_typst_preset", { root, choice });

export const installPageTypstPreset = (root: string, choice: PresetChoice) =>
  invoke<PresetSummary | null>("install_page_typst_preset", { root, choice });
