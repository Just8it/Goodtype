<script lang="ts">
  import { t } from "../i18n";
  let { precise, highlightsOnly, returns, onPreciseChange, onHighlightsChange, onReturnsChange }: {
    precise: boolean;
    highlightsOnly: boolean;
    returns: boolean;
    onPreciseChange: (enabled: boolean) => void;
    onHighlightsChange: (enabled: boolean) => void;
    onReturnsChange: (enabled: boolean) => void;
  } = $props();
  const id = $props.id();
</script>

<div class="eraser-settings">
  <label class="setting" title={$t("workspace.eraseOnlyTheInkUnderTheEraser")}>
    <span>{$t("workspace.precise")}</span>
    <input type="checkbox" role="switch" aria-label={$t("workspace.precise")}
      aria-describedby={id + "-mode"}
      checked={precise} onchange={(event) => onPreciseChange(event.currentTarget.checked)} />
    <span class="toggle" aria-hidden="true"></span>
  </label>
  <label class="setting" title={$t("workspace.eraseHighlightsWhileKeepingHandwritingAndShapes")}>
    <span>{$t("workspace.highlightsOnly")}</span>
    <input type="checkbox" role="switch" aria-label={$t("workspace.highlightsOnly")}
      aria-describedby={id + "-scope"}
      checked={highlightsOnly} onchange={(event) => onHighlightsChange(event.currentTarget.checked)} />
    <span class="toggle" aria-hidden="true"></span>
  </label>
  <label class="setting" title={$t("workspace.afterErasingReturnToTheToolYou")}>
    <span>{$t("workspace.returnToPreviousTool")}</span>
    <input type="checkbox" role="switch" aria-label={$t("workspace.returnToPreviousTool")}
      aria-describedby={id + "-return"}
      checked={returns} onchange={(event) => onReturnsChange(event.currentTarget.checked)} />
    <span class="toggle" aria-hidden="true"></span>
  </label>
  <span id={id + "-mode"} class="screen-reader-note">{$t("workspace.whenOffEachTouchedStrokeIsRemovedWhole")}</span>
  <span id={id + "-scope"} class="screen-reader-note">{$t("workspace.whenOffEraseInkAndShapesImages")}</span>
  <span id={id + "-return"} class="screen-reader-note">{$t("workspace.afterEachEraseGestureReturnToWhichever")}</span>
</div>

<style>
  .eraser-settings { padding: 4px 12px 8px; }
  .screen-reader-note { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .setting { position: relative; display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 40px; font-size: var(--text-md); cursor: pointer; }
  input { position: absolute; right: 0; width: 40px; height: 40px; opacity: 0; margin: 0; cursor: pointer; }
  .toggle { flex: none; width: 34px; height: 20px; padding: 3px; border-radius: var(--radius-pill); background: var(--track); pointer-events: none; transition: background var(--fast) ease; }
  .toggle::after { content: ""; display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--muted); transition: transform var(--fast) ease, background var(--fast) ease; }
  input:checked + .toggle { background: var(--blueprint); }
  input:checked + .toggle::after { background: var(--text); transform: translateX(14px); }
  input:focus-visible + .toggle { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  @media (pointer: coarse) { .setting { min-height: 44px; } input { width: 44px; height: 44px; } }
  @media (prefers-reduced-motion: reduce) { .toggle, .toggle::after { transition: none; } }
</style>
