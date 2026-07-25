<script lang="ts">
  import { COLOR_PRESETS, colorName, normalizeColor } from "../settings";

  // In-app colour picker: presets, recents, and exact hex. It replaces the OS-native
  // `<input type="color">`, which opened a heavyweight system modal that looked nothing like
  // the rest of the chrome and offered no recents.
  let {
    value,
    recent = [],
    mode = "edit",
    canRemove = false,
    onPick,
    onRemove,
    onClose,
  }: {
    value: string;
    recent?: string[];
    /** `edit` retargets an existing swatch; `add` appends a new one. */
    mode?: "edit" | "add";
    canRemove?: boolean;
    onPick: (color: string) => void;
    onRemove?: () => void;
    onClose: () => void;
  } = $props();

  let hex = $state("");
  let panel = $state<HTMLElement>();

  // --- Visual picker -------------------------------------------------------------------
  // A saturation/value field plus a hue rail. Typing a hex is precise but hostile as the only
  // way in; this is the one you reach for when you just want "a bit warmer".
  let hue = $state(0);
  let saturation = $state(0);
  let brightness = $state(0);
  let field = $state<HTMLElement>();
  let dragging: "field" | "hue" | null = null;

  function hsvToHex(h: number, s: number, v: number): string {
    const f = (n: number) => {
      const k = (n + h / 60) % 6;
      const channel = v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
      return Math.round(channel * 255)
        .toString(16)
        .padStart(2, "0");
    };
    return `#${f(5)}${f(3)}${f(1)}`;
  }

  function hexToHsv(input: string): { h: number; s: number; v: number } | null {
    const normalized = normalizeColor(input);
    if (!normalized) return null;
    const r = parseInt(normalized.slice(1, 3), 16) / 255;
    const g = parseInt(normalized.slice(3, 5), 16) / 255;
    const b = parseInt(normalized.slice(5, 7), 16) / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const span = max - min;
    let h = 0;
    if (span !== 0) {
      if (max === r) h = 60 * (((g - b) / span) % 6);
      else if (max === g) h = 60 * ((b - r) / span + 2);
      else h = 60 * ((r - g) / span + 4);
    }
    return { h: (h + 360) % 360, s: max === 0 ? 0 : span / max, v: max };
  }

  /// Keeps the wheel in step with the swatch being edited, and with preset/recent picks.
  $effect(() => {
    const hsv = hexToHsv(value);
    if (!hsv) return;
    // Preserve the hue rail when the colour is grey, otherwise it snaps to red.
    if (hsv.s > 0.001) hue = hsv.h;
    saturation = hsv.s;
    brightness = hsv.v;
  });

  const wheelHex = $derived(hsvToHex(hue, saturation, brightness));

  function pointFromEvent(event: PointerEvent, element: HTMLElement) {
    const rect = element.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width)),
      y: Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height)),
    };
  }

  function fieldTo(event: PointerEvent) {
    if (!field) return;
    const point = pointFromEvent(event, field);
    saturation = point.x;
    brightness = 1 - point.y;
    hex = wheelHex;
  }

  function hueTo(event: PointerEvent) {
    const point = pointFromEvent(event, event.currentTarget as HTMLElement);
    hue = point.x * 360;
    hex = wheelHex;
  }

  // Follows the swatch being edited, including when the panel is retargeted while open.
  $effect(() => {
    hex = value;
  });

  // Focus moves into the panel so the whole thing is keyboard reachable and Escape lands here.
  $effect(() => {
    panel?.querySelector<HTMLElement>("button, input")?.focus();
  });

  /// Nudge the panel back inside the window if anchoring it to a chip near an edge would push
  /// it off. Measured rather than assumed: the panel's height changes with the recent row and
  /// the remove button, so any hardcoded reserve would be wrong half the time.
  let shift = $state({ x: 0, y: 0 });
  $effect(() => {
    // Re-measure whenever the content that changes the panel's size changes.
    void recent.length;
    void canRemove;
    if (!panel) return;
    const margin = 12;
    requestAnimationFrame(() => {
      if (!panel) return;
      const box = panel.getBoundingClientRect();
      const overflowY = Math.max(0, box.bottom - (window.innerHeight - margin));
      const underflowY = Math.max(0, margin - box.top + shift.y);
      const overflowX = Math.max(0, box.right - (window.innerWidth - margin));
      const underflowX = Math.max(0, margin - box.left + shift.x);
      const next = {
        x: shift.x - overflowX + underflowX,
        y: shift.y - overflowY + underflowY,
      };
      if (next.x !== shift.x || next.y !== shift.y) shift = next;
    });
  });

  function commitHex() {
    const normalized = normalizeColor(hex);
    if (normalized) onPick(normalized);
    else hex = value;
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
    }
  }
</script>

<div
  bind:this={panel}
  class="color-panel"
  style:transform={`translate(${shift.x}px, ${shift.y}px)`}
  role="dialog"
  tabindex="-1"
  aria-label={mode === "add" ? "Add a color" : "Edit color"}
  onkeydown={keydown}
>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={field}
    class="field"
    style:background={`linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(${hue} 100% 50%))`}
    role="application"
    aria-label="Saturation and brightness"
    onpointerdown={(event) => {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      dragging = "field";
      fieldTo(event);
    }}
    onpointermove={(event) => dragging === "field" && fieldTo(event)}
    onpointerup={() => (dragging = null)}
    onpointercancel={() => (dragging = null)}
  >
    <span
      class="thumb"
      style:left={`${saturation * 100}%`}
      style:top={`${(1 - brightness) * 100}%`}
      style:background={wheelHex}
    ></span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="hue"
    role="application"
    aria-label="Hue"
    onpointerdown={(event) => {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      dragging = "hue";
      hueTo(event);
    }}
    onpointermove={(event) => dragging === "hue" && hueTo(event)}
    onpointerup={() => (dragging = null)}
    onpointercancel={() => (dragging = null)}
  >
    <span
      class="thumb"
      style:left={`${(hue / 360) * 100}%`}
      style:top="50%"
      style:background={`hsl(${hue} 100% 50%)`}
    ></span>
  </div>

  <button type="button" class="apply wide" onclick={() => onPick(wheelHex)}>
    Use {wheelHex.toUpperCase()}
  </button>

  <div class="row-label">Presets</div>
  <div class="grid">
    {#each COLOR_PRESETS as preset (preset.hex)}
      <button
        type="button"
        class="chip"
        class:current={preset.hex.toLowerCase() === value.toLowerCase()}
        style:background={preset.hex}
        aria-label={preset.name}
        title={preset.name}
        onclick={() => onPick(preset.hex)}
      ></button>
    {/each}
  </div>

  {#if recent.length}
    <div class="row-label">Recent</div>
    <div class="grid">
      {#each recent as color (color)}
        <button
          type="button"
          class="chip"
          class:current={color.toLowerCase() === value.toLowerCase()}
          style:background={color}
          aria-label={colorName(color)}
          title={colorName(color)}
          onclick={() => onPick(color)}
        ></button>
      {/each}
    </div>
  {/if}

  <div class="row-label">Hex</div>
  <div class="hex-row">
    <span class="preview" style:background={normalizeColor(hex) ?? value}></span>
    <input
      bind:value={hex}
      spellcheck="false"
      autocomplete="off"
      aria-label="Hex color"
      maxlength="7"
      onchange={commitHex}
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          commitHex();
        }
      }}
    />
    <button type="button" class="apply" onclick={commitHex}>Apply</button>
  </div>

  {#if canRemove}
    <button type="button" class="remove" onclick={() => onRemove?.()}>
      Remove this color
    </button>
  {/if}
</div>

<style>
  .color-panel {
    display: flex;
    width: 216px;
    flex-direction: column;
    gap: 6px;
    padding: 11px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: #23272f;
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .field {
    position: relative;
    height: 104px;
    border-radius: 7px;
    cursor: crosshair;
    touch-action: none;
  }

  .hue {
    position: relative;
    height: 12px;
    border-radius: 6px;
    background: linear-gradient(
      to right,
      #f00 0%,
      #ff0 17%,
      #0f0 33%,
      #0ff 50%,
      #00f 67%,
      #f0f 83%,
      #f00 100%
    );
    cursor: ew-resize;
    touch-action: none;
  }

  .thumb {
    position: absolute;
    width: 12px;
    height: 12px;
    border: 2px solid #fff;
    border-radius: 50%;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 45%);
    pointer-events: none;
    transform: translate(-50%, -50%);
  }

  .apply.wide {
    width: 100%;
    font-family: "Cascadia Mono", Consolas, monospace;
  }

  .row-label {
    color: #6a727c;
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .grid {
    display: grid;
    gap: 5px;
    grid-template-columns: repeat(8, 1fr);
  }

  .chip {
    width: 100%;
    height: 20px;
    padding: 0;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 5px;
    cursor: pointer;
  }

  .chip.current {
    outline: 1.5px solid #4c8df0;
    outline-offset: 1.5px;
  }

  .chip:focus-visible,
  .apply:focus-visible,
  .remove:focus-visible,
  input:focus-visible {
    outline: 2px solid #7fb0f7;
    outline-offset: 1px;
  }

  .hex-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .preview {
    width: 22px;
    height: 22px;
    flex: none;
    border: 1px solid rgb(255 255 255 / 16%);
    border-radius: 5px;
  }

  input {
    width: 100%;
    min-width: 0;
    padding: 4px 6px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 5px;
    background: #16181d;
    color: #e9ebee;
    font: 11px/1.4 "Cascadia Mono", Consolas, monospace;
  }

  .apply,
  .remove {
    padding: 4px 8px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 5px;
    background: #1b1e24;
    color: #e9ebee;
    font-size: 11px;
    cursor: pointer;
  }

  .apply:hover,
  .remove:hover {
    background: rgb(255 255 255 / 8%);
  }

  .remove {
    margin-top: 2px;
    color: #e5645e;
  }
</style>
