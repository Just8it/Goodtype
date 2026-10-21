<script lang="ts">
  import { fromPosition, nudge, toPosition, TRACK_RESOLUTION } from "./stepSlider";

  /**
   * A setting slider a pen can set exactly: the track for reaching a value, − and + for landing
   * on it one step at a time (held, they repeat). A pen cannot type, so every number in the
   * palette menus is set through this one control. `log` spreads a wide range — stroke widths —
   * so each doubling gets the same length of track.
   */
  let {
    name,
    value,
    min,
    max,
    step,
    log = false,
    valueText,
    label = name,
    spokenValue = valueText,
    onInput,
  }: {
    name: string;
    value: number;
    min: number;
    max: number;
    step: number;
    log?: boolean;
    /** The value as read, with its unit: "0.56 mm", "60%". */
    valueText: string;
    /** What a screen reader calls the slider, when the visible name is too short on its own. */
    label?: string;
    /** The value read aloud: words where `valueText` uses symbols ("×", "mm"). */
    spokenValue?: string;
    onInput: (value: number) => void;
  } = $props();

  const scale = $derived({ min, max, step, log });
  const REPEAT_DELAY_MS = 380;
  const REPEAT_EVERY_MS = 70;
  let repeat: ReturnType<typeof setTimeout> | undefined;
  // The value a held button steps from; the parent's copy can lag a frame behind.
  let held = 0;

  function stepBy(direction: -1 | 1) {
    held = nudge(held, direction, scale);
    onInput(held);
  }

  function press(event: PointerEvent, direction: -1 | 1) {
    if (event.button > 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    held = value;
    stepBy(direction);
    stopRepeat();
    repeat = setTimeout(function again() {
      stepBy(direction);
      repeat = setTimeout(again, REPEAT_EVERY_MS);
    }, REPEAT_DELAY_MS);
  }

  function stopRepeat() {
    if (repeat) clearTimeout(repeat);
    repeat = undefined;
  }

  /** Keyboard and assistive activation, which arrive as a click with no pointer press. */
  function activate(event: MouseEvent, direction: -1 | 1) {
    if (event.detail !== 0) return;
    held = value;
    stepBy(direction);
  }

  /** Arrow keys move one real step, not one of the track's thousand positions. */
  function key(event: KeyboardEvent) {
    const steps: Record<string, number> = {
      ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1, PageDown: -10, PageUp: 10,
    };
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      onInput(event.key === "Home" ? min : max);
      return;
    }
    const count = steps[event.key];
    if (!count) return;
    event.preventDefault();
    held = value;
    for (let index = 0; index < Math.abs(count); index += 1) held = nudge(held, count > 0 ? 1 : -1, scale);
    onInput(held);
  }

  $effect(() => stopRepeat);
</script>

<div class="step-slider">
  <div class="label">
    <span class="name">{name}</span>
    <output class="value">{valueText}</output>
  </div>
  <div class="track">
    <button type="button" class="nudge" aria-label={`Decrease ${label.toLowerCase()}`} title="Hold to keep stepping"
      disabled={value <= min}
      onpointerdown={(event) => press(event, -1)} onpointerup={stopRepeat} onpointercancel={stopRepeat}
      onlostpointercapture={stopRepeat} onclick={(event) => activate(event, -1)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 8h8" /></svg>
    </button>
    <input type="range" min="0" max={TRACK_RESOLUTION} step="1" value={toPosition(value, scale)}
      aria-label={label} aria-valuetext={spokenValue}
      oninput={(event) => onInput(fromPosition(Number(event.currentTarget.value), scale))}
      onkeydown={key} />
    <button type="button" class="nudge" aria-label={`Increase ${label.toLowerCase()}`} title="Hold to keep stepping"
      disabled={value >= max}
      onpointerdown={(event) => press(event, 1)} onpointerup={stopRepeat} onpointercancel={stopRepeat}
      onlostpointercapture={stopRepeat} onclick={(event) => activate(event, 1)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 8h8M8 4v8" /></svg>
    </button>
  </div>
</div>

<style>
  .step-slider { display: flex; flex-direction: column; gap: 6px; }

  .label {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .name { color: var(--text); font-size: var(--text-md); }

  .value {
    color: var(--muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  .track { display: flex; align-items: center; gap: 8px; }
  .track input { min-width: 0; flex: 1; margin: 0; accent-color: var(--blueprint); }

  /* Sized for a pen tip, quiet until used: the same well the segmented controls sit in. */
  .nudge {
    display: grid;
    width: 28px;
    height: 28px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: rgb(0 0 0 / 22%);
    color: var(--muted);
    cursor: pointer;
    place-items: center;
    touch-action: none;
  }

  .nudge:hover:not(:disabled) { background: var(--wash); color: var(--text); }
  .nudge:active:not(:disabled) { background: rgb(76 141 240 / 22%); color: var(--text); }
  .nudge:disabled { opacity: 0.4; cursor: default; }
  .nudge svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-width: 1.8; }

  .nudge:focus-visible,
  .track input:focus-visible {
    outline: 2px solid var(--blueprint-light, var(--blueprint));
    outline-offset: 1px;
  }
</style>
