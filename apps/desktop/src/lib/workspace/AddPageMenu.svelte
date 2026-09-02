<script lang="ts">
  import { dismissable } from "./dismiss";
  import { resolvePosition, type AddPageGroup, type AddPageSource, type AddPageWhere } from "./addPage";

  /**
   * The add-page panel: three columns instead of one long scroll.
   *
   * Settings on the left, the paper library in the middle, and a preview on the right showing the
   * page the button will actually make. The old menu stacked every decision in one 348px column,
   * so the templates sat four scrolls below the size they were drawn at and the writer never saw
   * the result until the page existed.
   *
   * The change that makes the rest possible: picking a paper no longer *makes* a page. A template
   * is a selection and the Add button is the commit. That is what lets a preview exist, what lets
   * arrow keys walk the library without creating six pages on the way past, and what lets a run
   * of pages be asked for once rather than by reopening this panel.
   *
   * Position and paper are remembered by the caller rather than reset each time it opens.
   */
  let {
    where,
    groups,
    tones,
    toneId,
    sizes,
    sizeId,
    orientation,
    previewAspect,
    geometry,
    currentPageId,
    pageNumber,
    pageCount,
    canPlaceRelative,
    onWhereChange,
    onToneChange,
    onSizeChange,
    onOrientationChange,
    onClose,
  }: {
    where: AddPageWhere;
    groups: AddPageGroup[];
    /** Paper colours. Every template below is shown in whichever one is selected. */
    tones: { id: string; name: string; backgroundColor: string }[];
    toneId: string;
    /** Page sizes, portrait. Landscape is the orientation toggle, not separate entries. */
    sizes: { id: string; name: string; detail: string }[];
    sizeId: string;
    orientation: "portrait" | "landscape";
    /** Width over height of the size being previewed, so a swatch is never a distorted page. */
    previewAspect: number;
    /** The chosen size after orientation, so the panel can read its own dimensions back. */
    geometry: { widthPt: number; heightPt: number };
    /** The page "before" and "after" are relative to. */
    currentPageId: string;
    /** One-based, as the writer sees it. */
    pageNumber: number;
    pageCount: number;
    /** False when no page is open, which leaves appending as the only meaning "add" can have. */
    canPlaceRelative: boolean;
    onWhereChange: (next: AddPageWhere) => void;
    onToneChange: (next: string) => void;
    onSizeChange: (next: string) => void;
    onOrientationChange: (next: "portrait" | "landscape") => void;
    onClose: () => void;
  } = $props();

  const CHOICES: { value: AddPageWhere; label: string }[] = [
    { value: "before", label: "Before" },
    { value: "after", label: "After" },
    { value: "last", label: "Last page" },
  ];

  const ORIENTATIONS = [
    { value: "portrait" as const, label: "Portrait" },
    { value: "landscape" as const, label: "Landscape" },
  ];

  const MM_PER_PT = 25.4 / 72;
  const MAX_RUN = 50;

  let category = $state("all");
  let selectedId = $state<string | null>(null);
  let count = $state(1);
  let added = $state(0);
  let panel = $state<HTMLElement>();

  /** Paper shelves. The import shelf is pinned above the library rather than scrolling with it. */
  const paperGroups = $derived(groups.filter((group) => (group.lane ?? "blank") === "blank"));
  const importSources = $derived(
    groups.filter((group) => group.lane === "import").flatMap((group) => group.sources),
  );
  /// The page you are on. A starting point rather than a kind of ruling, so it is pinned beside
  /// the import instead of standing among Plain / Lines / Squares / Dots as though it were one.
  const currentSources = $derived(
    groups.filter((group) => group.lane === "current").flatMap((group) => group.sources),
  );
  const pinned = $derived([...currentSources, ...importSources]);
  const everySource = $derived([...pinned, ...paperGroups.flatMap((group) => group.sources)]);

  /// Opens on the paper this page is already made of, which is the likeliest thing to want next.
  const selected = $derived<AddPageSource | undefined>(
    everySource.find((source) => source.id === selectedId) ??
      everySource.find((source) => source.id === "same") ??
      everySource[0],
  );
  const importing = $derived(Boolean(selected && importSources.some((source) => source.id === selected.id)));

  const shelves = $derived(
    category === "all" ? paperGroups : paperGroups.filter((group) => group.id === category),
  );

  const readout = $derived.by(() => {
    const inches = sizes.find((size) => size.id === sizeId)?.detail.includes("in");
    const width = geometry.widthPt * MM_PER_PT;
    const height = geometry.heightPt * MM_PER_PT;
    return inches
      ? `${(width / 25.4).toFixed(1)} × ${(height / 25.4).toFixed(1)} in`
      : `${Math.round(width)} × ${Math.round(height)} mm`;
  });

  // Reads back what the choice actually means for this notebook, so "Before" on page 1 says so
  // rather than leaving the writer to work out where the page will appear.
  const destination = $derived(
    !canPlaceRelative || where === "last"
      ? pageCount > 0
        ? `new page ${pageCount + 1}, at the end`
        : "the first page of the notebook"
      : where === "before"
        ? `new page ${pageNumber}, pushing page ${pageNumber} down`
        : `new page ${pageNumber + 1}, after the page you are on`,
  );

  const addLabel = $derived(
    importing ? "Import PDF" : count > 1 ? `Add ${count} pages` : "Add page",
  );

  const summary = $derived([
    {
      key: "size",
      value: `${sizes.find((size) => size.id === sizeId)?.name ?? ""} ${orientation} · ${readout}`,
    },
    { key: "paper", value: tones.find((tone) => tone.id === toneId)?.name ?? "" },
    { key: "ruling", value: importing ? "from the PDF" : (selected?.label ?? "") },
    { key: "where", value: canPlaceRelative ? where : "last" },
  ]);

  function commit(keepOpen: boolean) {
    if (!selected || selected.disabled) return;
    selected.onSelect(resolvePosition(canPlaceRelative ? where : "last", currentPageId), count);
    if (keepOpen) added += importing ? 1 : count;
    else onClose();
  }

  function step(by: number) {
    count = Math.max(1, Math.min(MAX_RUN, count + by));
  }

  /**
   * Arrow keys inside one group.
   *
   * Every group here selects rather than commits, so landing on something is safe: arrowing
   * across the library previews each paper in turn, which is the point of having a preview.
   */
  function moveWithin(event: KeyboardEvent, columns = 0) {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
    // Bound to the buttons rather than the group: a radiogroup that is itself focusable puts a
    // stop in the tab order that selects nothing.
    const group = (event.currentTarget as HTMLElement).closest<HTMLElement>(
      "[role='radiogroup'], [role='group']",
    );
    const items = group ? [...group.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")] : [];
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    if (current < 0) return;
    const stride = columns > 0 ? columns : 1;
    let next = current;
    if (event.key === "ArrowRight") next = current + 1;
    else if (event.key === "ArrowLeft") next = current - 1;
    else if (event.key === "ArrowDown") next = current + stride;
    else if (event.key === "ArrowUp") next = current - stride;
    else if (event.key === "Home") next = 0;
    else next = items.length - 1;
    next = Math.max(0, Math.min(items.length - 1, next));
    event.preventDefault();
    if (next === current) return;
    items[next].focus();
    items[next].click();
  }

  /// Enter commits from wherever the writer is, and Tab stays inside the panel so a keyboard
  /// writer cannot land behind it on the page it is covering.
  function keydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !(event.target as HTMLElement).closest(".add")) {
      event.preventDefault();
      commit(event.shiftKey);
      return;
    }
    if (event.key !== "Tab" || !panel) return;
    const stops = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled)")].filter(
      (stop) => stop.tabIndex >= 0,
    );
    if (stops.length < 2) return;
    const first = stops[0];
    const last = stops[stops.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  /// Opens on the commit: the panel already carries a sensible page, so Enter is a complete
  /// answer and everything else is an adjustment to it.
  $effect(() => {
    panel?.querySelector<HTMLElement>(".add")?.focus();
  });
</script>

<div
  bind:this={panel}
  use:dismissable={onClose}
  class="add-page-panel"
  role="dialog"
  tabindex="-1"
  aria-label="Add page"
  onkeydown={keydown}
>
  <header>
    <div class="subject">
      <strong>Add page</strong>
      <!-- Re-keyed so the line fades when it changes: it is the one thing here that answers
           "what will this do", and a silent swap is easy to miss. -->
      {#key destination}<span class="destination">{destination}</span>{/key}
    </div>

    <div class="run" role="group" aria-label="How many pages">
      <span class="run-label">pages</span>
      <button type="button" aria-label="One fewer page" disabled={count <= 1} onclick={() => step(-1)}>−</button>
      <output aria-live="polite">{count}</output>
      <button type="button" aria-label="One more page" disabled={count >= MAX_RUN} onclick={() => step(1)}>+</button>
    </div>

    <button type="button" class="close" aria-label="Close" onclick={onClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </header>

  <div class="body">
    <!-- Column A. Everything that decides the shape of the page, visible at once. -->
    <div class="settings">
      <section>
        <div class="eyebrow">Where</div>
        <div class="row" role="radiogroup" aria-label="Where the new page goes">
          {#each CHOICES as choice (choice.value)}
            {@const on = (canPlaceRelative ? where : "last") === choice.value}
            <button
              type="button"
              role="radio"
              class="chip"
              class:on
              disabled={!canPlaceRelative && choice.value !== "last"}
              aria-checked={on}
              tabindex={on ? 0 : -1}
              onkeydown={(event) => moveWithin(event)}
              onclick={() => onWhereChange(choice.value)}
            >{choice.label}</button>
          {/each}
        </div>
      </section>

      <section>
        <div class="eyebrow spread">
          <span>Size</span>
          <span class="readout">{readout}</span>
        </div>
        <div class="pairs" role="radiogroup" aria-label="Page size">
          {#each sizes as size (size.id)}
            <button
              type="button"
              role="radio"
              class="chip"
              class:on={size.id === sizeId}
              aria-checked={size.id === sizeId}
              tabindex={size.id === sizeId ? 0 : -1}
              title={size.detail}
              onkeydown={(event) => moveWithin(event, 2)}
              onclick={() => onSizeChange(size.id)}
            >{size.name}</button>
          {/each}
        </div>
      </section>

      <div class="row" role="radiogroup" aria-label="Orientation">
        {#each ORIENTATIONS as choice (choice.value)}
          <button
            type="button"
            role="radio"
            class="chip"
            class:on={orientation === choice.value}
            aria-checked={orientation === choice.value}
            tabindex={orientation === choice.value ? 0 : -1}
            onkeydown={(event) => moveWithin(event)}
            onclick={() => onOrientationChange(choice.value)}
          >
            <!-- The shape is the label: a page turned on its side says it faster than a word. -->
            <span class="page-mark" class:landscape={choice.value === "landscape"} aria-hidden="true"></span>
            {choice.label}
          </button>
        {/each}
      </div>

      <section>
        <div class="eyebrow">Paper</div>
        <div class="row" role="radiogroup" aria-label="Paper colour">
          {#each tones as tone (tone.id)}
            <button
              type="button"
              role="radio"
              class="chip tone"
              class:on={tone.id === toneId}
              aria-checked={tone.id === toneId}
              tabindex={tone.id === toneId ? 0 : -1}
              onkeydown={(event) => moveWithin(event)}
              onclick={() => onToneChange(tone.id)}
            >
              <span class="swatch" style:background={tone.backgroundColor} aria-hidden="true"></span>
              {tone.name}
            </button>
          {/each}
        </div>
      </section>
    </div>

    <!-- Column B. Filters and the import sit in a fixed band; only the papers scroll. -->
    <div class="library">
      <div class="band">
        <div class="filters" role="group" aria-label="Paper kind">
          <button
            type="button"
            class="pill"
            class:on={category === "all"}
            aria-pressed={category === "all"}
            onclick={() => (category = "all")}
          >All<span class="tally">{paperGroups.reduce((total, group) => total + group.sources.length, 0)}</span></button>
          {#each paperGroups as group (group.id)}
            <button
              type="button"
              class="pill"
              class:on={category === group.id}
              aria-pressed={category === group.id}
              onclick={() => (category = group.id)}
            >{group.title}<span class="tally">{group.sources.length}</span></button>
          {/each}
        </div>

        <!-- Pinned above the library rather than filed inside it. Neither the page you are on nor
             a document is a kind of ruling, and both used to sit among the paper shelves. -->
        <div class="pins" role="group" aria-label="Start from">
        {#each pinned as source (source.id)}
          <button
            type="button"
            class="import"
            class:on={selected?.id === source.id}
            disabled={source.disabled}
            aria-pressed={selected?.id === source.id}
            onkeydown={(event) => moveWithin(event)}
            onclick={() => (selectedId = source.id)}
          >
            <span class="sheet" aria-hidden="true">
              {#if importSources.some((entry) => entry.id === source.id)}
                <span class="badge">PDF</span>
              {:else if source.preview}
                {@html source.preview}
              {/if}
            </span>
            <span class="words">
              <span class="title">{source.label}</span>
              <span class="sub">{source.disabled ? "Needs the desktop app" : (source.detail ?? "")}</span>
            </span>
          </button>
        {/each}
        </div>
      </div>

      <!-- Dimmed while the PDF is selected: the file brings its own pages, so none of this
           applies to what the button is about to do. -->
      <div class="papers" class:muted={importing}>
        {#each shelves as group (group.id)}
          <div class="shelf">
            <div class="eyebrow">{group.title}</div>
            <div class="cards" role="group" aria-label={group.title}>
              {#each group.sources as source (source.id)}
                <button
                  type="button"
                  class="card"
                  class:on={selected?.id === source.id}
                  disabled={source.disabled}
                  aria-pressed={selected?.id === source.id}
                  onkeydown={(event) => moveWithin(event, 4)}
                  onclick={() => (selectedId = source.id)}
                >
                  <span class="thumb" style:aspect-ratio={previewAspect} aria-hidden="true">
                    {#if source.preview}
                      <!-- Built into this app, never read from an imported file. -->
                      {@html source.preview}
                    {/if}
                  </span>
                  <span class="name">{source.label}</span>
                </button>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- Column C. Not a swatch of the paper: the page, at the proportions it will have. -->
    <div class="preview">
      <div class="eyebrow">Preview</div>
      <div class="stage">
        <div class="sheet-preview" class:pdf={importing} style:aspect-ratio={previewAspect} style:--ratio={previewAspect}>
          {#if importing}
            <span class="badge big">PDF</span>
          {:else if selected?.preview}
            {@html selected.preview}
          {/if}
        </div>
      </div>

      <dl class="summary">
        {#each summary as row (row.key)}
          <div>
            <dt>{row.key}</dt>
            <dd>{row.value}</dd>
          </div>
        {/each}
      </dl>

      <div class="commit">
        <button
          type="button"
          class="add"
          disabled={!selected || selected.disabled}
          onclick={(event) => commit(event.shiftKey)}
        >{addLabel}</button>
        <span class="note">
          {added > 0 ? `${added} added — panel still open` : "Hold Shift to add and keep this open"}
        </span>
      </div>
    </div>
  </div>

  <footer>
    <span>&crarr; add · &#8679;&crarr; add and keep open · esc close</span>
    <span>{importing ? "PDF import · paper ignored" : `${selected?.label ?? ""} · ${sizes.find((size) => size.id === sizeId)?.name ?? ""}`}</span>
  </footer>
</div>

<style>
  /* Centred on the window, not hung off the button that opened it. `right: 0` against a 36px
     button sitting ~150px in from the window edge made `100vw` a lie about the space to its left,
     so the panel walked off-screen by that offset — invisible while maximised, obvious the moment
     it was not. A panel this size belongs to the window. */
  .add-page-panel {
    position: fixed;
    z-index: 50;
    top: 50%;
    left: 50%;
    display: flex;
    width: min(1040px, calc(100vw - 32px));
    flex-direction: column;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--surround);
    box-shadow: 0 30px 70px rgb(0 0 0 / 55%);
    overflow: hidden;
    /* The centring translate lives in the keyframes too, or the animation would drop it and the
       panel would fly in from the corner it is no longer anchored to. */
    transform: translate(-50%, -50%);
    animation: panel-in 150ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }

  @keyframes panel-in {
    from { opacity: 0; transform: translate(-50%, -50%) scale(0.985); }
    to { opacity: 1; transform: translate(-50%, -50%) scale(1); }
  }

  header {
    display: flex;
    flex: none;
    align-items: center;
    padding: 12px 12px 12px 20px;
    border-bottom: 1px solid var(--edge-soft);
    background: var(--panel);
    gap: 16px;
  }

  .subject { min-width: 0; flex: 1; }
  .subject strong { display: block; font-size: var(--text-lg); font-weight: 600; }

  .destination {
    display: block;
    margin-top: 3px;
    color: var(--muted);
    font-size: var(--text-sm);
    animation: said 180ms ease-out;
  }

  @keyframes said {
    from { opacity: 0; transform: translateY(-2px); }
  }

  .run {
    display: flex;
    flex: none;
    align-items: center;
    padding: 4px 5px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: var(--surround);
    gap: 4px;
  }

  .run-label {
    padding: 0 4px;
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .run button {
    width: 26px;
    height: 24px;
    border: 0;
    border-radius: var(--radius);
    background: rgb(255 255 255 / 7%);
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    cursor: pointer;
    transition: background 120ms ease;
  }

  .run button:hover:enabled { background: var(--wash); }
  .run button:disabled { opacity: 0.35; cursor: default; }

  .run output {
    min-width: 20px;
    color: var(--text);
    font-size: var(--text-md);
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    text-align: center;
  }

  .close {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }

  .close:hover { background: var(--wash); color: var(--text); }

  .close svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .body {
    display: flex;
    height: min(520px, calc(100vh - 240px));
    align-items: stretch;
  }

  /* Column A. Fixed width and no scroll of its own: the shape of a page is four small decisions,
     and making them scroll is what pushed the library out of sight in the first place. */
  .settings {
    display: flex;
    width: 250px;
    flex: none;
    flex-direction: column;
    padding: 16px;
    border-right: 1px solid var(--edge-soft);
    gap: 16px;
    overflow-y: auto;
  }

  section { display: flex; flex-direction: column; gap: 8px; }

  .eyebrow {
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .eyebrow.spread { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }

  .readout {
    font-variant-numeric: tabular-nums;
    letter-spacing: 0;
    text-transform: none;
  }

  .row { display: flex; gap: 6px; }
  .pairs { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }

  .chip {
    display: flex;
    height: var(--control);
    flex: 1;
    align-items: center;
    justify-content: center;
    padding: 0 8px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    gap: 7px;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }

  .chip:hover:enabled { background: var(--wash); color: var(--text); }
  .chip:disabled { opacity: 0.4; cursor: default; }

  .chip.on {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  .page-mark {
    width: 9px;
    height: 12px;
    flex: none;
    border: 1px solid currentColor;
    border-radius: 1px;
    opacity: 0.8;
    transition: width 140ms ease, height 140ms ease;
  }

  .page-mark.landscape { width: 12px; height: 9px; }

  .swatch {
    width: 16px;
    height: 16px;
    flex: none;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 22%);
  }

  /* Column B. */
  .library {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    border-right: 1px solid var(--edge-soft);
  }

  .band {
    display: flex;
    flex: none;
    flex-direction: column;
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--edge-soft);
    gap: 11px;
  }

  .filters { display: flex; flex-wrap: wrap; gap: 7px; }

  .pill {
    display: flex;
    height: var(--control-dense);
    align-items: center;
    padding: 0 11px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    gap: 6px;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }

  .pill:hover { background: var(--wash); color: var(--text); }

  .pill.on {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  .tally { color: var(--quiet); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }

  .pins { display: flex; flex-direction: column; gap: 6px; }

  .import {
    display: flex;
    align-items: center;
    padding: 10px 12px;
    border: 1px dashed rgb(255 255 255 / 18%);
    border-radius: var(--radius);
    background: rgb(255 255 255 / 2%);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    gap: 12px;
    transition: background 120ms ease, border-color 120ms ease;
  }

  .import:hover:enabled { border-color: rgb(255 255 255 / 32%); }
  .import:disabled { opacity: 0.45; cursor: default; }

  .import.on {
    border-color: var(--blueprint);
    border-style: solid;
    background: rgb(76 141 240 / 9%);
  }

  .sheet {
    position: relative;
    display: flex;
    width: 34px;
    height: 42px;
    flex: none;
    align-items: flex-end;
    justify-content: center;
    padding-bottom: 6px;
    border-radius: 3px;
    background: var(--paper);
    box-shadow: 0 1px 3px rgb(0 0 0 / 40%);
  }

  .sheet :global(svg) { display: block; width: 100%; height: 100%; }

  .badge {
    padding: 2px 4px;
    border-radius: 2px;
    background: var(--oxide);
    color: #fff;
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }

  .badge.big { padding: 3px 7px; font-size: var(--text-sm); }

  .words { display: flex; min-width: 0; flex-direction: column; gap: 2px; }
  .words .title { font-size: var(--text-md); font-weight: 600; }
  .words .sub { color: var(--muted); font-size: var(--text-sm); }

  .papers {
    min-width: 0;
    flex: 1;
    padding: 14px 16px 18px;
    overflow-y: auto;
    transition: opacity 160ms ease;
  }

  .papers.muted { opacity: 0.4; }
  .shelf + .shelf { margin-top: 18px; }
  .shelf .eyebrow { margin-bottom: 9px; }

  /* `minmax(0, …)` rather than `1fr`: a track's automatic minimum is its content's min-content
     width, and a preview is an SVG that reports an intrinsic 300px. One bare `1fr` would let a
     thumbnail push the whole panel wider than it is allowed to be. */
  .cards {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 11px;
  }

  .card {
    display: flex;
    flex-direction: column;
    padding: 7px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    gap: 7px;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }

  .card:hover:enabled { border-color: rgb(255 255 255 / 30%); color: var(--text); }
  .card:disabled { opacity: 0.45; cursor: default; }

  .card.on {
    border-color: var(--blueprint);
    background: rgb(76 141 240 / 14%);
    color: var(--text);
  }

  .thumb {
    display: block;
    width: 100%;
    border-radius: 3px;
    background: var(--paper);
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 30%);
    overflow: hidden;
    transition: transform 120ms ease;
  }

  .card:hover:enabled .thumb { transform: scale(1.03); }
  .thumb :global(svg) { display: block; width: 100%; height: 100%; }

  .name {
    overflow: hidden;
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Column C. */
  .preview {
    --stage-w: 230px;
    --stage-h: 300px;
    display: flex;
    width: 262px;
    flex: none;
    flex-direction: column;
    padding: 14px 16px 16px;
    background: var(--charcoal);
    gap: 12px;
  }

  .stage {
    display: flex;
    min-height: var(--stage-h);
    flex: 1;
    align-items: center;
    justify-content: center;
  }

  /* Sized from the stage rather than from its contents. `aspect-ratio` on a box with no children
     and no definite side resolves to zero, which is why a plain page previewed as nothing at all.
     Driving the height off whichever limit binds first keeps every page shape inside the column
     whether it is portrait or landscape. */
  .sheet-preview {
    width: auto;
    height: min(var(--stage-h), calc(var(--stage-w) / var(--ratio)));
    background: var(--paper);
    box-shadow: 0 2px 6px rgb(0 0 0 / 35%), 0 22px 46px rgb(0 0 0 / 45%);
  }

  .sheet-preview :global(svg) { display: block; width: 100%; height: 100%; }
  .sheet-preview.pdf { display: grid; place-items: center; }

  .summary {
    display: flex;
    flex: none;
    flex-direction: column;
    margin: 0;
    gap: 7px;
  }

  .summary div {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }

  .summary dt {
    flex: none;
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .summary dd {
    overflow: hidden;
    margin: 0;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .commit { display: flex; flex: none; flex-direction: column; gap: 8px; }

  .add {
    height: 42px;
    border: 0;
    border-radius: var(--radius);
    background: var(--blueprint);
    color: #12151a;
    font: inherit;
    font-size: var(--text-md);
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 6px 18px rgb(0 0 0 / 40%);
    transition: filter 120ms ease;
  }

  .add:hover:enabled { filter: brightness(1.08); }
  .add:disabled { opacity: 0.45; cursor: default; box-shadow: none; }

  .note { color: var(--quiet); font-size: var(--text-xs); text-align: center; }

  footer {
    display: flex;
    height: 34px;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 0 18px;
    border-top: 1px solid var(--edge-soft);
    background: var(--charcoal);
    color: var(--quiet);
    font-size: var(--text-sm);
    gap: 16px;
  }

  footer span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: 1px;
  }

  /* A narrower window tightens the two fixed columns and drops the library to three across
     before it gives up the preview — the preview is the point of the panel, so it is the last
     thing to go, not the first. */
  @media (max-width: 1120px) {
    .settings { width: 214px; padding: 14px; }
    .preview { --stage-w: 192px; --stage-h: 250px; width: 218px; padding: 12px 13px 14px; }
    .cards { grid-template-columns: repeat(3, minmax(0, 1fr)); }
    .body { height: min(480px, calc(100vh - 200px)); }
  }

  /* Below this there is no honest way to show a page and a library side by side. The summary in
     the footer still says what the button will make. */
  @media (max-width: 880px) {
    .preview { display: none; }
    .cards { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }

  @media (prefers-reduced-motion: reduce) {
    .add-page-panel,
    .destination { animation: none; }

    .chip,
    .pill,
    .card,
    .thumb,
    .page-mark,
    .papers,
    .add,
    .run button { transition: none; }
  }
</style>
