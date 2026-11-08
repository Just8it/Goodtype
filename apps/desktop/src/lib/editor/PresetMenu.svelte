<script lang="ts">
  import { t } from "../i18n";
  import { tick } from "svelte";
  import type { PresetDiagnostic, PresetSummary } from "../page/presets";
  import { DEFAULT_PRESET_PATH, presetPresentation, pagePresetOptions, pagePresetState } from "../page/presets";

  // The preset picker used to be a full-width <select> that printed the same list three times —
  // once as a page override, once as a notebook default, once as an import. It is one list now,
  // and the thing that actually differs between those three — the scope — is a toggle above it.
  //
  // The action strings are unchanged, so the caller's handler stays as it was.
  let {
    source,
    presets = [],
    busy = false,
    onAction,
  }: {
    source: string;
    presets?: PresetSummary[];
    busy?: boolean;
    onAction: (action: string) => void;
  } = $props();

  /// `problem` marks a preset that cannot be used: short reason in the row, full producer text
  /// on demand. `warning` keeps a usable preset's first warning visible in full.
  type Entry = {
    action: string;
    name: string;
    hint: string;
    problem?: { reason: string; detail: string };
    warning?: string;
  };

  let open = $state(false);
  /// `page` retargets this page only; `notebook` moves the default every page inherits.
  let scope = $state<"page" | "notebook">("page");
  let root = $state<HTMLElement>();
  let chip = $state<HTMLButtonElement>();
  let list = $state<HTMLElement>();
  /// The broken entry whose full text is shown.
  let expanded = $state<string | null>(null);

  const active = $derived(pagePresetState(source));
  const defaultPreset = $derived(presets.find((preset) => preset.kind === "default"));
  const pagePresets = $derived(pagePresetOptions(presets).map(preset => presetPresentation(preset, $t)));
  const builtins = $derived(presets.filter((preset) => preset.kind === "builtin").map(preset => presetPresentation(preset, $t)));

  /// A managed page names its preset; anything else says why it cannot be named.
  const label = $derived.by(() => {
    if (active.kind === "custom") return $t("editor.customSource");
    if (active.kind === "none") return $t("editor.noPreset");
    if (active.path === DEFAULT_PRESET_PATH) return defaultPreset?.name ?? $t("editor.notebookDefault");
    return (
      presets.find((preset) => preset.importPath === active.path)?.name ??
      active.path?.split("/").at(-1)?.replace(/\.typ$/, "") ??
      $t("editor.preset")
    );
  });

  /// The dot is the whole status readout: on preset, hand-written header, or nothing at all.
  const tone = $derived(active.kind === "managed" ? "on" : active.kind === "custom" ? "custom" : "off");

  const current = $derived(
    active.kind === "managed" && active.path ? `path:${active.path}` : active.kind === "none" ? "none" : "",
  );

  function located(diagnostic: PresetDiagnostic): string {
    const where = [diagnostic.file?.replace(/^\/styles\//, ""), diagnostic.line].filter(Boolean).join(":");
    return where ? `${where} · ${diagnostic.message}` : diagnostic.message;
  }

  /// Problem and warning state for one listed preset, localized where Goodtype owns the words.
  function status(preset: PresetSummary | undefined): Pick<Entry, "problem" | "warning"> {
    if (!preset) return {};
    if (preset.problem) {
      const reason = $t(({
        not_utf8: "editor.presetReasonNotUtf8",
        too_large: "editor.presetReasonTooLarge",
        unreadable: "editor.presetReasonUnreadable",
        invalid: "editor.presetReasonInvalid",
      } as const)[preset.problem.code]);
      const error = preset.diagnostics?.find((d) => d.severity === "error");
      return { problem: { reason, detail: error ? located(error) : preset.problem.detail } };
    }
    const warnings = (preset.diagnostics ?? []).filter((d) => d.severity === "warning");
    if (!warnings.length) return {};
    const first = located(warnings[0]);
    return {
      warning: warnings.length === 1
        ? first
        : $t("editor.presetWarningMore", { message: first, count: warnings.length - 1 }),
    };
  }

  const entries = $derived.by<Entry[]>(() =>
    scope === "notebook"
      ? [
          ...builtins.map((preset) => ({
            action: `default:${preset.id}`,
            name: preset.name,
            hint: preset.description,
          })),
          { action: "default:import", name: $t("editor.importTyp"), hint: $t("editor.useAFileOfYourOwn") },
        ]
      : [
          ...(defaultPreset
            ? [{ action: `path:${DEFAULT_PRESET_PATH}`, name: defaultPreset.name, hint: $t("editor.theNotebookDefault"), ...status(defaultPreset) }]
            : []),
          ...pagePresets.map((preset) => ({
            action: preset.importPath ? `path:${preset.importPath}` : `page:${preset.id}`,
            name: preset.name,
            hint: preset.description,
            ...status(preset),
          })),
          ...(active.kind === "managed" && active.path
            ? [{ action: `edit:${active.path}`, name: $t("editor.editThisTemplate"), hint: $t("editor.changeItEveryPageUsingItFollows") }]
            : []),
          { action: "page:new", name: $t("editor.newTemplate"), hint: $t("editor.startOneAndWriteItInThe") },
          { action: "none", name: $t("editor.noPreset"), hint: $t("editor.plainTypstNoPageTemplate") },
          { action: "page:import", name: $t("editor.importTyp"), hint: $t("editor.useAFileOfYourOwn") },
        ],
  );

  function checked(action: string): boolean {
    return scope === "page" ? action === current : action === `default:${defaultPreset?.id}`;
  }

  /// Arrow keys walk the scope toggle and the presets as one list, the way a menu behaves.
  function items(): HTMLButtonElement[] {
    return [...(list?.querySelectorAll<HTMLButtonElement>(".menu-item") ?? [])];
  }

  function close(restore = true) {
    open = false;
    if (restore) chip?.focus();
  }

  function toggle() {
    open = !open;
    if (!open) return;
    scope = "page";
    expanded = null;
    // Open on the preset that is in force, so the current answer is the one under the caret.
    void tick().then(() => {
      const nodes = [...(list?.querySelectorAll<HTMLButtonElement>(".entry") ?? [])];
      (nodes.find((node) => node.getAttribute("aria-checked") === "true") ?? nodes[0])?.focus();
    });
  }

  function pick(entry: Entry) {
    // An unusable preset is never applied; activating it shows why instead.
    if (entry.problem) {
      expanded = expanded === entry.action ? null : entry.action;
      // Keep the disclosed text in view inside the scrolling list, without moving focus.
      if (expanded) void tick().then(() => list?.querySelector(".detail")?.scrollIntoView({ block: "nearest" }));
      return;
    }
    close();
    onAction(entry.action);
  }

  function keys(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    const nodes = items();
    if (!nodes.length) return;
    const index = nodes.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      nodes[(Math.max(index, 0) + step + nodes.length) % nodes.length]?.focus();
    } else if (event.key === "Home") {
      event.preventDefault();
      nodes[0]?.focus();
    } else if (event.key === "End") {
      event.preventDefault();
      nodes.at(-1)?.focus();
    }
  }

  $effect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) close(false);
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  });
</script>

<div class="preset" bind:this={root}>
  <button
    type="button"
    class="chip"
    bind:this={chip}
    disabled={busy}
    aria-haspopup="menu"
    aria-expanded={open}
    title={$t("editor.pagePreset")}
    onclick={toggle}
  >
    <span class="dot" class:custom={tone === "custom"} class:off={tone === "off"} aria-hidden="true"></span>
    <span class="name">{label}</span>
    <span class="caret" aria-hidden="true">▾</span>
  </button>

  {#if open}
    <div class="menu" bind:this={list} role="menu" tabindex="-1" aria-label={$t("editor.pagePreset")} onkeydown={keys}>
      <div class="scope" role="group" aria-label={$t("editor.applyTo")}>
        <button
          type="button"
          class="menu-item"
          role="menuitemradio"
          aria-checked={scope === "page"}
          onclick={() => (scope = "page")}
        >{$t("editor.thisPage")}</button>
        <button
          type="button"
          class="menu-item"
          role="menuitemradio"
          aria-checked={scope === "notebook"}
          onclick={() => (scope = "notebook")}
        >{$t("editor.wholeNotebook")}</button>
      </div>

      <ul role="group" aria-label={$t("editor.presets")}>
        {#each entries as entry (entry.action)}
          {@const detailId = `preset-detail-${entry.action.replace(/[^A-Za-z0-9_-]/g, "_")}`}
          <li role="none">
            <button
              type="button"
              class="entry menu-item"
              class:unavailable={!!entry.problem}
              role="menuitemradio"
              aria-checked={checked(entry.action)}
              aria-disabled={entry.problem ? "true" : undefined}
              aria-expanded={entry.problem ? expanded === entry.action : undefined}
              aria-controls={entry.problem && expanded === entry.action ? detailId : undefined}
              aria-label={entry.problem
                ? $t("editor.presetEntryInvalid", { name: entry.name, reason: entry.problem.reason })
                : undefined}
              onclick={() => pick(entry)}
            >
              <span class="tick" aria-hidden="true">{checked(entry.action) ? "✓" : ""}</span>
              <span class="text">
                <span class="entry-name">{entry.name}</span>
                {#if entry.problem}
                  <span class="note">
                    <span class="mark error" aria-hidden="true"></span>
                    <span>{$t("editor.presetUnavailable", { reason: entry.problem.reason })}</span>
                    <span class="toggle">{expanded === entry.action ? $t("editor.presetHideDetails") : $t("editor.presetShowDetails")}</span>
                  </span>
                {:else if entry.warning}
                  <span class="note">
                    <span class="mark warning" aria-hidden="true"></span>
                    <span>{$t("editor.presetWarning", { message: entry.warning })}</span>
                  </span>
                {:else if entry.hint}
                  <span class="hint">{entry.hint}</span>
                {/if}
              </span>
            </button>
            {#if entry.problem && expanded === entry.action}
              <p class="detail" id={detailId}>{entry.problem.detail}</p>
            {/if}
          </li>
        {/each}
      </ul>

      <p class="footnote">
        {#if scope === "notebook"}
          {$t("editor.changesThePresetEveryPageInherits", { "value2": defaultPreset ? $t("editor.currentPreset", { name: defaultPreset.name }) : "" })}
        {:else}
          {$t("editor.overridesThePresetForThisPageOnly")}
        {/if}
      </p>
    </div>
  {/if}
</div>

<style>
  .preset {
    position: relative;
    min-width: 0;
    flex: 0 1 auto;
  }

  .chip {
    display: flex;
    max-width: 100%;
    height: var(--control-dense);
    align-items: center;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: var(--text-md)/1 var(--font-ui);
    cursor: pointer;
    gap: 7px;
  }

  .chip:hover:not(:disabled) {
    background: var(--wash);
    color: var(--text);
  }

  .chip:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: 2px;
  }

  .chip:disabled {
    cursor: not-allowed;
    opacity: var(--disabled);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 2px;
    background: var(--blueprint);
  }

  .dot.custom {
    background: var(--amber);
  }

  .dot.off {
    background: var(--quiet);
  }

  .caret {
    flex: none;
    color: var(--quiet);
    font-size: var(--text-xs);
  }

  .menu {
    position: absolute;
    z-index: 20;
    top: calc(100% + 6px);
    right: 0;
    width: 264px;
    max-width: calc(100vw - 24px);
    padding: 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow-menu);
    color: var(--text);
  }

  .scope {
    display: flex;
    padding: 2px;
    border-radius: var(--radius);
    background: rgb(0 0 0 / 22%);
    gap: 2px;
  }

  .scope button {
    height: 24px;
    flex: 1;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: var(--text-sm)/1 var(--font-ui);
    cursor: pointer;
  }

  .scope button[aria-checked="true"] {
    background: var(--edge);
    color: var(--text);
  }

  .menu-item:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: -1px;
  }

  .menu:focus-visible {
    outline: none;
  }

  ul {
    max-height: 268px;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
  }

  .entry {
    display: flex;
    width: 100%;
    align-items: flex-start;
    padding: 6px 6px 6px 4px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: inherit;
    font: var(--text-md)/1.35 var(--font-ui);
    text-align: left;
    cursor: pointer;
    gap: 6px;
  }

  .entry:hover {
    background: var(--wash);
  }

  .tick {
    width: 13px;
    flex: none;
    color: var(--blueprint-light);
  }

  .text {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 2px;
  }

  .hint {
    overflow: hidden;
    color: var(--quiet);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A problem or warning is essential information, so it reads in muted rather than quiet. */
  .note {
    display: flex;
    min-width: 0;
    align-items: baseline;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.35;
    overflow-wrap: anywhere;
    gap: 6px;
  }

  /* The chip's square status dot, reused: amber for a warning, oxide for an unusable preset. */
  .mark {
    width: 6px;
    height: 6px;
    flex: none;
    align-self: flex-start;
    margin-top: calc(0.675em - 3px);
    border-radius: 2px;
  }

  .mark.warning {
    background: var(--amber);
  }

  .mark.error {
    background: var(--oxide);
  }

  .toggle {
    margin-left: auto;
    flex: none;
    color: var(--blueprint-light);
    white-space: nowrap;
  }

  .entry.unavailable {
    cursor: default;
  }

  .entry.unavailable .entry-name {
    color: var(--muted);
  }

  .detail {
    max-height: 7.5rem;
    margin: 0 4px 4px 23px;
    padding: 6px 8px;
    border-top: 1px solid var(--edge-soft);
    color: var(--muted);
    font: var(--text-sm)/1.45 var(--font-code);
    overflow-wrap: anywhere;
    overflow-y: auto;
    user-select: text;
  }

  .footnote {
    margin: 6px 2px 2px;
    color: var(--quiet);
    font-size: var(--text-sm);
    line-height: 1.4;
  }
</style>
