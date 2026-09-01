<script lang="ts">
  import { tick } from "svelte";
  import type { PresetSummary } from "../page/presets";
  import { DEFAULT_PRESET_PATH, pagePresetOptions, pagePresetState } from "../page/presets";

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

  type Entry = { action: string; name: string; hint: string };

  let open = $state(false);
  /// `page` retargets this page only; `notebook` moves the default every page inherits.
  let scope = $state<"page" | "notebook">("page");
  let root = $state<HTMLElement>();
  let chip = $state<HTMLButtonElement>();
  let list = $state<HTMLElement>();

  const active = $derived(pagePresetState(source));
  const defaultPreset = $derived(presets.find((preset) => preset.kind === "default"));
  const pagePresets = $derived(pagePresetOptions(presets));
  const builtins = $derived(presets.filter((preset) => preset.kind === "builtin"));

  /// A managed page names its preset; anything else says why it cannot be named.
  const label = $derived.by(() => {
    if (active.kind === "custom") return "Custom source";
    if (active.kind === "none") return "No preset";
    if (active.path === DEFAULT_PRESET_PATH) return defaultPreset?.name ?? "Notebook default";
    return (
      presets.find((preset) => preset.importPath === active.path)?.name ??
      active.path?.split("/").at(-1)?.replace(/\.typ$/, "") ??
      "Preset"
    );
  });

  /// The dot is the whole status readout: on preset, hand-written header, or nothing at all.
  const tone = $derived(active.kind === "managed" ? "on" : active.kind === "custom" ? "custom" : "off");

  const current = $derived(
    active.kind === "managed" && active.path ? `path:${active.path}` : active.kind === "none" ? "none" : "",
  );

  const entries = $derived.by<Entry[]>(() =>
    scope === "notebook"
      ? [
          ...builtins.map((preset) => ({
            action: `default:${preset.id}`,
            name: preset.name,
            hint: preset.description,
          })),
          { action: "default:import", name: "Import .typ…", hint: "Use a file of your own" },
        ]
      : [
          ...(defaultPreset
            ? [{ action: `path:${DEFAULT_PRESET_PATH}`, name: defaultPreset.name, hint: "The notebook default" }]
            : []),
          ...pagePresets.map((preset) => ({
            action: preset.importPath ? `path:${preset.importPath}` : `page:${preset.id}`,
            name: preset.name,
            hint: preset.description,
          })),
          { action: "none", name: "No preset", hint: "Plain Typst, no page template" },
          { action: "page:import", name: "Import .typ…", hint: "Use a file of your own" },
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
    // Open on the preset that is in force, so the current answer is the one under the caret.
    void tick().then(() => {
      const nodes = [...(list?.querySelectorAll<HTMLButtonElement>(".entry") ?? [])];
      (nodes.find((node) => node.getAttribute("aria-checked") === "true") ?? nodes[0])?.focus();
    });
  }

  function pick(action: string) {
    close();
    onAction(action);
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
    title="Page preset"
    onclick={toggle}
  >
    <span class="dot" class:custom={tone === "custom"} class:off={tone === "off"} aria-hidden="true"></span>
    <span class="name">{label}</span>
    <span class="caret" aria-hidden="true">▾</span>
  </button>

  {#if open}
    <div class="menu" bind:this={list} role="menu" tabindex="-1" aria-label="Page preset" onkeydown={keys}>
      <div class="scope" role="group" aria-label="Apply to">
        <button
          type="button"
          class="menu-item"
          role="menuitemradio"
          aria-checked={scope === "page"}
          onclick={() => (scope = "page")}
        >This page</button>
        <button
          type="button"
          class="menu-item"
          role="menuitemradio"
          aria-checked={scope === "notebook"}
          onclick={() => (scope = "notebook")}
        >Whole notebook</button>
      </div>

      <ul role="group" aria-label="Presets">
        {#each entries as entry (entry.action)}
          <li role="none">
            <button
              type="button"
              class="entry menu-item"
              role="menuitemradio"
              aria-checked={checked(entry.action)}
              onclick={() => pick(entry.action)}
            >
              <span class="tick" aria-hidden="true">{checked(entry.action) ? "✓" : ""}</span>
              <span class="text">
                <span class="entry-name">{entry.name}</span>
                {#if entry.hint}<span class="hint">{entry.hint}</span>{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>

      <p class="footnote">
        {#if scope === "notebook"}
          Changes the preset every page inherits{defaultPreset ? `. Now: ${defaultPreset.name}` : ""}.
        {:else}
          Overrides the preset for this page only.
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
    color: var(--muted, #aeb5be);
    font: var(--text-md)/1 var(--font-ui, Bahnschrift, system-ui, sans-serif);
    cursor: pointer;
    gap: 7px;
  }

  .chip:hover:not(:disabled) {
    background: var(--wash);
    color: var(--text, #e9ebee);
  }

  .chip:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: 1px;
  }

  .chip:disabled {
    cursor: default;
    opacity: 0.55;
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
    background: var(--blueprint, #4c8df0);
  }

  .dot.custom {
    background: var(--amber, #e0912b);
  }

  .dot.off {
    background: var(--quiet, #6a727c);
  }

  .caret {
    flex: none;
    color: var(--quiet, #6a727c);
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
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: var(--radius-lg);
    background: var(--panel, #23272f);
    box-shadow: 0 14px 34px rgb(0 0 0 / 55%);
    color: var(--text, #e9ebee);
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
    color: var(--muted, #aeb5be);
    font: var(--text-sm)/1 var(--font-ui, Bahnschrift, system-ui, sans-serif);
    cursor: pointer;
  }

  .scope button[aria-checked="true"] {
    background: var(--edge);
    color: var(--text, #e9ebee);
  }

  .menu-item:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
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
    font: var(--text-md)/1.35 var(--font-ui, Bahnschrift, system-ui, sans-serif);
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
    color: var(--blueprint-light, #7fb0f7);
  }

  .text {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 2px;
  }

  .hint {
    overflow: hidden;
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .footnote {
    margin: 6px 2px 2px;
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm);
    line-height: 1.4;
  }
</style>
