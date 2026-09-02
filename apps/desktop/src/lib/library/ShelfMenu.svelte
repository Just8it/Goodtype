<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { dismissable } from "../workspace/dismiss";

  /**
   * A small popover for the shelf: the New menu, the sort menu, a tile's own menu.
   *
   * Anchored by the caller through CSS rather than by measuring, because every one of these
   * hangs off a control it is already positioned beside. Closing is this component's job — see
   * `dismissable`, which is where both of the bugs every popout in this app has had are fixed.
   */
  export type ShelfMenuItem = {
    id: string;
    label: string;
    /** Turns a choice into a checked menu-radio item. */
    checked?: boolean;
    disabled?: boolean;
    /** Destructive entries read in oxide, so a delete never looks like a rename. */
    destructive?: boolean;
    onSelect: () => void;
  };

  let {
    label,
    title,
    items,
    align = "right",
    onClose,
  }: {
    label: string;
    /** Optional subject line, e.g. the name of the tile this menu acts on. */
    title?: string;
    items: ShelfMenuItem[];
    align?: "left" | "right";
    onClose: () => void;
  } = $props();

  let menuElement = $state<HTMLElement>();
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    queueMicrotask(() => enabledItems()[0]?.focus());
  });

  onDestroy(() => {
    if (opener?.isConnected && menuElement?.contains(document.activeElement)) opener.focus();
  });

  function enabledItems(): HTMLButtonElement[] {
    return menuElement
      ? [...menuElement.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
      : [];
  }

  function handleKeydown(event: KeyboardEvent) {
    const buttons = enabledItems();
    if (buttons.length === 0) return;
    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null = null;
    if (event.key === "ArrowDown") next = (current + 1 + buttons.length) % buttons.length;
    else if (event.key === "ArrowUp") next = (current - 1 + buttons.length) % buttons.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = buttons.length - 1;
    if (next === null) return;
    event.preventDefault();
    buttons[next]?.focus();
  }

  function handleFocusout() {
    queueMicrotask(() => {
      if (menuElement && !menuElement.contains(document.activeElement)) onClose();
    });
  }

  function run(item: ShelfMenuItem) {
    if (item.disabled) return;
    item.onSelect();
    onClose();
  }
</script>

<div
  bind:this={menuElement}
  use:dismissable={onClose}
  class="menu"
  class:left={align === "left"}
  role="menu"
  tabindex="-1"
  aria-label={label}
  onkeydown={handleKeydown}
  onfocusout={handleFocusout}
>
  {#if title}
    <div class="subject" title={title}>{title}</div>
  {/if}
  {#each items as item (item.id)}
    <button
      type="button"
      role={item.checked === undefined ? "menuitem" : "menuitemradio"}
      aria-checked={item.checked}
      class:destructive={item.destructive}
      disabled={item.disabled}
      onclick={() => run(item)}
    >
      <span class="grow">{item.label}</span>
      {#if item.checked}
        <svg class="marker" viewBox="0 0 24 24" aria-hidden="true">
          <path d="m5 12.5 4.2 4.2L19 7" />
        </svg>
      {/if}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: absolute;
    z-index: 50;
    top: calc(100% + 6px);
    right: 0;
    width: max-content;
    min-width: 190px;
    padding: 7px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius-lg, 10px);
    background: var(--panel, #23272f);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    transform-origin: top right;
    animation: menu-in 120ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .menu.left {
    right: auto;
    left: 0;
    transform-origin: top left;
  }

  .subject {
    overflow: hidden;
    padding: 8px 10px 10px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    margin-bottom: 5px;
    color: var(--text, #e9ebee);
    font-size: var(--text-md, 13px);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button {
    display: flex;
    gap: 12px;
    align-items: center;
    width: 100%;
    min-height: var(--control, 36px);
    padding: 7px 11px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: transparent;
    color: var(--text, #e9ebee);
    font: inherit;
    font-size: var(--text-md, 13px);
    text-align: left;
    touch-action: manipulation;
    cursor: pointer;
  }

  button:hover:enabled {
    background: rgb(255 255 255 / 6%);
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  button:disabled {
    color: var(--quiet, #6a727c);
    cursor: default;
  }

  .destructive {
    color: var(--oxide, #e5645e);
  }

  .grow {
    flex: 1;
  }

  .marker {
    width: var(--icon-dense, 16px);
    height: var(--icon-dense, 16px);
    color: var(--blueprint, #4c8df0);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense, 2);
  }

  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(-3px) scale(0.985);
    }
  }

  @media (pointer: coarse) {
    button {
      min-height: var(--control-touch, 44px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .menu {
      animation: none;
    }
  }
</style>
