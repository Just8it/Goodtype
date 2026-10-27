<script lang="ts">
  import { subjectLabel } from "../i18n/labels";
  import { t } from "../i18n";
  type Props = {
    /** "page" is the context bar over empty canvas, where only Paste applies. */
    subject: "ink" | "image" | "shape" | "Typst block" | "items" | "page";
    left: number;
    top: number;
    ready: boolean;
    canMoveBack?: boolean;
    canMoveForward?: boolean;
    /** Absent over empty canvas: there is nothing to move between layers. */
    onMove?: (direction: -1 | 1) => void;
    onDelete?: () => void;
    grouped?: boolean;
    onGroup?: () => void;
    onConvert?: () => void;
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
    onDelete,
    grouped = false,
    onGroup,
    onConvert,
    onCut,
    onCopy,
    onPaste,
    pasteDisabled = false,
    onDuplicate,
    element = $bindable(),
  }: Props = $props();
</script>

<div
  bind:this={element}
  class:ready
  class="selection-actions"
  style:left={`${left}px`}
  style:top={`${top}px`}
  aria-label={subject === "page" ? $t("workspace.pageActions") : $t("workspace.selectedActions", { "subject": subjectLabel(subject, $t) })}
>
  {#if onMove}
  <button
    type="button"
    title={$t("workspace.moveSelectedBackOneLayer", { "subject": subjectLabel(subject, $t) })}
    aria-label={$t("workspace.moveSelectedBackOneLayer", { "subject": subjectLabel(subject, $t) })}
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
    title={$t("workspace.moveSelectedForwardOneLayer", { "subject": subjectLabel(subject, $t) })}
    aria-label={$t("workspace.moveSelectedForwardOneLayer", { "subject": subjectLabel(subject, $t) })}
    disabled={!canMoveForward}
    onclick={() => onMove(1)}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M9 19H6.5A1.5 1.5 0 0 1 5 17.5V9"></path>
      <rect x="8" y="5" width="11" height="11" rx="1.5"></rect>
      <path d="m15 11 3-3 3 3M18 8v5"></path>
    </svg>
  </button>
  {/if}
  {#if onGroup}
    <button
      type="button"
      title={grouped ? $t("workspace.ungroupInkFromTypst") : $t("workspace.groupSelectedInkWithTheSelectedTypst")}
      aria-label={grouped
        ? $t("workspace.ungroupInkFromTypst")
        : $t("workspace.groupSelectedInkWithTheSelectedTypst")}
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
  {#if onConvert}
    <button
      type="button"
      title={$t("workspace.convertSelectedInkToAnEditableShape")}
      aria-label={$t("workspace.convertSelectedInkToAnEditableShape")}
      onclick={onConvert}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4 17.5 9.5 5 14 16l3-8 3 9.5"></path>
        <path d="m17 14 3 3-3 3"></path>
      </svg>
    </button>
  {/if}
  {#if onCut}
    <button type="button" title={$t("workspace.cutCtrlX")} aria-label={$t("workspace.cutSelected", { "subject": subjectLabel(subject, $t) })} onclick={onCut}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle class="ring" cx="6.5" cy="7" r="2.5"></circle>
        <circle class="ring" cx="6.5" cy="17" r="2.5"></circle>
        <path d="M8.6 8.4 19 16M8.6 15.6 19 8"></path>
      </svg>
    </button>
  {/if}
  {#if onCopy}
    <button
      type="button"
      title={$t("workspace.copyCtrlC")}
      aria-label={$t("workspace.copySelected", { "subject": subjectLabel(subject, $t) })}
      onclick={onCopy}
    >
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
    <button
      type="button"
      title={$t("workspace.duplicateCtrlD")}
      aria-label={$t("workspace.duplicateSelected", { "subject": subjectLabel(subject, $t) })}
      onclick={onDuplicate}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="8.5" y="8.5" width="11" height="11" rx="2"></rect>
        <path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"></path>
        <path d="M14 12v4M12 14h4"></path>
      </svg>
    </button>
  {/if}
  {#if onDelete}
    <button
      type="button"
      title={$t("workspace.remove2", { "subject": subjectLabel(subject, $t) })}
      aria-label={$t("workspace.removeSelectedFromPage", { subject: subjectLabel(subject, $t) })}
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
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
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
  button:hover:not(:disabled) { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  button:disabled { opacity: 0.45; cursor: default; }
  svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  /* The scissors' finger loops are outlines, not the filled dots other icons use. */
  svg circle.ring { fill: none; stroke: currentColor; }
</style>
