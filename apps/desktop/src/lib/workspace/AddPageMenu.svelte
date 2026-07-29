<script lang="ts">
  import { dismissable } from "./dismiss";
  import { resolvePosition, type AddPageGroup, type AddPageSource, type AddPageWhere } from "./addPage";

  /**
   * The add-page popout: where the page goes, then what it is made of.
   *
   * Position is remembered by the caller rather than reset each time the menu opens. Somebody
   * inserting a run of pages before the current one should not have to re-pick "Before" on every
   * single one.
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

  // Reads back what the choice actually means for this notebook, so "Before" on page 1 says so
  // rather than leaving the writer to work out where the page will appear.
  const destination = $derived(
    !canPlaceRelative || where === "last"
      ? pageCount > 0
        ? `New page ${pageCount + 1} at the end`
        : "First page of the notebook"
      : where === "before"
        ? `New page ${pageNumber}, pushing page ${pageNumber} down`
        : `New page ${pageNumber + 1}, after the page you are on`,
  );

  function choose(source: AddPageSource) {
    if (source.disabled) return;
    source.onSelect(resolvePosition(canPlaceRelative ? where : "last", currentPageId));
    onClose();
  }
</script>

<aside use:dismissable={onClose} class="add-page-menu" aria-label="Add page">
  <div class="menu-subject">
    <strong>Add page</strong>
    <span>{destination}</span>
  </div>

  <div class="where" role="group" aria-label="Where the new page goes">
    {#each CHOICES as choice (choice.value)}
      <button
        type="button"
        class:selected={(canPlaceRelative ? where : "last") === choice.value}
        disabled={!canPlaceRelative && choice.value !== "last"}
        aria-pressed={(canPlaceRelative ? where : "last") === choice.value}
        onclick={() => onWhereChange(choice.value)}
      >
        {choice.label}
      </button>
    {/each}
  </div>

  <div class="menu-heading">Size</div>
  <div class="sizes" role="group" aria-label="Page size">
    {#each sizes as size (size.id)}
      <button
        type="button"
        class="size"
        class:selected={size.id === sizeId}
        aria-pressed={size.id === sizeId}
        title={size.detail}
        onclick={() => onSizeChange(size.id)}
      >
        {size.name}
      </button>
    {/each}
  </div>
  <div class="orientation" role="group" aria-label="Orientation">
    {#each [{ value: "portrait", label: "Portrait" }, { value: "landscape", label: "Landscape" }] as choice (choice.value)}
      <button
        type="button"
        class:selected={orientation === choice.value}
        aria-pressed={orientation === choice.value}
        onclick={() => onOrientationChange(choice.value as "portrait" | "landscape")}
      >
        {choice.label}
      </button>
    {/each}
  </div>

  <div class="menu-heading">Paper</div>
  <div class="tones" role="group" aria-label="Paper colour">
    {#each tones as paper (paper.id)}
      <button
        type="button"
        class="tone"
        class:selected={paper.id === toneId}
        aria-pressed={paper.id === toneId}
        onclick={() => onToneChange(paper.id)}
      >
        <span class="chip" style:background={paper.backgroundColor} aria-hidden="true"></span>
        {paper.name}
      </button>
    {/each}
  </div>

  <div class="shelves">
    {#each groups as group (group.id)}
      <div class="menu-heading">{group.title}</div>
      <div class="sources">
        {#each group.sources as source (source.id)}
          <button
            type="button"
            class="source"
            class:compact={source.compact}
            disabled={source.disabled}
            onclick={() => choose(source)}
          >
            <span class="preview" style:aspect-ratio={previewAspect} aria-hidden="true">
              {#if source.preview}
                <!-- Built into this app, never read from an imported file. -->
                {@html source.preview}
              {/if}
            </span>
            <span class="label">{source.label}</span>
            <!-- Always present, even when empty, so tiles in a row line up whether or not they
                 have something to say for themselves. -->
            <span class="detail">{source.detail ?? ""}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>
</aside>

<style>
  .add-page-menu {
    position: absolute;
    z-index: 50;
    top: calc(100% + 8px);
    right: 0;
    width: 348px;
    padding: 8px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .menu-subject {
    padding: 8px 10px 10px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    margin-bottom: 8px;
  }

  .menu-subject strong {
    display: block;
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
  }

  .menu-subject span {
    display: block;
    margin-top: 4px;
    color: var(--muted);
    font-size: 10.5px;
  }

  .menu-heading {
    padding: 10px 3px 6px;
    color: var(--quiet);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .where {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 3px;
    padding: 3px;
    border-radius: 8px;
    background: rgb(0 0 0 / 25%);
  }

  .where button {
    padding: 7px 4px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }

  .where button:hover:enabled {
    color: var(--text);
  }

  .where button.selected {
    background: rgb(76 141 240 / 16%);
    color: var(--blueprint);
  }

  .where button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .sizes {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 5px;
  }

  .sizes button {
    padding: 6px 2px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .sizes button:hover {
    color: var(--text);
  }

  .sizes button.selected,
  .orientation button.selected {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  .orientation {
    display: grid;
    margin-top: 5px;
    grid-template-columns: repeat(2, 1fr);
    gap: 5px;
  }

  .orientation button {
    padding: 6px 2px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .orientation button:hover {
    color: var(--text);
  }

  .tones {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 5px;
  }

  .tone {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 8px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .tone:hover {
    color: var(--text);
  }

  .tone.selected {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  /* Outlined, so a near-white chip and a near-black one both read against the panel. */
  .chip {
    width: 13px;
    height: 13px;
    border: 1px solid rgb(255 255 255 / 28%);
    border-radius: 3px;
    flex: none;
  }

  /* Scrolls rather than growing: the library gets longer every time a template is added, and a
     menu taller than the window is worse than one that scrolls. */
  .shelves {
    overflow-y: auto;
    max-height: 52vh;
    padding: 0 1px 1px;
  }

  .sources {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  .source {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    padding: 5px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .source:hover:enabled {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(255 255 255 / 5%);
  }

  .source:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .source.compact {
    display: grid;
    padding: 8px;
    grid-column: 1 / -1;
    grid-template-columns: 48px 1fr;
    grid-template-rows: 1fr 1fr;
    column-gap: 10px;
  }

  .source.compact .preview {
    width: 48px;
    height: 48px;
    margin: 0;
    grid-row: 1 / 3;
  }

  .source.compact .label {
    align-self: end;
    font-size: 11.5px;
    font-weight: 600;
  }

  .source.compact .detail {
    align-self: start;
  }

  /* Proportions come from the selected size, so a swatch is read at the shape it will be. */
  .preview {
    display: block;
    overflow: hidden;
    border-radius: 3px;
    margin-bottom: 6px;
    background: #fbfbf9;
  }

  .preview :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .label {
    overflow: hidden;
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    overflow: hidden;
    min-height: 12px;
    margin-top: 1px;
    color: var(--quiet);
    font-size: 9.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
