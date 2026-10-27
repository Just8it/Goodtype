<script lang="ts">
  import type { Snippet } from "svelte";
  import { t, notice, type UiText } from "../i18n";
  import { positionMessage } from "../i18n/labels";
  import type { AppSettings } from "../settings";
  import { nearestPaletteDock, paletteTransportPosition, type PaletteDock } from "./palette";
  import { chromeLayout, nearestHistoryCorner, type HistoryCorner } from "./chromeLayout";
  type PaletteDrag = { pointerId: number; size: number };
  let { workspace, settings, changeSettings, onStatus, closeToolCard, onUndo, onRedo,
    undoDisabled, redoDisabled, paletteDock = $bindable("bottom"), paletteElement = $bindable(),
    paletteDragging = $bindable(false), cards, tools, children }:
    { workspace: HTMLElement | undefined; settings: AppSettings; changeSettings: (next: AppSettings) => void;
      onStatus: (text: UiText) => void; closeToolCard: () => void; onUndo: () => void; onRedo: () => void;
      undoDisabled: boolean; redoDisabled: boolean; paletteDock?: PaletteDock; paletteElement?: HTMLElement;
      paletteDragging?: boolean; cards: Snippet; tools: Snippet; children: Snippet } = $props();
  let workspaceChrome = $state<HTMLElement>();
  let historyElement = $state<HTMLElement>();
  // The instrument bar snaps magnetically to the nearest workspace edge: horizontal along the
  // top/bottom, vertical along the left/right. Its inline sizes and colors follow that axis.
  let paletteX = $state(24);
  let paletteY = $state(96);
  let historyCorner = $state<HistoryCorner>("auto");
  $effect(() => { historyCorner = settings.historyCorner; });
  let historyDrag = $state<{ pointerId: number; size: number } | null>(null);
  let historyX = $state(0);
  let historyY = $state(0);
  /// Where each piece of canvas chrome sits. Derived from the dock in one place, so the palette
  /// and the controls around it cannot be given the same corner.
  let chromeCrowded = $state(false);
  const chrome = $derived(chromeLayout(paletteDock, historyCorner, chromeCrowded));
  let paletteDrag = $state<PaletteDrag | null>(null);

  /**
   * CSS grid line pair, e.g. `1 / 2`.
   *
   * Only applied while the element is parked. An absolutely positioned grid child that still names
   * a grid area is positioned *within that area* rather than within the grid, so a puck being
   * carried would measure its offsets from whichever cell it had been assigned — and the cell
   * moves as the drop target changes, which sends it off the canvas and under the clip.
   */
  function gridSpan(start: number, end: number): string {
    return `${start} / ${end}`;
  }

  function beginPaletteDrag(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary || !workspace) return;
    const target = event.currentTarget as HTMLElement;
    const workspaceBounds = workspace.getBoundingClientRect();
    const size = transportSize();
    const position = paletteTransportPosition(
      event.clientX - workspaceBounds.left,
      event.clientY - workspaceBounds.top,
      workspaceBounds.width,
      workspaceBounds.height,
      size,
    );
    const offset = transportOffset();
    paletteX = position.x - offset.x;
    paletteY = position.y - offset.y;
    closeToolCard();
    target.setPointerCapture(event.pointerId);
    paletteDrag = { pointerId: event.pointerId, size };
    paletteDragging = true;
    onStatus(notice("workspace.movingToolPaletteReleaseAtAnEdge"));
    event.preventDefault();
  }

  /**
   * Which edge the palette would take if the drag ended here.
   *
   * Applied while dragging, not only on release. The compact transport puck keeps one stable
   * geometry; its blue edge mark previews which full-palette orientation will return on release.
   */
  function dockUnderPointer(event: PointerEvent): PaletteDock | null {
    if (!workspace) return null;
    const bounds = workspace.getBoundingClientRect();
    return nearestPaletteDock(
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      bounds.width,
      bounds.height,
    );
  }

  function movePalette(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId || !workspace) return;
    const bounds = workspace.getBoundingClientRect();
    const position = paletteTransportPosition(
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      bounds.width,
      bounds.height,
      paletteDrag.size,
    );
    const offset = transportOffset();
    paletteX = position.x - offset.x;
    paletteY = position.y - offset.y;
    const next = dockUnderPointer(event);
    // Not written to settings yet: a drag that wanders across three edges should leave one
    // preference behind, on release, not three.
    if (next && next !== paletteDock) paletteDock = next;
  }

  function finishPaletteDrag(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId) return;
    const next = dockUnderPointer(event) ?? paletteDock;
    paletteDrag = null;
    paletteDragging = false;
    paletteDock = next;
    onStatus(notice("workspace.toolPaletteDockedToTheEdge", { "next": positionMessage(next) }));
    if (next !== settings.paletteDock) {
      changeSettings({ ...settings, paletteDock: next });
    }
  }

  /**
   * The pill is its own handle.
   *
   * There is nowhere to put a grip on a control this small without it becoming mostly grip, so the
   * strip between the two buttons is the grip — press anywhere on the pill that is not a button
   * and it moves. The buttons keep the press for themselves, so undo stays a single tap.
   */
  function beginHistoryDrag(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary || !workspace) return;
    if (event.target instanceof Element && event.target.closest("button")) return;
    const size = transportSize();
    carryHistoryTo(event, size);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    historyDrag = { pointerId: event.pointerId, size };
    onStatus(notice("workspace.movingTheUndoButtonsReleaseInA"));
    event.preventDefault();
  }

  function moveHistory(event: PointerEvent) {
    if (!historyDrag || event.pointerId !== historyDrag.pointerId) return;
    carryHistoryTo(event, historyDrag.size);
  }

  /// Carry the puck under the pointer and preview the corner it would land in. The corner is
  /// shown live but not written to settings until release, so one drag leaves one preference.
  function carryHistoryTo(event: PointerEvent, size: number) {
    if (!workspace) return;
    const bounds = workspace.getBoundingClientRect();
    const x = event.clientX - bounds.left;
    const y = event.clientY - bounds.top;
    const position = paletteTransportPosition(x, y, bounds.width, bounds.height, size);
    const offset = transportOffset();
    historyX = position.x - offset.x;
    historyY = position.y - offset.y;
    historyCorner = nearestHistoryCorner(x, y, bounds.width, bounds.height);
  }

  /**
   * Whether an end-docked rail and the history pill can share a row.
   *
   * Measured rather than guessed at, because it depends on how wide the side editor has been
   * pulled out and on how many tools the rail is carrying. `scrollWidth` is the rail's own width
   * even when the row has squeezed it, so this does not read back a width the pill caused — which
   * is what would make the pill move away, free up room, and be invited straight back.
   */
  $effect(() => {
    const chromeBox = workspaceChrome;
    const rail = paletteElement;
    const pill = historyElement;
    if (!chromeBox || !rail || !pill) return;
    const measure = () => {
      const style = getComputedStyle(chromeBox);
      const available =
        chromeBox.clientWidth -
        (Number.parseFloat(style.paddingLeft) || 0) -
        (Number.parseFloat(style.paddingRight) || 0);
      const gap = Number.parseFloat(style.columnGap) || 0;
      chromeCrowded = rail.scrollWidth + pill.scrollWidth + gap * 2 > available;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(chromeBox);
    observer.observe(rail);
    return () => observer.disconnect();
  });

  /**
   * From workspace coordinates to the ones a carried puck is actually positioned in.
   *
   * Drop targets are decided across the whole workspace, but the puck is an absolutely positioned
   * child of the chrome, so its offsets are measured from that element's padding box. Without the
   * conversion the puck sits a padding's width down and to the right of the pointer carrying it.
   */
  function transportOffset(): { x: number; y: number } {
    if (!workspaceChrome) return { x: 0, y: 0 };
    const style = getComputedStyle(workspaceChrome);
    return {
      x: Number.parseFloat(style.paddingLeft) || 0,
      y: Number.parseFloat(style.paddingTop) || 0,
    };
  }

  /// The touch-sized puck both transports shrink into, read from the same token they are built on.
  function transportSize(): number {
    if (!workspace) return 44;
    const token = Number.parseFloat(
      getComputedStyle(workspace).getPropertyValue("--control-touch"),
    );
    return Number.isFinite(token) && token > 0 ? token : 44;
  }

  function finishHistoryDrag(event: PointerEvent) {
    if (!historyDrag || event.pointerId !== historyDrag.pointerId) return;
    historyDrag = null;
    onStatus(notice("workspace.undoButtonsMovedToThe", { "value1": positionMessage(chrome.historyCorner) }));
    if (historyCorner !== settings.historyCorner) {
      changeSettings({ ...settings, historyCorner });
    }
  }

  /// Arrow keys park the pill in a corner, and Backspace hands the decision back to the layout.
  function moveHistoryWithKeyboard(event: KeyboardEvent) {
    const vertical = event.key === "ArrowUp" ? "top" : event.key === "ArrowDown" ? "bottom" : null;
    const horizontal =
      event.key === "ArrowLeft" ? "left" : event.key === "ArrowRight" ? "right" : null;
    const reset = event.key === "Backspace" || event.key === "Delete";
    if (!vertical && !horizontal && !reset) return;
    event.preventDefault();
    const current = historyCorner === "auto" ? "top-left" : historyCorner;
    const [row, column] = current.split("-") as ["top" | "bottom", "left" | "right"];
    const next: HistoryCorner = reset
      ? "auto"
      : (`${vertical ?? row}-${horizontal ?? column}` as HistoryCorner);
    historyCorner = next;
    onStatus(next === "auto"
        ? notice("workspace.undoButtonsFollowThePaletteAgain")
        : notice("workspace.undoButtonsMovedToThe", { "value1": positionMessage(next) }));
    if (next !== settings.historyCorner) changeSettings({ ...settings, historyCorner: next });
  }

  function dockPaletteWithKeyboard(event: KeyboardEvent) {
    const next: PaletteDock | null =
      event.key === "ArrowUp"
        ? "top"
        : event.key === "ArrowRight"
          ? "right"
          : event.key === "ArrowDown"
            ? "bottom"
            : event.key === "ArrowLeft"
              ? "left"
              : null;
    if (!next) return;
    event.preventDefault();
    paletteDock = next;
    onStatus(notice("workspace.toolPaletteDockedToTheEdge", { "next": positionMessage(next) }));
    if (next !== settings.paletteDock) changeSettings({ ...settings, paletteDock: next });
  }

</script>

<!-- One grid owns the canvas chrome and preserves the original sibling/drag ancestry. -->
<div bind:this={workspaceChrome} class="workspace-chrome">
      <div
        bind:this={historyElement}
        class:dragging={historyDrag !== null}
        class:reduced-motion={settings.reducedMotion}
        class="history-pill"
        data-corner={chrome.historyCorner}
        style:left={historyDrag ? `${historyX}px` : null}
        style:top={historyDrag ? `${historyY}px` : null}
        style:grid-column={historyDrag ? null : gridSpan(chrome.history.columnStart, chrome.history.columnEnd)}
        style:grid-row={historyDrag ? null : gridSpan(chrome.history.rowStart, chrome.history.rowEnd)}
        style:justify-self={historyDrag ? null : chrome.history.justify}
        style:align-self={historyDrag ? null : chrome.history.align}
        role="toolbar"
        tabindex="0"
        aria-label={$t("workspace.pageHistoryDragBetweenTheButtonsTo")}
        onpointerdown={beginHistoryDrag}
        onpointermove={moveHistory}
        onpointerup={finishHistoryDrag}
        onpointercancel={finishHistoryDrag}
        onkeydown={moveHistoryWithKeyboard}
      >
        <button type="button" aria-label={$t("workspace.undo")} title={$t("workspace.undoCtrlZ")} onclick={onUndo} disabled={undoDisabled}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 7-5 5 5 5M4 12h10a6 6 0 0 1 0 12" /></svg>
        </button>
        <span class="grip" aria-hidden="true"></span>
        <span class="transport" aria-hidden="true">
          <svg viewBox="0 0 24 24"><path d="m9 7-5 5 5 5M4 12h10a6 6 0 0 1 0 12" /></svg>
        </span>
        <button type="button" aria-label={$t("workspace.redo")} title={$t("workspace.redoCtrlY")} onclick={onRedo} disabled={redoDisabled}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 7 5 5-5 5M20 12H10a6 6 0 0 0 0 12" /></svg>
        </button>
      </div>


      <nav
        bind:this={paletteElement}
        class:dragging={paletteDrag !== null}
        class:reduced-motion={settings.reducedMotion}
        class:horizontal={paletteDrag === null && (paletteDock === "top" || paletteDock === "bottom")}
        class:dock-top={paletteDock === "top" && paletteDrag === null}
        class:dock-right={paletteDock === "right" && paletteDrag === null}
        class:dock-bottom={paletteDock === "bottom" && paletteDrag === null}
        class:dock-left={paletteDock === "left" && paletteDrag === null}
        class="instrument-palette"
        data-dock={paletteDock}
        style:grid-column={paletteDrag ? null : gridSpan(chrome.palette.columnStart, chrome.palette.columnEnd)}
        style:grid-row={paletteDrag ? null : gridSpan(chrome.palette.rowStart, chrome.palette.rowEnd)}
        style:justify-self={paletteDrag ? null : chrome.palette.justify}
        style:align-self={paletteDrag ? null : chrome.palette.align}
        style:left={paletteDrag ? `${paletteX}px` : null}
        style:top={paletteDrag ? `${paletteY}px` : null}
        aria-label={$t("workspace.canvasTools")}
      >

    {@render cards()}
        <button
          class="palette-grip"
          type="button"
          aria-label={$t("workspace.moveToolPalette")}
          aria-keyshortcuts="ArrowUp ArrowRight ArrowDown ArrowLeft"
          title={$t("workspace.dragToMoveArrowKeysDock")}
          onpointerdown={beginPaletteDrag}
          onpointermove={movePalette}
          onpointerup={finishPaletteDrag}
          onpointercancel={finishPaletteDrag}
          onlostpointercapture={finishPaletteDrag}
          onkeydown={dockPaletteWithKeyboard}
        >
          <span class="palette-grip-dots" aria-hidden="true">
            <i></i><i></i><i></i><i></i><i></i><i></i>
          </span>
          <svg class="palette-transport-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3.5a8.5 8.5 0 1 0 0 17h1.2a1.8 1.8 0 0 0 1.1-3.2 1.8 1.8 0 0 1 1.1-3.2h1.7A3.4 3.4 0 0 0 20.5 10 8.5 8.5 0 0 0 12 3.5Z" />
            <circle cx="7.8" cy="9" r="1" />
            <circle cx="11.4" cy="6.9" r="1" />
            <circle cx="15.4" cy="8.1" r="1" />
          </svg>
        </button>

    <div class="palette-primary">{@render tools()}</div>
  </nav>
  {@render children()}
</div>

<style>
  button { border: 0; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  button:disabled { cursor: not-allowed; opacity: .32; }
  .workspace-chrome {
    position: absolute;
    z-index: 15;
    inset: 0;
    display: grid;
    min-width: 0;
    min-height: 0;
    grid-template-columns: max-content minmax(0, 1fr) max-content;
    grid-template-rows: max-content minmax(0, 1fr) max-content;
    gap: 10px;
    padding: 16px 18px;
    pointer-events: none;
  }
  .workspace-chrome > :global(*) { pointer-events: auto; }

  .history-pill {
    position: relative;
    z-index: 15;
    display: flex;
    align-items: center;
    padding: 4px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 12px 30px rgb(0 0 0 / 45%);
    gap: 0;
    cursor: grab;
    touch-action: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .history-pill:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .history-pill.dragging { border-color: rgb(76 141 240 / 72%); cursor: grabbing; }
  /* The gap between the buttons is the handle, so it has to be wide enough to be one. The dots
     only appear on approach, so the control reads as two buttons until it needs to read as more. */
  /* The pill travels the way the rail does: it becomes one touch-sized puck under the pointer,
     carrying the icon of what is being moved, with a bracket marking the corner it will land in.
     The bracket follows the settled corner, not the one under the pointer, so a drag over a
     corner the palette owns shows the pill stepping aside instead of promising a place it
     cannot have. */
  .history-pill.dragging {
    position: absolute;
    z-index: 30;
    width: var(--control-touch);
    height: var(--control-touch);
    padding: 0;
    border-color: rgb(76 141 240 / 72%);
    box-shadow: 0 14px 34px rgb(0 0 0 / 62%);
    animation: palette-pickup 120ms cubic-bezier(0.16, 1, 0.3, 1);
    will-change: left, top;
  }
  .history-pill.dragging button,
  .history-pill.dragging .grip { display: none; }

  .history-pill .transport { display: none; }
  .history-pill.dragging .transport {
    display: grid;
    width: 100%;
    height: 100%;
    color: var(--text);
    place-items: center;
  }
  .history-pill .transport svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .history-pill.dragging::after {
    position: absolute;
    width: 9px;
    height: 9px;
    border: 2px solid var(--blueprint-light);
    content: "";
    pointer-events: none;
  }
  .history-pill.dragging[data-corner="top-left"]::after {
    top: -6px; left: -6px; border-right: 0; border-bottom: 0;
  }
  .history-pill.dragging[data-corner="top-right"]::after {
    top: -6px; right: -6px; border-bottom: 0; border-left: 0;
  }
  .history-pill.dragging[data-corner="bottom-left"]::after {
    bottom: -6px; left: -6px; border-top: 0; border-right: 0;
  }
  .history-pill.dragging[data-corner="bottom-right"]::after {
    bottom: -6px; right: -6px; border-top: 0; border-left: 0;
  }

  .history-pill.reduced-motion { transition: none; }
  .history-pill.reduced-motion.dragging { animation: none; }

  .history-pill .grip {
    width: 14px;
    height: var(--control);
    flex: none;
    background-image: radial-gradient(circle, var(--muted) 1.1px, transparent 1.2px);
    background-position: center;
    background-size: 4px 6px;
    background-repeat: repeat-y;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .history-pill:hover .grip,
  .history-pill:focus-visible .grip,
  .history-pill.dragging .grip { opacity: 0.8; }
  @media (prefers-reduced-motion: reduce) {
    .history-pill, .history-pill .grip { transition: none; }
    .history-pill.dragging { animation: none; }
  }
  .history-pill button {
    display: grid;
    width: var(--control);
    height: var(--control);
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }
  .history-pill button:hover:not(:disabled) { background: var(--wash); }
  .history-pill svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .instrument-palette {
    position: relative;
    z-index: 20;
    display: grid;
    /* One control wide plus its breathing room. Stated once and reused by the columns inside:
       when the grid track and the column boxes disagree, the tiles centre on the wider box and
       drift off the panel's own centre line. */
    --rail: calc(var(--control-touch) + 6px);
    grid-template-columns: var(--rail);
    grid-template-rows: auto 1fr;
    gap: 3px 0;
    padding: 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 20px 50px rgb(0 0 0 / 52%);
    touch-action: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }

  /* Placement comes from `chromeLayout`, which is the only thing that sees every piece of chrome
     at once and so the only thing able to keep them off each other. The dock classes that remain
     carry appearance: which way a settings card opens, and where its arrow points. */
  .instrument-palette.horizontal {
    grid-template-columns: auto 1fr;
    grid-template-rows: var(--rail);
  }

  /* The palette overlays the page. Keep its intrinsic size rather than squeezing its
     controls into a scroll container when the page or available space changes. */
  .instrument-palette:not(.horizontal):not(.dragging) { min-width: max-content; }

  /* One column, always. The quick controls sit at the foot of this same column rather than in a
     second one, which is what lets the rail keep a single fixed width in every state. */
  .palette-primary {
    display: flex;
    width: var(--rail);
    flex-direction: column;
    align-items: center;
    justify-content: center;
    grid-column: 1;
    grid-row: 2;
    gap: 3px;
    opacity: 1;
    transition: opacity 120ms ease;
  }

  .palette-grip {
    grid-column: 1 / -1;
    grid-row: 1;
    justify-self: center;
    touch-action: none;
  }

  /* Never narrower than the tiles it holds. With `min-width: 0` the row squeezed below its own
     contents and the tiles carried on past the panel, so the background stopped short of the last
     of them — the rail looked cut off at one end rather than full. */
  .horizontal .palette-primary {
    width: auto;
    min-width: max-content;
    flex-direction: row;
    grid-column: 2;
    grid-row: 1;
  }

  .horizontal .palette-grip {
    grid-column: 1;
    grid-row: 1 / -1;
    align-self: center;
  }

  /* The full rail is too large to move accurately near the opposite edge. During transport it
     becomes one stable touch-sized puck, centred under the pointer, then restores at the dock. */
  .instrument-palette.dragging {
    position: absolute;
    display: grid;
    width: var(--control-touch);
    height: var(--control-touch);
    padding: 0;
    border-color: rgb(76 141 240 / 72%);
    grid-template: 1fr / 1fr;
    gap: 0;
    box-shadow: 0 14px 34px rgb(0 0 0 / 62%);
    animation: palette-pickup 120ms cubic-bezier(0.16, 1, 0.3, 1);
    will-change: left, top;
  }

  .instrument-palette.dragging::after {
    position: absolute;
    border-radius: var(--radius-pill);
    background: var(--blueprint-light);
    content: "";
    pointer-events: none;
  }

  .instrument-palette.dragging[data-dock="top"]::after,
  .instrument-palette.dragging[data-dock="bottom"]::after {
    left: 12px;
    width: 20px;
    height: 2px;
  }

  .instrument-palette.dragging[data-dock="top"]::after { top: -5px; }
  .instrument-palette.dragging[data-dock="bottom"]::after { bottom: -5px; }

  .instrument-palette.dragging[data-dock="left"]::after,
  .instrument-palette.dragging[data-dock="right"]::after {
    top: 12px;
    width: 2px;
    height: 20px;
  }

  .instrument-palette.dragging[data-dock="left"]::after { left: -5px; }
  .instrument-palette.dragging[data-dock="right"]::after { right: -5px; }

  .instrument-palette.dragging .palette-primary {
    position: absolute;
    visibility: hidden;
    opacity: 0;
    pointer-events: none;
  }

  /* Grip: two columns of three dots, drag handle at the leading edge. */
  .palette-grip {
    display: grid;
    min-width: var(--control-touch);
    min-height: var(--control-touch);
    flex: none;
    place-content: center;
    padding: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: grab;
  }

  .palette-grip:hover { background: var(--wash); }
  .palette-grip:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }

  .palette-grip-dots {
    display: grid;
    grid-template-columns: repeat(2, 3.5px);
    grid-template-rows: repeat(3, 3.5px);
    gap: 3px;
  }

  .palette-grip i {
    width: 3.5px;
    height: 3.5px;
    border-radius: 50%;
    background: rgb(255 255 255 / 28%);
  }

  .palette-transport-icon {
    display: none;
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke);
  }

  .palette-transport-icon circle {
    fill: currentColor;
    stroke: none;
  }

  .dragging .palette-grip {
    width: 100%;
    height: 100%;
    color: var(--text);
    cursor: grabbing;
    grid-column: 1;
    grid-row: 1;
  }

  .dragging .palette-grip:hover { background: transparent; }
  .dragging .palette-grip-dots { display: none; }
  .dragging .palette-transport-icon { display: block; }

  @keyframes palette-pickup {
    from {
      opacity: 0.82;
      transform: scale(0.9);
    }
  }

  .instrument-palette.reduced-motion,
  .instrument-palette.reduced-motion .palette-primary {
    transition: none;
  }

  .instrument-palette.reduced-motion.dragging {
    animation: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .instrument-palette,
    .palette-primary {
      transition: none;
    }

    .instrument-palette.dragging {
      animation: none;
    }
  }


</style>
