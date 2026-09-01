<script lang="ts">
  import TypstEditor from "../editor/TypstEditor.svelte";
  import WritingBar from "../editor/WritingBar.svelte";
  import type { WritingCommand } from "../editor/writingCommands";
  import type { PresetSummary } from "../page/presets";

  // Full-height source view beside the canvas. It exists because the in-canvas editor is capped
  // at ten lines: short edits happen on the page, sustained writing happens here, and the canvas
  // stays live either way.
  //
  // A movable block stays pinned until the writer picks another block. Page text follows the
  // active page because each page owns its own fixed writing surface.
  let {
    mode = "edit",
    source = "",
    blockLabel = "",
    pageNumber = null,
    hasAnyBlock = false,
    root = null,
    dock = "left",
    width = 420,
    diagnostics = [],
    pageText = false,
    pageTextLineWrap = true,
    presets = [],
    presetBusy = false,
    onChange,
    onClose,
    onDockChange,
    onWidthChange,
    onGoToBlock,
    onCreatePageText,
    onCreateBlock,
    onPresetAction,
    onPageTextLineWrapChange,
  }: {
    /** `edit` — the target is on this page; `away` — it is on another page; `none` — no target. */
    mode?: "edit" | "style" | "away" | "none";
    source?: string;
    blockLabel?: string;
    /** The page the target lives on — shown in the strip, and named in the `away` notice. */
    pageNumber?: number | null;
    hasAnyBlock?: boolean;
    root?: string | null;
    dock?: "left" | "right";
    width?: number;
    diagnostics?: { severity: string; message: string }[];
    pageText?: boolean;
    pageTextLineWrap?: boolean;
    presets?: PresetSummary[];
    presetBusy?: boolean;
    onChange: (value: string) => void;
    onClose: () => void;
    onDockChange: (dock: "left" | "right") => void;
    onWidthChange: (width: number) => void;
    onGoToBlock?: () => void;
    onCreatePageText?: () => void;
    onCreateBlock?: () => void;
    onPresetAction?: (action: string) => void;
    onPageTextLineWrapChange?: (lineWrap: boolean) => void;
  } = $props();

  const MIN_WIDTH = 280;
  /// The panel never takes more than half the window; the page keeps the rest. The stored value
  /// is clamped here so it stays sensible, and `max-width: 50%` enforces it visually regardless.
  function clampWidth(next: number): number {
    const ceiling = Math.max(MIN_WIDTH, window.innerWidth * 0.5);
    return Math.round(Math.min(ceiling, Math.max(MIN_WIDTH, next)));
  }

  let editor = $state<{
    focus: () => void;
    showHelp: () => Promise<void>;
    formatDocument: () => Promise<void>;
    applyWritingCommand: (command: WritingCommand) => void;
  }>();
  /// Reported to assistive tech; the real ceiling is half the window (see `clampWidth`).
  let maxWidth = $state(MIN_WIDTH);
  $effect(() => {
    maxWidth = Math.max(MIN_WIDTH, Math.round(window.innerWidth * 0.5));
  });
  let drag: { pointerId: number; startX: number; startWidth: number } | null = null;

  /// Called after a pen stroke lands on the canvas: drawing must not cost the writer the caret.
  export function focus() {
    editor?.focus();
  }

  function beginResize(event: PointerEvent) {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag = { pointerId: event.pointerId, startX: event.clientX, startWidth: width };
  }

  function resize(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.pointerId) return;
    // Dragging the inner edge: rightwards widens a left-docked panel and narrows a right one.
    const delta = event.clientX - drag.startX;
    onWidthChange(clampWidth(drag.startWidth + (dock === "left" ? delta : -delta)));
  }

  function endResize(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.pointerId) return;
    drag = null;
  }

  function nudge(event: KeyboardEvent) {
    const step = event.shiftKey ? 40 : 8;
    const towards = dock === "left" ? 1 : -1;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      const direction = event.key === "ArrowRight" ? 1 : -1;
      onWidthChange(clampWidth(width + direction * towards * step));
    }
  }
</script>

<aside
  class="side-editor"
  class:dock-right={dock === "right"}
  style:width={`${width}px`}
  aria-label="Typst source"
>
  <!-- Identity strip: what this panel is holding, and how to move or close it. Everything you do
       *to* the text lives in the writing bar below, so this row never grows tool buttons. -->
  <header>
    <span class="identity">
      <span class="title">{mode === "style" ? "Notebook style" : pageText ? "Page text" : mode === "edit" || mode === "away" ? blockLabel : "Typst source"}</span>
      <!-- Only when the target is somewhere you cannot see. While you are editing the page in
           view, the status strip already answers "which page", and printing it twice made
           neither one look like the answer. -->
      {#if pageNumber !== null && mode === "away"}
        <span class="where">Page {pageNumber}</span>
      {/if}
    </span>
    {#if mode === "edit" || mode === "style"}
      <button
        type="button"
        class="icon"
        aria-label="Explain at caret"
        title="Explain at caret (F1)"
        onclick={() => void editor?.showHelp()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="8.5" />
          <path d="M12 16.5v-.6c0-1 .4-1.6 1.2-2.2.8-.6 1.3-1.2 1.3-2.2A2.5 2.5 0 0 0 12 9a2.4 2.4 0 0 0-2.4 2" />
        </svg>
      </button>
      <button
        type="button"
        class="icon"
        aria-label="Format document"
        title="Format document (Ctrl+Shift+F)"
        onclick={() => void editor?.formatDocument()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M4 7h16M4 12h10M4 17h13" />
        </svg>
      </button>
    {/if}
    <button
      type="button"
      class="icon"
      aria-label={dock === "left" ? "Move panel to the right" : "Move panel to the left"}
      title={dock === "left" ? "Move panel to the right" : "Move panel to the left"}
      onclick={() => onDockChange(dock === "left" ? "right" : "left")}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="3" y="4.5" width="18" height="15" rx="2" />
        <path d={dock === "left" ? "M10 4.5v15" : "M14 4.5v15"} />
      </svg>
    </button>
    <button
      type="button"
      class="icon"
      aria-label="Close source view"
      title="Close source view (Ctrl+Shift+E)"
      onclick={onClose}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5l14 14M19 5 5 19" /></svg>
    </button>
  </header>

  {#if mode === "edit" && pageText}
    <WritingBar
      {source}
      {presets}
      busy={presetBusy}
      onCommand={(command) => editor?.applyWritingCommand(command)}
      onPresetAction={(action) => onPresetAction?.(action)}
    />
  {/if}

  {#if mode === "edit" || mode === "style"}
    <div class="body">
      <TypstEditor
        bind:this={editor}
        value={source}
        {root}
        maxLines={null}
        lineWrap={pageText && pageTextLineWrap}
        ariaLabel={mode === "style" ? "Shared notebook Typst style" : `Source for ${blockLabel}`}
        onChange={(next) => onChange(next)}
        onExit={onClose}
        onLineWrapToggle={pageText && onPageTextLineWrapChange
          ? () => onPageTextLineWrapChange?.(!pageTextLineWrap)
          : undefined}
      />
    </div>
    {#if diagnostics.length}
      <ul class="diagnostics" aria-live="polite">
        {#each diagnostics as diagnostic}
          <li class:error={diagnostic.severity === "error"}>
            {diagnostic.severity}: {diagnostic.message}
          </li>
        {/each}
      </ul>
    {/if}
  {:else if mode === "away"}
    <!-- The target is still held, just not on the page in view. Editing stays parked rather than
         writing to a page the writer cannot see. -->
    <div class="notice">
      <p>{blockLabel} is on page {pageNumber}.</p>
      <button type="button" class="action" onclick={() => onGoToBlock?.()}>
        Go to page {pageNumber}
      </button>
      <p class="quiet">Or double-click a Typst block on this page to edit that one instead.</p>
    </div>
  {:else}
    <div class="notice">
      {#if pageText}
        <p>This page has no Page text yet.</p>
        <button type="button" class="action" onclick={() => onCreatePageText?.()}>
          Write Page text
        </button>
      {:else}
        <p>What would you like to write?</p>
        {#if hasAnyBlock}
          <button type="button" class="action" onclick={() => onGoToBlock?.()}>
            Go to the first block
          </button>
        {/if}
        <button type="button" class="action" onclick={() => onCreatePageText?.()}>
          Write Page text
        </button>
        <button type="button" class="action" onclick={() => onCreateBlock?.()}>
          Add a Typst block
        </button>
      {/if}
      <p class="quiet">Page text fills the writing area; Typst blocks stay movable.</p>
    </div>
  {/if}

  <!-- Drag the inner edge to resize; arrow keys nudge it for keyboard users. This is the ARIA
       window-splitter pattern: a `separator` with `aria-valuenow` is interactive precisely
       because it is focusable, which the a11y lint does not model. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="resizer"
    role="separator"
    aria-label="Resize source view"
    aria-orientation="vertical"
    aria-valuenow={width}
    aria-valuemin={MIN_WIDTH}
    aria-valuemax={maxWidth}
    tabindex="0"
    onpointerdown={beginResize}
    onpointermove={resize}
    onpointerup={endResize}
    onpointercancel={endResize}
    onkeydown={nudge}
  ></div>
</aside>

<style>
  .side-editor {
    position: relative;
    display: flex;
    flex: none;
    flex-direction: column;
    /* The page always keeps at least half the window. Enforced in CSS so it stays true when the
       window is resized, not just while dragging. */
    max-width: 50%;
    border-right: 1px solid var(--edge-soft);
    background: var(--surround, #1b1e24);
  }

  .side-editor.dock-right {
    border-right: 0;
    border-left: 1px solid var(--edge-soft);
  }

  header {
    display: flex;
    height: var(--control);
    align-items: center;
    gap: 2px;
    padding: 0 6px 0 12px;
    border-bottom: 1px solid var(--edge-soft);
    background: var(--panel, #23272f);
  }

  .identity {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: baseline;
    padding-right: 6px;
    gap: 9px;
  }

  .title {
    overflow: hidden;
    color: var(--muted, #aeb5be);
    font-size: var(--text-sm);
    letter-spacing: 0.09em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  /* Which page the target sits on. Quiet, because it is context rather than a control. */
  .where {
    flex: none;
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm);
  }

  .icon {
    display: grid;
    width: var(--control-dense);
    height: var(--control-dense);
    flex: none;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted, #aeb5be);
    cursor: pointer;
    place-items: center;
  }

  .icon:hover {
    background: var(--wash);
    color: var(--text, #e9ebee);
  }

  .icon:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  .icon svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .body {
    min-height: 0;
    flex: 1;
  }

  .notice {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    padding: 18px 16px;
    color: #e9ebee;
    font-size: var(--text-md);
  }

  .notice p {
    margin: 0;
  }

  .notice .quiet {
    color: #6a727c;
    font-size: var(--text-sm);
  }

  .action {
    padding: 7px 11px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: #23272f;
    color: #e9ebee;
    font-size: var(--text-md);
    cursor: pointer;
  }

  .action:hover {
    background: var(--wash);
  }

  .action:focus-visible {
    outline: 2px solid #7fb0f7;
    outline-offset: 1px;
  }

  .diagnostics {
    max-height: 9rem;
    margin: 0;
    padding: 8px 12px;
    border-top: 1px solid var(--edge-soft);
    color: #aeb5be;
    font: var(--text-sm)/1.5 "Cascadia Mono", Consolas, monospace;
    list-style: none;
    overflow-y: auto;
  }

  .diagnostics .error {
    color: #e5645e;
  }

  .resizer {
    position: absolute;
    top: 0;
    right: -3px;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
    touch-action: none;
  }

  .side-editor.dock-right .resizer {
    right: auto;
    left: -3px;
  }

  .resizer:hover,
  .resizer:focus-visible {
    background: rgb(76 141 240 / 45%);
    outline: none;
  }
</style>
