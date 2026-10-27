<script lang="ts">
  import { t } from "../i18n";
  import ColorPanel from "../prototype/ColorPanel.svelte";
  import { dismissable } from "../workspace/dismiss";
  import { withRecentColor } from "../settings";
  import type { WritingCommand } from "./writingCommands";

  let { onCommand }: { onCommand: (command: WritingCommand) => void } = $props();
  let open = $state(false);
  let color = $state("#4c8df0");
  let recent = $state<string[]>([]);
  let use = $state<"text-color" | "color-value">("text-color");
  let button: HTMLButtonElement;

  function close(restore = true) {
    open = false;
    if (restore) button.focus();
  }

  function pick(next: string) {
    if (!open) return;
    color = next;
    recent = withRecentColor(recent, next);
    close(false);
    onCommand(`${use}:${next}`);
  }

  function place(node: HTMLElement) {
    const position = () => {
      const anchor = button.getBoundingClientRect();
      const box = node.getBoundingClientRect();
      node.style.left = `${Math.max(8, Math.min(anchor.left, window.innerWidth - box.width - 8))}px`;
      node.style.top = `${Math.max(8, Math.min(anchor.bottom + 6, window.innerHeight - box.height - 8))}px`;
    };
    position();
    const observer = new ResizeObserver(position);
    observer.observe(node);
    window.addEventListener("resize", position);
    return { destroy() { observer.disconnect(); window.removeEventListener("resize", position); } };
  }
</script>

<div class="color-control" use:dismissable={() => close(false)}>
  <button bind:this={button} type="button" class="color-button" class:active={open}
    aria-label={$t("editor.typstColour")} title={$t("editor.typstColour")} aria-haspopup="dialog" aria-expanded={open}
    onclick={() => { if (open) close(); else open = true; }}>
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="m6 16 6-12 6 12M8.5 11h7" />
      <path class="colour-line" d="M5 21h14" style:stroke={color} />
    </svg>
  </button>
  {#if open}
    <div class="color-popout" use:place role="dialog" tabindex="-1" aria-label={$t("editor.typstColour")}
      onkeydown={(event) => { if (event.key === "Escape") { event.stopPropagation(); close(); } }}>
      <div class="purpose">
        <label for="typst-colour-use">{$t("editor.insertAs")}</label>
        <select id="typst-colour-use" bind:value={use}>
          <option value="text-color">{$t("editor.textColour")}</option>
          <option value="color-value">{$t("editor.colourValueRgb")}</option>
        </select>
      </div>
      <p>{use === "text-color" ? $t("editor.colourSelectedTextOrMathOrAdd") : $t("editor.insertAnRgbValueForAFill")}</p>
      <ColorPanel embedded value={color} {recent} onPick={pick} onClose={() => close()} />
    </div>
  {/if}
</div>

<style>
  .color-control { display: inline-flex; flex: none; }
  .color-button { display: grid; place-items: center; width: var(--control-dense); height: var(--control-dense); padding: 0; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; }
  .color-button:hover, .color-button.active { background: var(--wash); color: var(--text); }
  .color-button:focus-visible, select:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; stroke-linejoin: round; }
  .colour-line { stroke-width: 3; }
  .color-popout { position: fixed; z-index: 60; width: 272px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow-y: auto; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--text); box-shadow: 0 18px 44px rgb(0 0 0 / 55%); }
  .purpose { display: flex; align-items: center; gap: 6px; padding: 12px 12px 0; font-size: var(--text-md); }
  select { flex: 1; min-width: 0; height: var(--control-dense); padding: 0 6px; border: 1px solid var(--edge); border-radius: var(--radius); background: var(--surround); color: var(--text); font: inherit; }
  p { margin: 6px 12px 0; color: var(--muted); font-size: var(--text-sm); line-height: 1.4; }
</style>
