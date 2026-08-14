<script lang="ts">
  type Props = {
    subject: "ink" | "image" | "Typst block";
    left: number;
    top: number;
    ready: boolean;
    canMoveBack: boolean;
    canMoveForward: boolean;
    onMove: (direction: -1 | 1) => void;
    onDelete?: () => void;
    grouped?: boolean;
    onGroup?: () => void;
    element?: HTMLElement;
  };

  let {
    subject,
    left,
    top,
    ready,
    canMoveBack,
    canMoveForward,
    onMove,
    onDelete,
    grouped = false,
    onGroup,
    element = $bindable(),
  }: Props = $props();
</script>

<div
  bind:this={element}
  class:ready
  class="selection-actions"
  style:left={`${left}px`}
  style:top={`${top}px`}
  aria-label={`Selected ${subject} actions`}
>
  <button
    type="button"
    title={`Move selected ${subject} back one layer`}
    aria-label={`Move selected ${subject} back one layer`}
    disabled={!canMoveBack}
    onclick={() => onMove(-1)}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="8" y="5" width="11" height="11" rx="1.5"></rect>
      <path d="M15 19H6.5A1.5 1.5 0 0 1 5 17.5V9"></path>
      <path d="m9 13-3 3-3-3M6 16v-5"></path>
    </svg>
  </button>
  <button
    type="button"
    title={`Move selected ${subject} forward one layer`}
    aria-label={`Move selected ${subject} forward one layer`}
    disabled={!canMoveForward}
    onclick={() => onMove(1)}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M9 19H6.5A1.5 1.5 0 0 1 5 17.5V9"></path>
      <rect x="8" y="5" width="11" height="11" rx="1.5"></rect>
      <path d="m15 11 3-3 3 3M18 8v5"></path>
    </svg>
  </button>
  {#if onGroup}
    <button
      type="button"
      title={grouped ? "Ungroup ink from Typst" : "Group ink with selected Typst block"}
      aria-label={grouped
        ? "Ungroup selected ink from its Typst block"
        : "Group selected ink with the selected Typst block"}
      onclick={onGroup}
    >
      {#if grouped}
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m8.5 15.5-2 2a3.2 3.2 0 0 1-4.5-4.5l3-3a3.2 3.2 0 0 1 4.5 0"></path>
          <path d="m15.5 8.5 2-2A3.2 3.2 0 0 1 22 11l-3 3a3.2 3.2 0 0 1-4.5 0"></path>
          <path d="m4 4 16 16"></path>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m9 15-2 2a3.2 3.2 0 0 1-4.5-4.5l3-3A3.2 3.2 0 0 1 10 14"></path>
          <path d="m15 9 2-2a3.2 3.2 0 0 1 4.5 4.5l-3 3A3.2 3.2 0 0 1 14 10"></path>
          <path d="m8 16 8-8"></path>
        </svg>
      {/if}
    </button>
  {/if}
  {#if onDelete}
    <button
      type="button"
      title={`Remove ${subject}`}
      aria-label={`Remove selected ${subject} from this page`}
      onclick={onDelete}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4.5 7h15M9 7V4.5h6V7m2.5 0-.7 13h-9.6L6.5 7M10 10.5v6M14 10.5v6"></path>
      </svg>
    </button>
  {/if}
</div>

<style>
  .selection-actions {
    position: absolute;
    z-index: 25;
    display: flex;
    visibility: hidden;
    align-items: center;
    gap: 3px;
    padding: 5px;
    pointer-events: auto;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }
  .selection-actions.ready { visibility: visible; }
  button {
    display: grid;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }
  button:hover:not(:disabled) { background: rgb(255 255 255 / 6%); }
  button:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  button:disabled { opacity: 0.45; cursor: default; }
  svg {
    width: 19px;
    height: 19px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
