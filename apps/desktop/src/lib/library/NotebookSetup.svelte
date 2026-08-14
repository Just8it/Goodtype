<script lang="ts">
  import { onMount } from "svelte";
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
    catch { failure = "Typst presets are unavailable"; }
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
  <form class="setup" onsubmit={submit}>
    <header>
      <p>NEW NOTEBOOK</p>
      <h2>Set up your paper</h2>
    </header>

    <label class="wide">Name
      <input bind:this={field} bind:value={name} maxlength="80" autocomplete="off" spellcheck="false" aria-invalid={problem !== null} />
      <small class:error={problem}>{problem ?? "Stored locally in your Goodtype library"}</small>
    </label>

    <div class="fields">
      <label>Page size
        <select bind:value={sizeId}>{#each PAGE_SIZES as size}<option value={size.id}>{size.name} · {size.detail}</option>{/each}</select>
      </label>
      <fieldset>
        <legend>Orientation</legend>
        <div class="segmented">
          <button type="button" class:active={orientation === "portrait"} onclick={() => (orientation = "portrait")}>Portrait</button>
          <button type="button" class:active={orientation === "landscape"} onclick={() => (orientation = "landscape")}>Landscape</button>
        </div>
      </fieldset>
      <label>Paper tone
        <select bind:value={toneId}>{#each PAPER_TONES as paper}<option value={paper.id}>{paper.name}</option>{/each}</select>
      </label>
      <label>Paper template
        <select bind:value={templateId}>{#each templates as template}<option value={template.id}>{template.name}</option>{/each}</select>
      </label>
      <label class="wide">Typst preset
        <select value={presetValue} onchange={(event) => void choosePreset(event.currentTarget.value)}>
          <option value="none">None</option>
          {#each presets as item}<option value={item.id}>{item.name} · {item.description}</option>{/each}
          <option value="import">Import .typ…</option>
        </select>
        <small>Applied only when you first open Page text.</small>
      </label>
    </div>

    {#if failure}<p class="failure" role="alert">{failure}</p>{/if}
    <footer>
      <button type="button" class="quiet" disabled={busy} onclick={onCancel}>Cancel</button>
      <button type="submit" class="primary" disabled={!ready}>Create notebook</button>
    </footer>
  </form>
</div>

<style>
  .scrim { position: absolute; z-index: 60; display: grid; background: rgb(0 0 0 / 52%); inset: 0; place-items: center; }
  .setup { width: min(620px, calc(100vw - 40px)); padding: 22px; border: 1px solid rgb(255 255 255 / 12%); border-radius: 14px; background: #23272f; box-shadow: 0 24px 60px rgb(0 0 0 / 60%); color: #e9ebee; }
  header { margin-bottom: 18px; }
  header p { margin: 0 0 4px; color: #7fb0f7; font-size: 10px; font-weight: 700; letter-spacing: .12em; }
  h2 { margin: 0; font-size: 20px; }
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 14px; }
  label, fieldset { display: grid; gap: 6px; min-width: 0; margin: 0; padding: 0; border: 0; color: #aeb5be; font-size: 11px; }
  .wide { grid-column: 1 / -1; }
  input, select { box-sizing: border-box; width: 100%; height: 38px; padding: 0 10px; border: 1px solid rgb(255 255 255 / 14%); border-radius: 8px; background: #171a20; color: #eef1f4; font: 13px var(--font-ui, Bahnschrift, system-ui, sans-serif); }
  option { background: #23272f; }
  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid #4c8df0; outline-offset: 2px; }
  small { min-height: 1.2em; color: #737d89; }
  small.error, .failure { color: #f08c82; }
  .segmented { display: flex; height: 38px; padding: 3px; border: 1px solid rgb(255 255 255 / 14%); border-radius: 8px; background: #171a20; }
  .segmented button { flex: 1; border: 0; border-radius: 5px; background: transparent; color: #aeb5be; }
  .segmented button.active { background: #34445d; color: #fff; }
  .failure { margin: 12px 0 0; font-size: 12px; }
  footer { display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }
  footer button { height: 36px; padding: 0 15px; border: 0; border-radius: 8px; color: #eef1f4; font: inherit; cursor: pointer; }
  .quiet { background: rgb(255 255 255 / 7%); }
  .primary { background: #4c8df0; color: #0e1b31; font-weight: 700; }
  button:disabled { opacity: .5; cursor: default; }
  @media (max-width: 520px) { .fields { grid-template-columns: 1fr; } .wide { grid-column: auto; } }
</style>
