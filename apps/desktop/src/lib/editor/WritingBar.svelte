<script lang="ts">
  import type { PresetSummary } from "../page/presets";
  import { pagePresetOptions, pagePresetPath, pagePresetState } from "../page/presets";
  import { TYPST_SNIPPETS } from "./snippets";
  import type { WritingCommand } from "./writingCommands";

  let {
    source,
    presets = [],
    busy = false,
    onCommand,
    onPresetAction,
  }: {
    source: string;
    presets?: PresetSummary[];
    busy?: boolean;
    onCommand: (command: WritingCommand) => void;
    onPresetAction: (action: string) => void;
  } = $props();

  const path = $derived(pagePresetPath(source));
  const defaultPreset = $derived(presets.find((preset) => preset.kind === "default"));
  const pagePresets = $derived(pagePresetOptions(presets));
  const state = $derived(pagePresetState(source));
  const value = $derived(state.kind === "managed" && path ? `path:${path}` : state.kind === "custom" ? "custom" : "none");

  function choose(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    onPresetAction(select.value);
  }

  function insert(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    if (select.value) onCommand(`snippet:${select.value}`);
    select.value = "";
  }
</script>

<nav class="writing-bar" aria-label="Page text formatting">
  <label class="preset">
    <span class="sr-only">Page preset</span>
    <select aria-label="Page preset" disabled={busy} {value} onchange={choose}>
      {#if defaultPreset}
        <option value="path:/styles/default.typ">Preset: {defaultPreset.name}</option>
      {/if}
      {#if value === "custom"}<option value="custom" disabled>Preset: Custom source</option>{/if}
      <option value="none">Preset: None</option>
      <optgroup label="Page override">
        {#each pagePresets as preset (preset.id)}
          <option value={preset.importPath ? `path:${preset.importPath}` : `page:${preset.id}`}>
            {preset.name}
          </option>
        {/each}
        <option value="page:import">Import .typ…</option>
      </optgroup>
      <optgroup label="Change notebook default">
        {#each presets.filter((preset) => preset.kind === "builtin") as preset (preset.id)}
          <option value={`default:${preset.id}`}>{preset.name}</option>
        {/each}
        <option value="default:import">Import .typ…</option>
      </optgroup>
    </select>
  </label>

  <span class="rule" aria-hidden="true"></span>
  <div class="tools" role="group" aria-label="Text style">
    <button type="button" aria-label="Bold" title="Bold (Ctrl+B)" onclick={() => onCommand("bold")}><b>B</b></button>
    <button type="button" aria-label="Emphasis" title="Emphasis (Ctrl+I)" onclick={() => onCommand("italic")}><i>I</i></button>
    <button type="button" aria-label="Underline" title="Underline (Ctrl+U)" onclick={() => onCommand("underline")}><u>U</u></button>
  </div>
  <span class="rule" aria-hidden="true"></span>
  <label class="compact-select">
    <span class="sr-only">Heading level</span>
    <select aria-label="Heading level" title="Headings (Ctrl+Alt+1–3)" onchange={(event) => {
      const target = event.currentTarget;
      if (target.value) onCommand(target.value as WritingCommand);
      target.value = "";
    }}>
      <option value="">H ▾</option>
      <option value="heading-1">Heading 1</option>
      <option value="heading-2">Heading 2</option>
      <option value="heading-3">Heading 3</option>
    </select>
  </label>
  <button type="button" aria-label="Bullet list" title="Bullet list" onclick={() => onCommand("bullet-list")}>•</button>
  <button type="button" aria-label="Numbered list" title="Numbered list" onclick={() => onCommand("numbered-list")}>1.</button>
  <span class="rule" aria-hidden="true"></span>
  <button class="math" type="button" aria-label="Inline math" title="Inline math" onclick={() => onCommand("inline-math")}>$x$</button>
  <label class="insert">
    <span class="sr-only">Insert</span>
    <select aria-label="Insert Typst content" title="Insert Typst content" onchange={insert}>
      <option value="">Insert ▾</option>
      {#each TYPST_SNIPPETS as snippet (snippet.label)}
        <option value={snippet.label}>{snippet.label} — {snippet.detail}</option>
      {/each}
    </select>
  </label>
</nav>

<style>
  .writing-bar {
    display: flex;
    min-height: 38px;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    padding: 4px 6px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    background: #20242b;
    color: #eef1f4;
  }
  .tools { display: flex; gap: 2px; }
  button, select {
    height: 28px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #eef1f4;
    font: 12px/1 var(--font-ui, Bahnschrift, system-ui, sans-serif);
  }
  button { min-width: 28px; padding: 0 7px; cursor: pointer; }
  select { max-width: 100%; padding: 0 7px; cursor: pointer; }
  option, optgroup { background: #252a32; color: #eef1f4; }
  button:hover, select:hover { background: rgb(255 255 255 / 8%); }
  button:focus-visible, select:focus-visible { outline: 2px solid #4c8df0; outline-offset: 1px; }
  .preset { min-width: 0; flex: 1 1 148px; }
  .preset select { width: 100%; text-overflow: ellipsis; }
  .compact-select select { width: 49px; }
  .insert { margin-left: auto; }
  .math { font-family: Cambria, Georgia, serif; font-style: italic; }
  .rule { width: 1px; height: 20px; margin: 0 3px; background: rgb(255 255 255 / 12%); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  @media (max-width: 390px) {
    .preset { flex-basis: 100%; }
    .preset select { width: 100%; }
  }
</style>
