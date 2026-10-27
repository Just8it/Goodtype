<script lang="ts">
  import { t } from "../i18n";
  import type { PresetSummary } from "../page/presets";
  import PresetMenu from "./PresetMenu.svelte";
  import EditorColorButton from "./EditorColorButton.svelte";
  import { typstSnippets } from "./snippets";
  import type { WritingCommand } from "./writingCommands";

  // One row of things you do to the text. What the panel *is* — its title, and how to close it —
  // lives in the strip above; keeping the two apart is what stops this row growing a third one.
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

  function insert(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    if (select.value) onCommand(`snippet:${select.value}`);
    select.value = "";
  }

  function choose(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    if (select.value) onCommand(select.value as WritingCommand);
    select.value = "";
  }
</script>

<nav class="writing-bar" aria-label={$t("editor.pageTextFormatting")}>
  <div class="tools" role="group" aria-label={$t("editor.textStyle")}>
    <button type="button" aria-label={$t("editor.bold")} title={$t("editor.boldCtrlB")} onclick={() => onCommand("bold")}><b>B</b></button>
    <button type="button" aria-label={$t("editor.emphasis")} title={$t("editor.emphasisCtrlI")} onclick={() => onCommand("italic")}><i>I</i></button>
    <button type="button" aria-label={$t("editor.underline")} title={$t("editor.underlineCtrlU")} onclick={() => onCommand("underline")}><u>U</u></button>
    <EditorColorButton {onCommand} />
  </div>

  <span class="rule" aria-hidden="true"></span>

  <div class="tools" role="group" aria-label={$t("editor.structure")}>
    <!-- The menu is a native select so the keyboard and screen-reader behaviour comes for free;
         the face on top of it is what the eye gets, because a styled select cannot hide the
         platform's own arrow. -->
    <label class="menu-select heading">
      <span class="sr-only">{$t("editor.headingLevel")}</span>
      <span class="face" aria-hidden="true">H<span class="caret">▾</span></span>
      <select aria-label={$t("editor.headingLevel")} title={$t("editor.headingsCtrlAlt")} onchange={choose}>
        <option value="">{$t("editor.headingLevel")}</option>
        <option value="heading-1">{$t("editor.heading")}</option>
        <option value="heading-2">{$t("editor.heading2")}</option>
        <option value="heading-3">{$t("editor.heading3")}</option>
      </select>
    </label>
    <!-- Drawn as marker-plus-lines rather than the bare "•" and "1." glyphs the comp used: at
         28px a lone dot reads as punctuation, not as "turn these lines into a list". -->
    <button type="button" aria-label={$t("editor.bulletList")} title={$t("editor.bulletList")} onclick={() => onCommand("bullet-list")}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle class="solid" cx="4.2" cy="6.5" r="1.5" /><circle class="solid" cx="4.2" cy="12" r="1.5" /><circle class="solid" cx="4.2" cy="17.5" r="1.5" />
        <path d="M9.5 6.5h10.5M9.5 12h10.5M9.5 17.5h10.5" />
      </svg>
    </button>
    <button type="button" aria-label={$t("editor.numberedList")} title={$t("editor.numberedList")} onclick={() => onCommand("numbered-list")}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M10 6.5h10M10 12h10M10 17.5h10" />
        <path d="M3.4 4.9h1.3v4M3.2 8.9h2.4" />
        <path d="M6 19.6H3.2c0-1.2 2.5-2 2.5-3.1 0-.8-.8-1.3-2.2-.8" />
      </svg>
    </button>
    <button class="math" type="button" aria-label={$t("editor.inlineMath")} title={$t("editor.inlineMath")} onclick={() => onCommand("inline-math")}>$x$</button>
  </div>

  <div class="spacer"></div>

  <label class="menu-select insert">
    <span class="sr-only">{$t("editor.insert")}</span>
    <span class="face" aria-hidden="true">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>{$t("editor.insert")}
    </span>
    <select aria-label={$t("editor.insertTypstContent")} title={$t("editor.insertTypstContent")} onchange={insert}>
      <option value="">{$t("editor.insertTypstContent")}</option>
      {#each typstSnippets($t) as snippet (snippet.label)}
        <option value={snippet.label}>{snippet.label} — {snippet.detail}</option>
      {/each}
    </select>
  </label>

  <PresetMenu {source} {presets} {busy} onAction={onPresetAction} />
</nav>

<style>
  .writing-bar {
    display: flex;
    min-height: 42px;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 7px 12px;
    /* No background of its own: the strip above is the lighter chrome, this row sits on the panel
       body, and the rule below separates it from the darker editor. */
    border-bottom: 1px solid var(--edge-soft);
    color: var(--text, #e9ebee);
  }

  .tools {
    display: flex;
    flex: none;
    gap: 2px;
  }

  .spacer {
    flex: 1 1 0;
  }

  button,
  select {
    height: var(--control-dense);
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted, #aeb5be);
    font: var(--text-md)/1 var(--font-ui, Bahnschrift, system-ui, sans-serif);
  }

  .tools button {
    display: grid;
    width: var(--control-dense);
    padding: 0;
    cursor: pointer;
    place-items: center;
  }

  .tools button:hover {
    background: var(--wash);
    color: var(--text, #e9ebee);
  }

  .tools button svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  /* List bullets are shapes, not line work, so they are filled circles rather than a second
     stroke weight smuggled into an icon set that has exactly one. */
  .tools button svg .solid {
    fill: currentColor;
    stroke: none;
  }

  .tools button:focus-visible,
  .menu-select:has(select:focus-visible) .face {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: 1px;
  }

  /* The select carries the behaviour and the face carries the look, so the select sits invisibly
     on top of it: clicks and focus land on the real control, and its dropdown still anchors here. */
  .menu-select {
    position: relative;
    display: inline-flex;
    flex: none;
  }

  .menu-select select {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    cursor: pointer;
    opacity: 0;
  }

  .menu-select .face {
    display: flex;
    height: var(--control-dense);
    align-items: center;
    padding: 0 8px;
    border-radius: var(--radius);
    color: var(--muted, #aeb5be);
    font: var(--text-md)/1 var(--font-ui, Bahnschrift, system-ui, sans-serif);
    gap: 4px;
  }

  .menu-select:hover .face {
    background: var(--wash);
    color: var(--text, #e9ebee);
  }

  .insert .face {
    border: 1px solid var(--edge);
    background: var(--surround, #1b1e24);
    color: var(--text, #e9ebee);
    font-size: var(--text-md);
    gap: 6px;
  }

  .insert:hover .face {
    background: var(--wash);
  }

  .insert svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .caret {
    color: var(--quiet, #6a727c);
    font-size: var(--text-xs);
  }

  option {
    background: var(--panel, #23272f);
    color: var(--text, #e9ebee);
  }

  .math {
    font-family: Cambria, Georgia, serif;
    font-style: italic;
  }

  .rule {
    width: 1px;
    height: 18px;
    flex: none;
    background: var(--edge);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  /* Narrow panel: the preset drops to its own line rather than squeezing the tools. */
  @media (max-width: 390px) {
    .spacer {
      flex-basis: 100%;
    }
  }
</style>
