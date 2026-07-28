<script lang="ts">
  import { ERASER_RADIUS_PT, type EraserSize } from "../settings";
  import { dismissable } from "../workspace/dismiss";

  /**
   * The eraser's settings, opened by pressing its tile again.
   *
   * It exists so the eraser obeys the rule the rest of the bar follows. Its three sizes used to
   * live inline, which made it the one tool whose settings were always on screen — so "press the
   * tile again for settings" had an exception with nothing marking it, and the bar grew a row
   * when you picked up the eraser and lost it again when you put it down.
   */
  let {
    size,
    onChange,
    onClose,
  }: {
    size: EraserSize;
    onChange: (size: EraserSize) => void;
    onClose: () => void;
  } = $props();

  const SIZES: { id: EraserSize; label: string; diameter: number }[] = [
    { id: "small", label: "Small", diameter: 12 },
    { id: "medium", label: "Medium", diameter: 18 },
    { id: "large", label: "Large", diameter: 26 },
  ];
</script>

<aside use:dismissable={onClose} class="eraser-panel" aria-label="Eraser settings">
  <div class="row-label">Hit area</div>
  <div class="sizes" role="radiogroup" aria-label="Eraser hit-area size">
    {#each SIZES as option (option.id)}
      <button
        type="button"
        class="size"
        class:current={size === option.id}
        role="radio"
        aria-checked={size === option.id}
        aria-label={option.label}
        onclick={() => onChange(option.id)}
      >
        <span
          class="ring"
          style:width={`${option.diameter}px`}
          style:height={`${option.diameter}px`}
        ></span>
        <span class="caption">{option.label}</span>
      </button>
    {/each}
  </div>
  <!-- The number the tool actually erases by, so the three words are not the only guide. -->
  <div class="note">Erases whole strokes within {ERASER_RADIUS_PT[size]} pt</div>
</aside>

<style>
  .eraser-panel {
    display: flex;
    width: 216px;
    flex-direction: column;
    gap: 8px;
    padding: 11px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .row-label {
    color: var(--quiet);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .sizes {
    display: grid;
    gap: 6px;
    grid-template-columns: repeat(3, 1fr);
  }

  .size {
    display: grid;
    gap: 6px;
    padding: 10px 0 8px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font: inherit;
    justify-items: center;
  }

  .size:hover { background: rgb(255 255 255 / 6%); }

  .size.current {
    outline: 1.5px solid var(--blueprint);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  .ring {
    box-sizing: border-box;
    border: 1.5px solid currentColor;
    border-radius: 50%;
  }

  .caption { font-size: 11px; }

  .note {
    color: var(--quiet);
    font-size: 11px;
  }

  .size:focus-visible {
    outline: 2px solid var(--blueprint);
    outline-offset: 2px;
  }
</style>
