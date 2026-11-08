<script lang="ts">
  import { tick } from "svelte";
  import { subjectLabel } from "../i18n/labels";
  import { t } from "../i18n";
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import { populated, type MenuAction, type MenuSection } from "../workspace/menu";
  import { placeFloatingToolbar } from "../geometry/placement";

  /**
   * The bar beside a selection.
   *
   * Four actions stay in reach because they are what a selection is usually made for: ask the
   * assistant, copy, duplicate, remove. Paste joins them when the bar was opened by a long press
   * or right-click, because that press has already said where the paste goes. Everything else
   * sits behind one More button rather than widening the bar to twelve icons. A pen cannot
   * hover, so there is one level of menu and no flyouts.
   */
  type Props = {
    /** "page" is the context bar over empty canvas, where only Paste applies. */
    subject: "ink" | "image" | "shape" | "Typst block" | "items" | "page";
    left: number;
    top: number;
    ready: boolean;
    canMoveBack?: boolean;
    canMoveForward?: boolean;
    /** One layer at a time. Absent over empty canvas: there is nothing to move between layers. */
    onMove?: (direction: -1 | 1) => void;
    canArrangeBack?: boolean;
    canArrangeFront?: boolean;
    /** Straight to the back or the front. Objects only; ink moves a layer at a time. */
    onArrange?: (edge: "back" | "front") => void;
    canReadEarlier?: boolean;
    canReadLater?: boolean;
    /** Where screen readers announce the object. Objects only. */
    onReadingOrder?: (direction: -1 | 1) => void;
    onDelete?: () => void;
    grouped?: boolean;
    onGroup?: () => void;
    onConvert?: () => void;
    onShare?: () => void;
    onAI?: (anchor: { left: number; top: number }) => void;
    /** Ink only, and only where Windows offers a recognizer (ADR 0040). */
    onTranscribe?: () => void;
    onCut?: () => void;
    onCopy?: () => void;
    /** Shown greyed when nothing has been copied, so the bar keeps its shape. */
    onPaste?: () => void;
    pasteDisabled?: boolean;
    onDuplicate?: () => void;
    element?: HTMLElement;
  };

  let {
    subject,
    left,
    top,
    ready,
    canMoveBack = false,
    canMoveForward = false,
    onMove,
    canArrangeBack = false,
    canArrangeFront = false,
    onArrange,
    canReadEarlier = false,
    canReadLater = false,
    onReadingOrder,
    onDelete,
    grouped = false,
    onGroup,
    onConvert,
    onTranscribe,
    onShare,
    onAI,
    onCut,
    onCopy,
    onPaste,
    pasteDisabled = false,
    onDuplicate,
    element = $bindable(),
  }: Props = $props();

  const what = $derived(subjectLabel(subject, $t));

  /** An entry only when its action exists, so callers keep deciding what applies by passing it. */
  function entry(
    id: string,
    label: string,
    icon: string,
    onSelect: (() => void) | undefined,
    extra: Partial<MenuAction> = {},
  ): MenuAction[] {
    return onSelect ? [{ kind: "action", id, label, icon, onSelect, ...extra }] : [];
  }

  const more = $derived<MenuSection[]>(populated([
    {
      entries: entry("cut", $t("selectionMenu.cut"),
        "M8.6 8.4 19 16M8.6 15.6 19 8M6.5 4.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5M6.5 14.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5",
        onCut, { hint: "Ctrl X" }),
    },
    {
      entries: [
        ...entry("back", $t("selectionMenu.backOneLayer"), "M15 19H6.5A1.5 1.5 0 0 1 5 17.5V9M8 5h11v11H8zM9 13l-3 3-3-3M6 16v-5",
          onMove && (() => onMove(-1)), { disabled: !canMoveBack }),
        ...entry("forward", $t("selectionMenu.forwardOneLayer"), "M9 19H6.5A1.5 1.5 0 0 1 5 17.5V9M8 5h11v11H8zM15 11l3-3 3 3M18 8v5",
          onMove && (() => onMove(1)), { disabled: !canMoveForward }),
        ...entry("to-back", $t("selectionMenu.sendToBack"), "M4 12l8 4 8-4M4 8l8-4 8 4-8 4z",
          onArrange && (() => onArrange("back")), { disabled: !canArrangeBack }),
        ...entry("to-front", $t("selectionMenu.bringToFront"), "M4 12l8-4 8 4-8 4zM4 16l8 4 8-4",
          onArrange && (() => onArrange("front")), { disabled: !canArrangeFront }),
      ],
    },
    {
      entries: [
        ...entry("read-earlier", $t("workspace.readEarlierByScreenReaders"), "M12 19V5m0 0-5 5m5-5 5 5",
          onReadingOrder && (() => onReadingOrder(-1)), { disabled: !canReadEarlier }),
        ...entry("read-later", $t("workspace.readLaterByScreenReaders"), "M12 5v14m0 0 5-5m-5 5-5-5",
          onReadingOrder && (() => onReadingOrder(1)), { disabled: !canReadLater }),
      ],
    },
    {
      entries: [
        ...entry("group", grouped ? $t("selectionMenu.ungroup") : $t("selectionMenu.groupWithTypst"),
          grouped
            ? "m8.5 15.5-2 2a3.2 3.2 0 0 1-4.5-4.5l3-3a3.2 3.2 0 0 1 4.5 0m6-1.5 2-2A3.2 3.2 0 0 1 22 11l-3 3a3.2 3.2 0 0 1-4.5 0M4 4l16 16"
            : "m9 15-2 2a3.2 3.2 0 0 1-4.5-4.5l3-3A3.2 3.2 0 0 1 10 14m5-5 2-2a3.2 3.2 0 0 1 4.5 4.5l-3 3A3.2 3.2 0 0 1 14 10m-6 6 8-8",
          onGroup),
        ...entry("convert", $t("selectionMenu.convertToShape"), "M4 17.5 9.5 5 14 16l3-8 3 9.5m-3-3.5 3 3-3 3", onConvert),
        ...entry("transcribe", $t("selectionMenu.transcribe"), "M3 16c1.5-5 3-5 4 0s2.5 5 4 0M14 6.5h7M17.5 6.5V18", onTranscribe),
        ...entry("share", $t("helper.share"), "M4 4h16v12H9l-5 4z", onShare),
      ],
    },
  ]));

  let moreOpen = $state(false);
  let moreButton = $state<HTMLButtonElement>();
  let morePopover = $state<HTMLElement>();

  async function toggleMore() {
    if (moreOpen) { closeMore(); return; }
    moreOpen = true;
    await tick();
    const menu = morePopover?.querySelector<HTMLElement>(".overflow-menu");
    if (!morePopover || !menu || !moreButton) return;
    // The top layer, so a bar near the edge of the page cannot clip its own menu.
    morePopover.showPopover();
    const placed = placeFloatingToolbar(moreButton.getBoundingClientRect(), menu.getBoundingClientRect(),
      { left: 0, top: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight }, 6);
    morePopover.style.left = `${placed.left}px`;
    morePopover.style.top = `${placed.top}px`;
    menu.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
  }

  function closeMore() {
    const restore = element?.contains(document.activeElement);
    moreOpen = false;
    if (restore) moreButton?.focus();
  }
</script>

<div
  bind:this={element}
  class:ready
  class="selection-actions"
  role="toolbar"
  style:left={`${left}px`}
  style:top={`${top}px`}
  aria-label={subject === "page" ? $t("workspace.pageActions") : $t("workspace.selectedActions", { "subject": what })}
>
  {#if onAI}
    <button type="button" title={$t("aiEntry.shortcut")} aria-label={$t("aiEntry.ask")}
      onclick={event => { const rect = event.currentTarget.getBoundingClientRect(); onAI({ left: rect.left, top: rect.bottom }); }}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5h14v10H10l-5 4Z" /></svg>
    </button>
  {/if}
  {#if onCopy}
    <button type="button" title={$t("workspace.copyCtrlC")} aria-label={$t("workspace.copySelected", { "subject": what })} onclick={onCopy}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="8.5" y="8.5" width="11" height="11" rx="2"></rect>
        <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"></path>
      </svg>
    </button>
  {/if}
  {#if onPaste}
    <button
      type="button"
      title={pasteDisabled ? $t("workspace.nothingCopiedYet") : $t("workspace.pasteHereCtrlV")}
      aria-label={$t("workspace.pasteHere")}
      disabled={pasteDisabled}
      onclick={onPaste}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="8.5" y="3.5" width="7" height="3.5" rx="1"></rect>
        <path d="M8.5 5.25H6.5a1.5 1.5 0 0 0-1.5 1.5v12.5a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5V6.75a1.5 1.5 0 0 0-1.5-1.5h-2"></path>
      </svg>
    </button>
  {/if}
  {#if onDuplicate}
    <button type="button" title={$t("workspace.duplicateCtrlD")} aria-label={$t("workspace.duplicateSelected", { "subject": what })} onclick={onDuplicate}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="8.5" y="8.5" width="11" height="11" rx="2"></rect>
        <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"></path>
        <path d="M14 12v4M12 14h4"></path>
      </svg>
    </button>
  {/if}
  {#if onDelete}
    <button type="button" title={$t("workspace.remove2", { "subject": what })} aria-label={$t("workspace.removeSelectedFromPage", { subject: what })} onclick={onDelete}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4.5 7h15M9 7V4.5h6V7m2.5 0-.7 13h-9.6L6.5 7M10 10.5v6M14 10.5v6"></path>
      </svg>
    </button>
  {/if}
  {#if more.length}
    <button
      bind:this={moreButton}
      type="button"
      class:active={moreOpen}
      title={$t("selectionMenu.more")}
      aria-label={$t("selectionMenu.more")}
      aria-haspopup="menu"
      aria-expanded={moreOpen}
      onclick={() => void toggleMore()}
    >
      <svg class="solid" viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="19" cy="12" r="1.7"></circle></svg>
    </button>
    {#if moreOpen}
      <div class="more-popover" popover="manual" bind:this={morePopover}>
        <OverflowMenu compact title={$t("selectionMenu.more")} sections={more} onClose={closeMore} trigger={moreButton} />
      </div>
    {/if}
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
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow-menu);
  }
  .selection-actions.ready { visibility: visible; }
  button {
    display: grid;
    width: var(--control);
    height: var(--control);
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }
  button:hover:not(:disabled), button.active { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  button:disabled { opacity: var(--disabled); cursor: default; }
  svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  svg.solid { fill: currentColor; stroke: none; }
  .more-popover { position: fixed; inset: auto; margin: 0; padding: 0; border: 0; background: transparent; overflow: visible; }
  .more-popover :global(.overflow-menu) { position: relative; top: auto; right: auto; max-height: calc(100vh - 24px); overflow-y: auto; }
</style>
