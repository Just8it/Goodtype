<script lang="ts">
  import { t } from "../i18n";
  import { onMount } from "svelte";
  import Dialog from "../workspace/Dialog.svelte";
  import { errorDetails } from "../ipc/errors";
  import { listTypstPresets, pickTypstPreset } from "../ipc/presets";
  import { presetPresentation } from "../page/presets";
  import type { NotebookSetup, PresetChoice, PresetSummary } from "../page/presets";
  import { DEFAULT_PAGE_SIZE, PAGE_SIZES, pageSizes, geometryOf, type Orientation } from "../page/sizes";
  import { PAPER_TONES, paperTones, templateGroups } from "../page/templates";
  import { nameProblem } from "./library";
  import { isPageListSyntax } from "../workspace/exportTarget";

  let { busy = false, onConfirm, onCancel }: {
    busy?: boolean;
    onConfirm: (setup: NotebookSetup) => void;
    onCancel: () => void;
  } = $props();

  let name = $state("");
  let sizeId = $state(DEFAULT_PAGE_SIZE.id);
  let orientation = $state<Orientation>("portrait");
  let fromPdf = $state(false);
  let pdfPages = $state("");
  let toneId = $state(PAPER_TONES[0].id);
  let templateId = $state("blank-white");
  let preset = $state<PresetChoice>({ kind: "none" });
  let presetValue = $state("none");
  let presets = $state<PresetSummary[]>([]);
  let failure = $state("");
  let field = $state<HTMLInputElement>();

  const tone = $derived(PAPER_TONES.find((item) => item.id === toneId) ?? PAPER_TONES[0]);
  const templates = $derived(templateGroups(tone, $t).flatMap((group) => group.templates));
  const problem = $derived(name ? nameProblem(name, $t) : null);
  const pdfPagesValid = $derived(!fromPdf || isPageListSyntax(pdfPages));
  const ready = $derived(Boolean(name) && !problem && !busy && pdfPagesValid);

  onMount(async () => {
    field?.focus();
    try { presets = (await listTypstPresets()).filter((item) => item.kind === "builtin"); }
    catch { failure = $t("library.typstPresetsAreNotAvailableRightNow"); }
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
        failure = errorDetails(error);
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
      fromPdf,
      ...(fromPdf && pdfPages.trim() ? { pdfPages: pdfPages.trim() } : {}),
    });
  }
</script>

<Dialog title={$t("library.newNotebook")} lead={$t("library.setUpThePaperTheWayYou")} size="lg" onClose={onCancel}>
  {#snippet children()}
    <form id="notebook-setup-form" onsubmit={submit}>
      <label class="wide">{$t("library.name")}
      <input bind:this={field} bind:value={name} maxlength="80" autocomplete="off" spellcheck="false" aria-invalid={problem !== null} aria-describedby="notebook-name-help" />
      <small id="notebook-name-help" class:error={problem}>{problem ?? $t("library.savedLocallyInYourGoodtypeLibrary")}</small>
      </label>

      <div class="fields">
      <fieldset class="wide">
        <legend>{$t("library.startWith")}</legend>
        <div class="segmented">
          <button type="button" class:active={!fromPdf} aria-pressed={!fromPdf} onclick={() => (fromPdf = false)}>{$t("library.blankPaper")}</button>
          <button type="button" class:active={fromPdf} aria-pressed={fromPdf} onclick={() => (fromPdf = true)}>{$t("library.pdfPages")}</button>
        </div>
        {#if fromPdf}<small>{$t("library.pdfStartHelp")}</small>{/if}
      </fieldset>
      {#if fromPdf}
      <label class="wide">{$t("library.pdfPageList")}
        <input bind:value={pdfPages} type="text" inputmode="numeric" spellcheck="false" autocomplete="off" placeholder={$t("library.pdfPageListPlaceholder")} aria-invalid={!pdfPagesValid} aria-describedby="notebook-pdf-pages-help" />
        <small id="notebook-pdf-pages-help" class:error={!pdfPagesValid}>{pdfPagesValid ? $t("library.pdfPageListHelp") : $t("library.pdfPageListInvalid")}</small>
      </label>
      {/if}
      <!-- A PDF brings its own page sizes, so the paper controls only apply to blank paper. -->
      {#if !fromPdf}
      <label>{$t("library.pageSize")}
        <select bind:value={sizeId}>{#each pageSizes($t) as size}<option value={size.id}>{size.name} · {size.detail}</option>{/each}</select>
      </label>
      <fieldset>
        <legend>{$t("library.orientation")}</legend>
        <div class="segmented">
          <button type="button" class:active={orientation === "portrait"} aria-pressed={orientation === "portrait"} onclick={() => (orientation = "portrait")}>{$t("library.portrait")}</button>
          <button type="button" class:active={orientation === "landscape"} aria-pressed={orientation === "landscape"} onclick={() => (orientation = "landscape")}>{$t("library.landscape")}</button>
        </div>
      </fieldset>
      <label>{$t("library.paperColour")}
        <select bind:value={toneId}>{#each paperTones($t) as paper}<option value={paper.id}>{paper.name}</option>{/each}</select>
      </label>
      <label>{$t("library.paperTemplate")}
        <select bind:value={templateId}>{#each templates as template}<option value={template.id}>{template.name}</option>{/each}</select>
      </label>
      {/if}
      <label class="wide">{$t("library.typstPreset")}
        <select value={presetValue} onchange={(event) => void choosePreset(event.currentTarget.value)}>
          <option value="none">{$t("library.none")}</option>
          {#each presets as item}<option value={item.id}>{presetPresentation(item, $t).name} · {presetPresentation(item, $t).description}</option>{/each}
          <option value="import">{$t("library.importTypFile")}</option>
        </select>
        <small>{$t("library.appliedTheFirstTimePageTextIs")}</small>
      </label>
      </div>

      {#if failure}<p class="failure" role="alert">{failure}</p>{/if}
    </form>
  {/snippet}
  {#snippet actions()}
    <button type="button" class="control" disabled={busy} onclick={onCancel}>{$t("library.cancel")}</button>
    <button type="submit" form="notebook-setup-form" class="primary" disabled={!ready}>{fromPdf ? $t("library.createAndChoosePdf") : $t("library.createNotebook")}</button>
  {/snippet}
</Dialog>

<style>
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 14px; }
  label, fieldset { display: grid; gap: 6px; min-width: 0; margin: 0; padding: 0; border: 0; color: var(--muted); font-size: var(--text-sm); }
  .wide { grid-column: 1 / -1; }
  input, select { box-sizing: border-box; width: 100%; height: var(--control); padding: 0 10px; border: 1px solid var(--edge); border-radius: var(--radius); background-color: rgb(0 0 0 / 25%); color: var(--text); font: var(--text-md) var(--font-ui); }
  select { padding-right: 34px; appearance: none; background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23aeb5be' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m7 9.5 5 5 5-5'/%3E%3C/svg%3E"); background-repeat: no-repeat; background-position: right 10px center; }
  option { background: var(--panel); }
  input:focus-visible, select:focus-visible, button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  input:hover, select:hover { border-color: var(--edge-strong); }
  input[aria-invalid="true"] { border-color: var(--oxide); }
  small { min-height: 1.2em; color: var(--quiet); }
  small.error, .failure { color: var(--oxide); }
  .segmented { display: flex; height: var(--control); padding: 3px; border: 1px solid var(--edge); border-radius: var(--radius); background: rgb(0 0 0 / 25%); gap: 3px; }
  .segmented button { flex: 1; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; }
  .segmented button:hover { background: var(--wash); color: var(--text); }
  .segmented button.active { background: var(--accent-fill); box-shadow: inset 0 0 0 1px var(--accent-edge); color: var(--text); }
  .failure { margin: 12px 0 0; font-size: var(--text-sm); }
  @media (max-width: 520px) { .fields { grid-template-columns: 1fr; } .wide { grid-column: auto; } }
  @media (pointer: coarse) { input, select, .segmented { min-height: var(--control-touch); } }
</style>
