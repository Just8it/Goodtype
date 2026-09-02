<script lang="ts">
  import { onMount } from "svelte";
  import { focusTrap } from "../workspace/focus";
  import { listTypstPresets, pickTypstPreset } from "../ipc/presets";
  import type { NotebookSetup, PresetChoice, PresetSummary } from "../page/presets";
  import { DEFAULT_PAGE_SIZE, PAGE_SIZES, geometryOf, type Orientation } from "../page/sizes";
  import { PAPER_TONES, templateGroups } from "../page/templates";
  import { nameProblem } from "./library";

  let { busy = false, onConfirm, onCancel }: {
    busy?: boolean;
    onConfirm: (setup: NotebookSetup) => void;
    onCancel: () => void;
  } = $props();

  let name = $state("");
  let sizeId = $state(DEFAULT_PAGE_SIZE.id);
  let orientation = $state<Orientation>("portrait");
  let toneId = $state(PAPER_TONES[0].id);
  let templateId = $state("blank-white");
  let preset = $state<PresetChoice>({ kind: "none" });
  let presetValue = $state("none");
  let presets = $state<PresetSummary[]>([]);
  let failure = $state("");
  let field = $state<HTMLInputElement>();

  const tone = $derived(PAPER_TONES.find((item) => item.id === toneId) ?? PAPER_TONES[0]);
  const templates = $derived(templateGroups(tone).flatMap((group) => group.templates));
  const problem = $derived(name ? nameProblem(name) : null);
  const ready = $derived(Boolean(name) && !problem && !busy);

  onMount(async () => {
    field?.focus();
    try { presets = (await listTypstPresets()).filter((item) => item.kind === "builtin"); }
    catch { failure = "Typst-Voreinstellungen sind gerade nicht verfügbar."; }
  });

  $effect(() => {
    if (!templates.some((template) => template.id === templateId)) templateId = templates[0]?.id ?? "";
  });

  async function choosePreset(value: string) {
    presetValue = value;
    failure = "";
    if (value === "none") { preset = { kind: "none" }; return; }
    if (value === "import") {
      try {
        const picked = await pickTypstPreset();
        if (picked) preset = picked;
        else presetValue = preset.kind === "builtin" ? preset.id : preset.kind === "imported" ? "import" : "none";
      } catch (error) {
        failure = error instanceof Error ? error.message : String(error);
        presetValue = "none";
        preset = { kind: "none" };
      }
      return;
    }
    preset = { kind: "builtin", id: value };
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!ready) return;
    const size = PAGE_SIZES.find((item) => item.id === sizeId) ?? DEFAULT_PAGE_SIZE;
    const template = templates.find((item) => item.id === templateId) ?? templates[0];
    if (!template) return;
    onConfirm({
      name,
      geometry: geometryOf(size, orientation),
      background: { kind: "template", template },
      preset,
    });
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === "Escape") onCancel(); }} />
<div class="scrim" role="presentation" onpointerdown={(event) => { if (event.target === event.currentTarget) onCancel(); }}>
  <div use:focusTrap class="setup" role="dialog" aria-modal="true" aria-labelledby="notebook-setup-title" tabindex="-1">
    <form onsubmit={submit}>
      <header>
      <h2 id="notebook-setup-title">Neues Notizbuch</h2>
      <p>Richte das Papier so ein, wie du am liebsten arbeitest.</p>
      </header>

      <label class="wide">Name
      <input bind:this={field} bind:value={name} maxlength="80" autocomplete="off" spellcheck="false" aria-invalid={problem !== null} aria-describedby="notebook-name-help" />
      <small id="notebook-name-help" class:error={problem}>{problem ?? "Lokal in deiner Goodtype-Bibliothek gespeichert"}</small>
      </label>

      <div class="fields">
      <label>Seitengröße
        <select bind:value={sizeId}>{#each PAGE_SIZES as size}<option value={size.id}>{size.name} · {size.detail}</option>{/each}</select>
      </label>
      <fieldset>
        <legend>Ausrichtung</legend>
        <div class="segmented">
          <button type="button" class:active={orientation === "portrait"} aria-pressed={orientation === "portrait"} onclick={() => (orientation = "portrait")}>Hochformat</button>
          <button type="button" class:active={orientation === "landscape"} aria-pressed={orientation === "landscape"} onclick={() => (orientation = "landscape")}>Querformat</button>
        </div>
      </fieldset>
      <label>Papierfarbe
        <select bind:value={toneId}>{#each PAPER_TONES as paper}<option value={paper.id}>{paper.name}</option>{/each}</select>
      </label>
      <label>Papiervorlage
        <select bind:value={templateId}>{#each templates as template}<option value={template.id}>{template.name}</option>{/each}</select>
      </label>
      <label class="wide">Typst-Voreinstellung
        <select value={presetValue} onchange={(event) => void choosePreset(event.currentTarget.value)}>
          <option value="none">Keine</option>
          {#each presets as item}<option value={item.id}>{item.name} · {item.description}</option>{/each}
          <option value="import">.typ-Datei importieren…</option>
        </select>
        <small>Wird beim ersten Öffnen von Seitentext angewendet.</small>
      </label>
      </div>

      {#if failure}<p class="failure" role="alert">{failure}</p>{/if}
      <footer>
        <button type="button" class="quiet" disabled={busy} onclick={onCancel}>Abbrechen</button>
        <button type="submit" class="primary" disabled={!ready}>Notizbuch anlegen</button>
      </footer>
    </form>
  </div>
</div>

<style>
  .scrim { position: absolute; z-index: 60; display: grid; padding: 20px; background: rgb(10 12 16 / 62%); inset: 0; place-items: center; animation: scrim-in 140ms ease-out; }
  .setup { width: min(620px, 100%); max-height: 100%; padding: 22px; border: 1px solid var(--edge, rgb(255 255 255 / 12%)); border-radius: var(--radius-lg, 10px); background: var(--panel, #23272f); box-shadow: 0 18px 44px rgb(0 0 0 / 55%); color: var(--text, #e9ebee); overflow-y: auto; animation: setup-in 160ms cubic-bezier(0.16, 1, 0.3, 1); }
  header { margin-bottom: 18px; }
  header p { margin: 5px 0 0; color: var(--muted, #aeb5be); font-size: var(--text-md, 13px); }
  h2 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -0.02em; }
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 14px; }
  label, fieldset { display: grid; gap: 6px; min-width: 0; margin: 0; padding: 0; border: 0; color: var(--muted, #aeb5be); font-size: var(--text-sm, 11px); }
  .wide { grid-column: 1 / -1; }
  input, select { box-sizing: border-box; width: 100%; height: var(--control, 36px); padding: 0 10px; border: 1px solid var(--edge, rgb(255 255 255 / 12%)); border-radius: var(--radius, 6px); background-color: rgb(0 0 0 / 25%); color: var(--text, #e9ebee); font: var(--text-md, 13px) var(--font-ui, Bahnschrift, system-ui, sans-serif); }
  select { padding-right: 34px; appearance: none; background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23aeb5be' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m7 9.5 5 5 5-5'/%3E%3C/svg%3E"); background-repeat: no-repeat; background-position: right 10px center; }
  option { background: var(--panel, #23272f); }
  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--blueprint-light, #7fb0f7); outline-offset: 1px; }
  input:hover, select:hover { border-color: rgb(255 255 255 / 20%); }
  input[aria-invalid="true"] { border-color: var(--oxide, #e5645e); }
  small { min-height: 1.2em; color: var(--quiet, #6a727c); }
  small.error, .failure { color: var(--oxide, #e5645e); }
  .segmented { display: flex; height: var(--control, 36px); padding: 3px; border: 1px solid var(--edge, rgb(255 255 255 / 12%)); border-radius: var(--radius, 6px); background: rgb(0 0 0 / 25%); gap: 3px; }
  .segmented button { flex: 1; border: 0; border-radius: var(--radius, 6px); background: transparent; color: var(--muted, #aeb5be); cursor: pointer; }
  .segmented button:hover { background: var(--wash, rgb(255 255 255 / 8%)); color: var(--text, #e9ebee); }
  .segmented button.active { background: rgb(76 141 240 / 16%); box-shadow: inset 0 0 0 1px rgb(76 141 240 / 52%); color: var(--text, #e9ebee); }
  .failure { margin: 12px 0 0; font-size: var(--text-sm, 11px); }
  footer { display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }
  footer button { min-height: var(--control, 36px); padding: 0 15px; border: 0; border-radius: var(--radius, 6px); color: var(--text, #e9ebee); font: inherit; cursor: pointer; touch-action: manipulation; }
  .quiet { background: rgb(255 255 255 / 5%); }
  .quiet:hover { background: var(--wash, rgb(255 255 255 / 8%)); }
  .primary { background: var(--blueprint, #4c8df0); color: #0e1b31; font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--blueprint-light, #7fb0f7); }
  button:disabled { opacity: .5; cursor: default; }
  @keyframes scrim-in { from { opacity: 0; } }
  @keyframes setup-in { from { opacity: 0; transform: translateY(7px) scale(.99); } }
  @media (max-width: 520px) { .fields { grid-template-columns: 1fr; } .wide { grid-column: auto; } .setup { padding: 18px; } }
  @media (pointer: coarse) { input, select, .segmented, footer button { min-height: var(--control-touch, 44px); } }
  @media (prefers-reduced-motion: reduce) { .scrim, .setup { animation: none; } }
</style>
