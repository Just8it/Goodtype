<script lang="ts">
  let { highlightsOnly, returns, onHighlightsChange, onReturnsChange }: {
    highlightsOnly: boolean;
    returns: boolean;
    onHighlightsChange: (enabled: boolean) => void;
    onReturnsChange: (enabled: boolean) => void;
  } = $props();
  const id = $props.id();
</script>

<div class="eraser-settings">
  <label class="setting" title="Erase highlights while keeping handwriting and shapes. Images and text stay untouched.">
    <span>Highlights only</span>
    <input type="checkbox" role="switch" aria-label="Highlights only"
      aria-describedby={id + "-scope"}
      checked={highlightsOnly} onchange={(event) => onHighlightsChange(event.currentTarget.checked)} />
    <span class="toggle" aria-hidden="true"></span>
  </label>
  <label class="setting" title="After erasing, return to the tool you were using.">
    <span>Return to previous tool</span>
    <input type="checkbox" role="switch" aria-label="Return to previous tool"
      aria-describedby={id + "-return"}
      checked={returns} onchange={(event) => onReturnsChange(event.currentTarget.checked)} />
    <span class="toggle" aria-hidden="true"></span>
  </label>
  <span id={id + "-scope"} class="screen-reader-note">When off, erase ink and shapes. Images and text are always preserved.</span>
  <span id={id + "-return"} class="screen-reader-note">After each erase gesture, return to whichever tool was active before the eraser.</span>
</div>

<style>
  .eraser-settings { padding: 4px 12px 8px; }
  .screen-reader-note { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .setting { position: relative; display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 40px; font-size: var(--text-md); cursor: pointer; }
  input { position: absolute; right: 0; width: 40px; height: 40px; opacity: 0; margin: 0; cursor: pointer; }
  .toggle { flex: none; width: 32px; height: 18px; padding: 2px; border-radius: 10px; background: #505764; pointer-events: none; }
  .toggle::after { content: ""; display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--text); }
  input:checked + .toggle { background: var(--blueprint); }
  input:checked + .toggle::after { transform: translateX(14px); }
  input:focus-visible + .toggle { outline: 2px solid var(--blueprint); outline-offset: 3px; }
  @media (pointer: coarse) { .setting { min-height: 44px; } input { width: 44px; height: 44px; } }
</style>
