<script lang="ts">
  import { focusPrompt, type EditorFocus } from "./editorEntry";
  import type { Snippet } from "svelte";
  import { t, formatNumber } from "../i18n";
  import type { CodexStatus } from "../ipc/helper";
  import { canSend, composerKey, PROMPT_LIMIT } from "./codex";
  let { regionContext = false, blockContext = false, cellContext = false, focus = null, status, count, model = $bindable(null), webSearch = $bindable(true), busy = false, followUp = false, thumbnail, selectionLabel, context, disclosure, initialPrompt = "", focusVersion = 0, promptVersion = 0, details = "", running = false, stopping = false, ending = false, onStop, onEnd, onSend, onClose }:
    { regionContext?: boolean; blockContext?: boolean; cellContext?: boolean; focus?: EditorFocus | null; status: CodexStatus; count: number; model?: string | null; webSearch?: boolean; busy?: boolean; followUp?: boolean; thumbnail?: Snippet; selectionLabel?: string; context?: Snippet; disclosure?: string; initialPrompt?: string; focusVersion?: number; promptVersion?: number; details?: string; running?: boolean; stopping?: boolean; ending?: boolean; onStop?: () => void; onEnd?: () => void; onSend: (prompt: string) => Promise<boolean>; onClose: () => void } = $props();
  const id = $props.id();
  let prompt = $state("");
  let form = $state<HTMLFormElement>();
  let input = $state<HTMLTextAreaElement>();
  let activeFocus = $state<EditorFocus | null>(null);
  $effect(() => { focusVersion; activeFocus = focus; void import("svelte").then(({ tick }) => tick().then(() => input?.focus())); });
  $effect(() => { focusVersion; promptVersion; if (initialPrompt) { prompt = initialPrompt; void import("svelte").then(({ tick }) => tick().then(() => input?.focus())); } });
  const promptBudget = $derived(Math.max(0, PROMPT_LIMIT - focusPrompt("", activeFocus).length - (cellContext && activeFocus ? 1 : 0)));
  async function send() { if (!canSend(prompt, busy) || prompt.length > promptBudget) return; const captured = prompt; if (await onSend(cellContext && activeFocus ? focusPrompt(captured, activeFocus).replace(/(`{3,})typst\n/, "$1python\n") : focusPrompt(captured, activeFocus)) && prompt === captured) prompt = ""; }
</script>
<svelte:window onkeydown={event => { if (event.key === "Escape" && form?.contains(event.target as Node)) composerKey(event, send, onClose); }} />
<form bind:this={form} onsubmit={event => { event.preventDefault(); send(); }}>
  <div class="context-block">
  {#if count > 0 || selectionLabel}<div class="context-row">
    {#if thumbnail}
      <button class="attachment" type="button" popovertarget={id + "-selection"} title={$t("codex.selectionPreview")} aria-label={$t("codex.attachments", { count })} aria-describedby={selectionLabel ? id + "-attachment" : undefined}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m8 13 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l8-8" /></svg><span id={id + "-attachment"} class="attachment-text">{selectionLabel || $t("codex.attachments", { count })}</span>
      </button>
      <div id={id + "-selection"} class="selection-preview" popover="auto"><p>{$t("codex.selectionPreview")}</p>{@render thumbnail()}</div>
    {:else}
      <span class="attachment"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m8 13 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l8-8" /></svg><span class="attachment-text">{selectionLabel || $t("codex.attachments", { count })}</span></span>
    {/if}
  </div>{/if}
  {#if activeFocus}<button class="attachment" type="button" title={$t("aiEntry.remove")} aria-label={$t("aiEntry.remove")} onclick={() => activeFocus = null}><span>{$t("aiEntry.lines", { count: activeFocus.toLine - activeFocus.fromLine + 1, from: activeFocus.fromLine, to: activeFocus.toLine })}</span><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/if}
  {#if context}{@render context()}{/if}
  </div>
  <div class="prompt-box">
    <textarea bind:this={input} class:followup={followUp} aria-label={$t(followUp ? "codex.followUp" : "codex.prompt")} rows={followUp ? 2 : 3} maxlength={promptBudget} bind:value={prompt}
      placeholder={$t(followUp ? "codex.followUpHint" : cellContext ? "codex.promptCells" : blockContext ? "codex.promptBlock" : regionContext ? "codex.promptRegion" : "codex.promptHint")}
      onkeydown={event => { if (event.key !== "Escape") composerKey(event, send, onClose); }}></textarea>
    <div class="input-actions">
      <label class="select-shell"><span class="sr-only">{$t("codex.model")}</span>
        <select bind:value={model} disabled={busy || followUp} title={followUp ? $t("codex.fixedOptions") : model ? status.models.find(item => item.id === model)?.label : $t("codex.defaultModelDescription")}>
          <option value={null}>{$t("codex.defaultModel")}</option>
          {#each status.models as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
        </select><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
      </label>
      <label class="web" title={followUp ? $t("codex.fixedOptions") : $t("codex.web")}>
        <input type="checkbox" role="switch" bind:checked={webSearch} disabled={busy || followUp} />
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8" /><path d="M4 12h16M12 4c5 4 5 12 0 16-5-4-5-12 0-16" /></svg>
        <span class="sr-only">{$t("codex.web")}</span>
      </label>
      {#if running && onStop}<button class="stop" type="button" disabled={stopping} onclick={onStop}>{$t("codex.stop")}<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1" /></svg></button>
      {:else}<button class="primary" type="submit" disabled={!canSend(prompt, busy) || prompt.length > promptBudget}>{$t("codex.send")}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5m-5 5 5-5 5 5" /></svg></button>{/if}
    </div>
  </div>
  {#if prompt.length > promptBudget}<p class="handoff" role="status">{$t("aiEntry.focusLimit")}</p>{/if}
  <div class="footnote">
    <p class="handoff">{$t("codex.experimental")} · {disclosure} <button class="link" type="button" popovertarget={id + "-details"}>{$t("codex.handoffDetails")}</button></p>
    {#if prompt.length}<small>{$t("codex.limit", { length: formatNumber(prompt.length) })}</small>{/if}
    {#if onEnd}<button class="end" type="button" title={$t("proposal.lifetime")} disabled={ending} onclick={onEnd}>{$t("codex.endConversation")}</button>{/if}
  </div>
  <div id={id + "-details"} class="details" popover="auto"><p>{$t("codex.experimentalHint")}</p><p>{details} {$t("codex.disclosure")}</p><p>{$t("codex.packageDownloads")}</p></div>
</form>
<style>
  .context-block { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding: 2px; margin: -2px; }
  .handoff { color: var(--muted); font-size: var(--text-sm); line-height: 1.5; overflow-wrap: anywhere; }
  form { display: grid; gap: 7px; min-width: 0; }
  button, select, textarea { font: inherit; color: var(--text); }
  button { cursor: pointer; }
  svg { width: var(--icon-dense); height: var(--icon-dense); flex: none; fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; stroke-linejoin: round; }
  .context-row { display: flex; align-items: center; justify-content: space-between; min-width: 0; gap: 8px; }
  .attachment { display: inline-flex; align-items: center; gap: 6px; min-width: 0; min-height: var(--control-dense); padding: 0 6px; border: 1px solid var(--edge); border-radius: var(--radius); background: var(--panel); color: var(--muted); font-size: var(--text-sm); text-align: left; justify-self: start; max-width: 100%; }
  button.attachment:hover { background: var(--wash); color: var(--text); }
  .attachment-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .prompt-box { min-width: 0; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--charcoal); }
  .prompt-box:focus-within { border-color: var(--blueprint); }
  textarea { display: block; width: 100%; min-height: 76px; max-height: min(160px, 22dvh); padding: 12px; border: 0; border-radius: var(--radius-lg); background: transparent; line-height: 1.6; resize: vertical; caret-color: var(--blueprint-light); }
  textarea:focus-visible { outline: none; }
  textarea.followup { min-height: 56px; }
  textarea::placeholder { color: var(--quiet); }
  textarea::selection { background: var(--accent-select); }
  .input-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-width: 0; padding: 6px; }
  .select-shell { position: relative; display: flex; flex: 1; min-width: 110px; max-width: 185px; }
  select { width: 100%; min-width: 0; height: var(--control); padding: 0 24px 0 6px; border: 0; border-radius: var(--radius); appearance: none; background: transparent; color: var(--muted); color-scheme: dark; font-size: var(--text-sm); text-overflow: ellipsis; cursor: pointer; }
  option { color: var(--text); background: var(--panel); }
  .select-shell svg { position: absolute; right: 4px; top: 50%; transform: translateY(-50%); color: var(--muted); pointer-events: none; }
  .web { position: relative; display: flex; align-items: center; justify-content: center; width: var(--control); height: var(--control); flex: none; border-radius: var(--radius); color: var(--muted); font-size: var(--text-sm); cursor: pointer; }
  .web { border: 1px solid transparent; }
  .web:hover:not(:has(input:disabled)) { background: var(--wash); color: var(--text); }
  .web:has(input:checked) { border-color: var(--accent-edge); background: var(--accent-fill); color: var(--text); }
  .web input { position: absolute; inset: 0; margin: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; }
  .web:has(input:focus-visible) { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .primary { display: inline-flex; align-items: center; justify-content: center; flex: none; gap: 6px; min-height: var(--control); margin-left: auto; padding: 0 14px; border: 0; border-radius: var(--radius); background: var(--blueprint); color: var(--on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--blueprint-hover); }
  button:disabled, select:disabled, .web:has(input:disabled) { opacity: var(--disabled); cursor: not-allowed; }
  .footnote { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: baseline; gap: 4px 12px; color: var(--muted); font-size: var(--text-sm); line-height: 1.5; }
  .footnote .handoff { flex: 1 1 200px; color: var(--quiet); }
  .link { padding: 0; border: 0; background: none; color: var(--muted); font-size: inherit; text-decoration: underline; text-underline-offset: 2px; }
  .link:hover { color: var(--text); }
  .end { flex: none; min-height: var(--control-dense); padding: 0 8px; border: 0; border-radius: var(--radius); background: transparent; color: var(--oxide-text); font-size: var(--text-sm); }
  .end:hover:not(:disabled) { background: var(--wash); }
  .stop { display: inline-flex; align-items: center; justify-content: center; flex: none; gap: 6px; min-height: var(--control); margin-left: auto; padding: 0 14px; border: 0; border-radius: var(--radius); background: var(--wash); color: var(--text); font-weight: 600; }
  .stop:hover:not(:disabled) { background: var(--wash-strong); }
  .details { position: fixed; inset: auto 12px 70px auto; width: min(320px, calc(100vw - 24px)); margin: 0; padding: 12px; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--muted); box-shadow: var(--shadow-menu); font-size: var(--text-sm); line-height: 1.5; }
  .details p + p { margin-top: 8px; }
  p { margin: 0; }
  small { font-size: inherit; font-variant-numeric: tabular-nums; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .selection-preview { position: fixed; inset: auto 12px 70px auto; width: min(320px, calc(100vw - 24px)); max-height: min(240px, 50dvh); margin: 0; overflow: auto; padding: 12px; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--text); box-shadow: var(--shadow-menu); font-size: var(--text-sm); }
  .selection-preview p { margin-bottom: 8px; color: var(--muted); }
  .selection-preview :global(svg) { display: block; width: 100%; height: 120px; background: var(--paper); }
  button:focus-visible, select:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  @media (max-height: 500px) {
    form { gap: 6px; }
    .context-block { max-height: 112px; overflow-y: auto; }
    textarea, textarea.followup { min-height: 40px; height: 40px; padding: 8px 12px; }
    .footnote { font-size: var(--text-sm); }
  }
  @media (pointer: coarse) { .primary, select, .web, .attachment { min-height: var(--control-touch); } .web { height: var(--control-touch); } }
</style>
