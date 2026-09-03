import type { AppSettings } from "../settings";

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
    }
  | {
      kind: "choice";
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

export const SETTING_GROUPS: SettingGroup[] = [
  {
    id: "pen",
    title: "Pen",
    icon: "M15.5 3.5l5 5-9.5 9.5-5.5 1.5 1.5-5.5 9.5-9.5zM6 20l1.2-3.6",
    blurb:
      "Nib, width and colour live on the palette, one tap away. What is here is how the pen answers your hand.",
    items: [
      {
        id: "pressureEnabled",
        label: "Use stylus pressure",
        hint: "Off gives every pressure-sensitive pen a uniform width",
        keywords: ["force", "stylus", "tilt"],
        control: {
          kind: "toggle",
          read: (settings) => settings.pressureEnabled,
          write: (settings, value) => ({ ...settings, pressureEnabled: value }),
        },
      },
      {
        id: "curve",
        label: "Pressure curve",
        hint: "Below 1 makes a light touch draw heavier; above 1 asks for more force",
        keywords: ["calibration", "response", "sensitivity"],
        control: {
          kind: "slider",
          min: 0.25,
          max: 3,
          step: 0.05,
          format: (value) => value.toFixed(2),
          read: (settings) => settings.calibration.curve,
          write: (settings, value) => ({
            ...settings,
            calibration: { ...settings.calibration, curve: value },
          }),
        },
      },
      {
        id: "smoothing",
        label: "Stroke smoothing",
        hint: "Steadies a line as it is drawn. High values round off deliberate detail",
        keywords: ["stabiliser", "stabilizer", "shake", "jitter"],
        control: {
          kind: "slider",
          min: 0,
          max: 0.8,
          step: 0.05,
          format: (value) => value.toFixed(2),
          read: (settings) => settings.calibration.smoothing,
          write: (settings, value) => ({
            ...settings,
            calibration: { ...settings.calibration, smoothing: value },
          }),
        },
      },
      {
        id: "drawAndHoldShapes",
        label: "Draw and hold for shapes",
        hint: "Pause at the end of a deliberate line or form to make it editable",
        keywords: ["shape", "circle", "rectangle", "straighten", "quick shape"],
        control: {
          kind: "toggle",
          read: (settings) => settings.drawAndHoldShapes,
          write: (settings, value) => ({ ...settings, drawAndHoldShapes: value }),
        },
      },
    ],
  },
  {
    id: "editing",
    title: "Editing",
    icon: "M4 7h16M4 12h10M4 17h13",
    items: [
      {
        id: "undoScope",
        label: "Undo reaches",
        hint: "What Ctrl+Z steps back through",
        keywords: ["redo", "history", "ctrl z"],
        control: {
          kind: "choice",
          options: [
            { value: "page", label: "This page", hint: "Only the page in view" },
            { value: "notebook", label: "Whole notebook", hint: "The most recent change anywhere" },
          ],
          read: (settings) => settings.undoScope,
          write: (settings, value) => ({
            ...settings,
            undoScope: value as AppSettings["undoScope"],
          }),
        },
      },
      {
        id: "pageTextLineWrap",
        label: "Wrap long editor lines",
        hint: "Keeps prose visible without writing line breaks into the Typst source (Alt+Z)",
        keywords: ["word wrap", "page text", "source"],
        control: {
          kind: "toggle",
          read: (settings) => settings.pageTextLineWrap,
          write: (settings, value) => ({ ...settings, pageTextLineWrap: value }),
        },
      },
      {
        id: "pageTextBaselineGrid",
        label: "Snap blocks to paper rhythm",
        hint: "Headings and display equations reserve whole paper rows",
        keywords: ["baseline", "grid", "ruling", "page text"],
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
        label: "Download packages from Typst Universe",
        hint: "Fetches an imported package the first time you use it, then keeps it on this device. Packages you already have keep working offline either way.",
        keywords: ["network", "offline", "universe", "import"],
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
    title: "Motion",
    icon: "M4 12h5l2-5 3 10 2-5h4",
    items: [
      {
        id: "touchGlide",
        label: "Touch glide",
        hint: "How far the canvas keeps travelling after a one-finger pan is released",
        keywords: ["inertia", "scroll", "momentum", "pan"],
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
        label: "Reduce motion",
        hint: "Skips smooth scrolling and animated transitions",
        keywords: ["accessibility", "animation", "vestibular"],
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
    title: "Diagnostics",
    icon: "M12 7v5l3.5 2M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z",
    blurb: "Local measurements only. Nothing here leaves this device.",
    items: [
      {
        id: "metrics",
        label: "Timing evidence",
        hint: "Stroke latency and compile timings measured on this machine",
        keywords: ["performance", "latency", "metrics", "profiling"],
        control: {
          kind: "action",
          buttonLabel: "Open",
          run: (actions) => actions.openMetrics(),
        },
      },
    ],
  },
];

/** One flat row per setting, carrying the group it came from so a result can say where it lives. */
export type SettingHit = { group: SettingGroup; item: SettingItem };

/**
 * Settings matching `query`, across every group.
 *
 * Matches the label, the hint, the group title and the hidden keywords, so "stabiliser" finds
 * smoothing and "offline" finds the package switch — the words a writer reaches for are rarely
 * the words on the control.
 */
export function searchSettings(query: string): SettingHit[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const words = needle.split(/\s+/);
  return SETTING_GROUPS.flatMap((group) =>
    group.items.map((item) => ({ group, item })),
  ).filter(({ group, item }) => {
    const haystack = [group.title, item.label, item.hint ?? "", ...(item.keywords ?? [])]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}
