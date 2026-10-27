import type { AppSettings } from "../settings";
import { LANGUAGES, translate, formatNumber, type Translator } from "../i18n";
import { paletteTools, type PaletteCommand } from "../prototype/palette";

/**
 * What Settings holds, described as data rather than markup.
 *
 * The panel used to be one scrolling column of hand-written sections, so every new preference
 * cost another block of form markup and made the column longer than the one before it. Here a
 * setting is an object: it says what it is called, how it is read and written, and which group
 * it belongs to. The panel renders whatever this list contains, and the search field can only
 * exist because the list is inspectable.
 *
 * Adding a preference is one entry. Adding a group is one entry with a title.
 *
 * Values are clamped again in Rust; the ranges here are what the writer is offered, not a trust
 * boundary.
 */

/**
 * What a setting can ask the app to *do*, as opposed to a value it can store.
 *
 * The schema says which action an entry offers; the panel's caller says what that action is. A
 * diagnostic belongs in Settings rather than one tap from the page, but it is not a preference,
 * so it needs somewhere to send the request.
 */
export type SettingActions = { openMetrics: () => void };

export type SettingControl =
  | {
      kind: "toggle";
      read: (settings: AppSettings) => boolean;
      write: (settings: AppSettings, value: boolean) => AppSettings;
      /** True while the switch may not be flipped; the hint should say why. */
      locked?: (settings: AppSettings) => boolean;
    }
  | {
      kind: "choice" | "select";
      options: { value: string; label: string; hint?: string }[];
      read: (settings: AppSettings) => string;
      write: (settings: AppSettings, value: string) => AppSettings;
    }
  | {
      kind: "action";
      buttonLabel: string;
      run: (actions: SettingActions) => void;
    }
  | {
      kind: "slider";
      min: number;
      max: number;
      step: number;
      /** How the number is shown. A raw 0.35 means nothing to the writer on its own. */
      format: (value: number) => string;
      read: (settings: AppSettings) => number;
      write: (settings: AppSettings, value: number) => AppSettings;
    };

export type SettingItem = {
  id: string;
  label: string;
  hint?: string;
  /** Words that should find this setting but do not appear in its own text. */
  keywords?: string[];
  control: SettingControl;
};

export type SettingGroup = {
  id: string;
  title: string;
  /** Drawn in the sidebar. Stroke paths on a 24 viewBox, like every other icon in the app. */
  icon: string;
  blurb?: string;
  items: SettingItem[];
};

export function settingGroups(t: Translator = translate, fonts: string[] = []): SettingGroup[] { return [
  {
    id: "appearance",
    title: t("workspace.appearance"),
    icon: "M12 3a9 9 0 1 0 0 18h2a2 2 0 0 0 0-4h-1a2 2 0 0 1 0-4h3a5 5 0 0 0 0-10z",
    items: [{
      id: "uiLanguage",
      label: t("language.title"),
      hint: t("language.hint"),
      keywords: t("language.search").split(" "),
      control: {
        kind: "choice",
        options: [...LANGUAGES],
        read: (settings) => settings.uiLanguage,
        write: (settings, value) => ({ ...settings, uiLanguage: value === "de" ? "de" : "en" }),
      },
    }],
  },
  {
    id: "pen",
    title: t("workspace.pen"),
    icon: "M15.5 3.5l5 5-9.5 9.5-5.5 1.5 1.5-5.5 9.5-9.5zM6 20l1.2-3.6",
    blurb:
      t("workspace.nibWidthAndColourLiveOnThe"),
    items: [
      {
        id: "pressureEnabled",
        label: t("workspace.useStylusPressure"),
        hint: t("workspace.offGivesEveryPressureSensitivePenA"),
        keywords: [t("settingsSearch.pressureEnabled")],
        control: {
          kind: "toggle",
          read: (settings) => settings.pressureEnabled,
          write: (settings, value) => ({ ...settings, pressureEnabled: value }),
        },
      },
      {
        id: "curve",
        label: t("workspace.pressureCurve"),
        hint: t("workspace.belowMakesALightTouchDrawHeavier"),
        keywords: [t("settingsSearch.curve")],
        control: {
          kind: "slider",
          min: 0.25,
          max: 3,
          step: 0.05,
          format: (value) => formatNumber(value, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
          read: (settings) => settings.calibration.curve,
          write: (settings, value) => ({
            ...settings,
            calibration: { ...settings.calibration, curve: value },
          }),
        },
      },
      {
        id: "smoothing",
        label: t("workspace.strokeSmoothing"),
        hint: t("workspace.steadiesALineAsItIsDrawn"),
        keywords: [t("settingsSearch.smoothing")],
        control: {
          kind: "slider",
          min: 0,
          max: 0.8,
          step: 0.05,
          format: (value) => formatNumber(value, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
          read: (settings) => settings.calibration.smoothing,
          write: (settings, value) => ({
            ...settings,
            calibration: { ...settings.calibration, smoothing: value },
          }),
        },
      },
      {
        id: "drawAndHoldShapes",
        label: t("workspace.drawAndHoldForShapes"),
        hint: t("workspace.pauseAtTheEndOfADeliberate"),
        keywords: [t("settingsSearch.drawAndHoldShapes")],
        control: {
          kind: "toggle",
          read: (settings) => settings.drawAndHoldShapes,
          write: (settings, value) => ({ ...settings, drawAndHoldShapes: value }),
        },
      },
      {
        id: "drawWithFinger",
        label: t("workspace.drawWithFinger"),
        hint: t("workspace.oneFingerUsesTheToolInHand"),
        keywords: [t("settingsSearch.drawWithFinger")],
        control: {
          kind: "toggle",
          read: (settings) => settings.drawWithFinger,
          write: (settings, value) => ({ ...settings, drawWithFinger: value }),
        },
      },
      {
        id: "touchHistory",
        label: t("workspace.doubleTapWithTwoFingersToUndo"),
        hint: t("workspace.threeFingersRedoUseTwoQuickTaps"),
        keywords: [t("settingsSearch.touchHistory")],
        control: {
          kind: "toggle",
          read: (settings) => settings.touchHistory,
          write: (settings, value) => ({ ...settings, touchHistory: value }),
        },
      },
      {
        id: "penSideButton",
        label: t("workspace.penSideButton"),
        hint: t("workspace.holdTheButtonOnTheBarrelWhile"),
        keywords: [t("settingsSearch.penSideButton")],
        control: {
          kind: "choice",
          options: [
            { value: "eraser", label: t("workspace.eraser") },
            { value: "lasso", label: t("workspace.lasso"), hint: t("workspace.circleInkToSelectIt") },
            { value: "none", label: t("workspace.nothing"), hint: t("workspace.thePenKeepsWriting") },
          ],
          read: (settings) => settings.penSideButton,
          write: (settings, value) => ({
            ...settings,
            penSideButton: value as AppSettings["penSideButton"],
          }),
        },
      },
    ],
  },
  {
    id: "palette",
    title: t("workspace.palette"),
    icon: "M4 7h16M4 12h16M4 17h16M9 4v16",
    blurb:
      t("workspace.takeToolsYouNeverUseOffThe"),
    items: paletteTools(t).map((tool) => paletteToggle(tool.id, tool.label, t)),
  },
  {
    id: "editing",
    title: t("workspace.editing"),
    icon: "M4 7h16M4 12h10M4 17h13",
    items: [
      {
        id: "inlineTextFont",
        label: t("settings.inlineTextFont"),
        hint: t("settings.inlineTextFontHint"),
        keywords: t("settings.inlineTextFontSearch").split(" "),
        control: {
          kind: "select",
          options: [{ value: "", label: t("settings.inheritBlockFont") }, ...fonts.map(font => ({ value: font, label: font }))],
          read: settings => settings.inlineTextFont,
          write: (settings, value) => ({ ...settings, inlineTextFont: value }),
        },
      },
      {
        id: "undoScope",
        label: t("workspace.undoReaches"),
        hint: t("workspace.whatCtrlZStepsBackThrough"),
        keywords: [t("settingsSearch.undoScope")],
        control: {
          kind: "choice",
          options: [
            { value: "page", label: t("editor.thisPage"), hint: t("workspace.onlyThePageInView") },
            { value: "notebook", label: t("editor.wholeNotebook"), hint: t("workspace.theMostRecentChangeAnywhere") },
          ],
          read: (settings) => settings.undoScope,
          write: (settings, value) => ({
            ...settings,
            undoScope: value as AppSettings["undoScope"],
          }),
        },
      },
      {
        id: "pullToAddPage",
        label: t("workspace.pullPastTheLastPageToAdd"),
        hint: t("workspace.keepScrollingAtTheEndOfThe"),
        keywords: [t("settingsSearch.pullToAddPage")],
        control: {
          kind: "toggle",
          read: (settings) => settings.pullToAddPage,
          write: (settings, value) => ({ ...settings, pullToAddPage: value }),
        },
      },
      {
        id: "pageTextLineWrap",
        label: t("workspace.wrapLongEditorLines"),
        hint: t("workspace.keepsProseVisibleWithoutWritingLineBreaks"),
        keywords: [t("settingsSearch.pageTextLineWrap")],
        control: {
          kind: "toggle",
          read: (settings) => settings.pageTextLineWrap,
          write: (settings, value) => ({ ...settings, pageTextLineWrap: value }),
        },
      },
      {
        id: "pageTextBaselineGrid",
        label: t("workspace.snapBlocksToPaperRhythm"),
        hint: t("workspace.headingsAndDisplayEquationsReserveWholePaper"),
        keywords: [t("settingsSearch.pageTextBaselineGrid")],
        control: {
          kind: "toggle",
          read: (settings) => settings.pageTextBaselineGrid,
          write: (settings, value) => ({ ...settings, pageTextBaselineGrid: value }),
        },
      },
    ],
  },
  {
    id: "typst",
    title: "Typst",
    icon: "M5 4.5h14M12 4.5v15M8 19.5h8",
    items: [
      {
        id: "remotePackages",
        label: t("workspace.downloadPackagesFromTypstUniverse"),
        hint: t("workspace.fetchesAnImportedPackageTheFirstTime"),
        keywords: [t("settingsSearch.remotePackages")],
        control: {
          kind: "toggle",
          read: (settings) => settings.remotePackages,
          write: (settings, value) => ({ ...settings, remotePackages: value }),
        },
      },
    ],
  },
  {
    id: "motion",
    title: t("workspace.motion"),
    icon: "M4 12h5l2-5 3 10 2-5h4",
    items: [
      {
        id: "touchGlide",
        label: t("workspace.touchGlide"),
        hint: t("workspace.howFarTheCanvasKeepsTravellingAfter"),
        keywords: [t("settingsSearch.touchGlide")],
        control: {
          kind: "slider",
          min: 0,
          max: 4,
          step: 0.1,
          format: (value) => `${Math.round(value * 50)}%`,
          read: (settings) => settings.touchGlide,
          write: (settings, value) => ({ ...settings, touchGlide: value }),
        },
      },
      {
        id: "reducedMotion",
        label: t("workspace.reduceMotion"),
        hint: t("workspace.skipsSmoothScrollingAndAnimatedTransitions"),
        keywords: [t("settingsSearch.reducedMotion")],
        control: {
          kind: "toggle",
          read: (settings) => settings.reducedMotion,
          write: (settings, value) => ({ ...settings, reducedMotion: value }),
        },
      },
    ],
  },
  {
    id: "diagnostics",
    title: t("workspace.diagnostics"),
    icon: "M12 7v5l3.5 2M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z",
    blurb: t("workspace.localMeasurementsOnlyNothingHereLeavesThis"),
    items: [
      {
        id: "metrics",
        label: t("workspace.timingEvidence"),
        hint: t("workspace.strokeLatencyAndCompileTimingsMeasuredOn"),
        keywords: [t("settingsSearch.metrics")],
        control: {
          kind: "action",
          buttonLabel: t("workspace.open"),
          run: (actions) => actions.openMetrics(),
        },
      },
    ],
  },
]; }

/** At least one pen stays on the bar; Rust holds the same line for an edited settings file. */
function lastPenShown(settings: AppSettings, id: PaletteCommand): boolean {
  const otherPen = id === "pen-1" ? "pen-2" : id === "pen-2" ? "pen-1" : null;
  return (
    otherPen !== null &&
    !settings.hiddenTools.includes(id) &&
    settings.hiddenTools.includes(otherPen)
  );
}

function paletteToggle(id: PaletteCommand, label: string, t: Translator): SettingItem {
  return {
    id: `palette-${id}`,
    label,
    hint: id === "pen-1" || id === "pen-2" ? t("workspace.atLeastOnePenStaysOnThe") : undefined,
    keywords: [t("settingsSearch.palette")],
    control: {
      kind: "toggle",
      read: (settings) => !settings.hiddenTools.includes(id),
      write: (settings, shown) => ({
        ...settings,
        hiddenTools: shown
          ? settings.hiddenTools.filter((hidden) => hidden !== id)
          : [...settings.hiddenTools.filter((hidden) => hidden !== id), id],
      }),
      locked: (settings) => lastPenShown(settings, id),
    },
  };
}

/** One flat row per setting, carrying the group it came from so a result can say where it lives. */
export type SettingHit = { group: SettingGroup; item: SettingItem };

/**
 * Settings matching `query`, across every group.
 *
 * Matches the label, the hint, the group title and the hidden keywords, so "stabiliser" finds
 * smoothing and "offline" finds the package switch — the words a writer reaches for are rarely
 * the words on the control.
 */
export function searchSettings(query: string, groups = settingGroups()): SettingHit[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const words = needle.split(/\s+/);
  return groups.flatMap((group) =>
    group.items.map((item) => ({ group, item })),
  ).filter(({ group, item }) => {
    const haystack = [group.title, item.label, item.hint ?? "", ...(item.keywords ?? [])]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}
