<script lang="ts">
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
    /** Shown to the right, for a checkmark on the current choice. */
    marker?: string;
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

  function run(item: ShelfMenuItem) {
    if (item.disabled) return;
    item.onSelect();
    onClose();
  }
</script>

<aside use:dismissable={onClose} class="menu" class:left={align === "left"} aria-label={label}>
  {#if title}
    <div class="subject" title={title}>{title}</div>
  {/if}
  {#each items as item (item.id)}
    <button
      type="button"
      class:destructive={item.destructive}
      disabled={item.disabled}
      onclick={() => run(item)}
    >
      <span class="grow">{item.label}</span>
      {#if item.marker}<span class="marker">{item.marker}</span>{/if}
    </button>
  {/each}
</aside>

<style>
  .menu {
    position: absolute;
    z-index: 50;
    top: calc(100% + 6px);
    right: 0;
    width: max-content;
    min-width: 190px;
    padding: 7px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel, #23272f);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .menu.left {
    right: auto;
    left: 0;
  }

  .subject {
    overflow: hidden;
    padding: 8px 10px 10px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    margin-bottom: 5px;
    color: var(--text, #e9ebee);
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button {
    display: flex;
    gap: 12px;
    align-items: center;
    width: 100%;
    padding: 9px 11px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text, #e9ebee);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  button:hover:enabled {
    background: rgb(255 255 255 / 6%);
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: -2px;
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
    color: var(--blueprint, #4c8df0);
    font-size: 12px;
  }
</style>
