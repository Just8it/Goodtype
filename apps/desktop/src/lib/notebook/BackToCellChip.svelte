<script lang="ts">
  import { t } from "../i18n";

  /**
   * A way back to a Python cell that still has the caret but has been scrolled out of sight.
   *
   * The focused cell pins its page (`typingOnActivePage`), so the writer can look something up
   * further down without losing the editor. This chip is the return trip. It watches only the
   * focused editor, with one IntersectionObserver, so scrolling itself costs nothing here.
   */
  let { viewport, pageNumber, overPalette = false, reducedMotion = false }: {
    /** The scrolling page area: the observer's root, and where focus is watched. */
    viewport: HTMLElement | undefined;
    /** One-based number of the page with this id, as the writer sees it. */
    pageNumber: (pageId: string) => number;
    /** Lifted clear of a palette docked at the bottom. */
    overPalette?: boolean;
    /** The app's own reduced-motion setting, on top of the system preference. */
    reducedMotion?: boolean;
  } = $props();

  let host = $state<HTMLElement | null>(null);
  let editor = $state<HTMLElement | null>(null);
  let outOfView = $state(false);
  let chip = $state<HTMLButtonElement>();

  const page = $derived(host?.closest<HTMLElement>("[data-page-id]")?.dataset.pageId ?? null);
  const visible = $derived(Boolean(host && outOfView && page));

  $effect(() => {
    const root = viewport;
    if (!root) return;
    const focusIn = (event: FocusEvent) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      const next = target?.closest<HTMLElement>(".cell-editor") ?? null;
      if (next) {
        host = next;
        editor = target;
      }
    };
    const focusOut = (event: FocusEvent) => {
      const going = event.relatedTarget instanceof Node ? event.relatedTarget : null;
      // Tabbing onto the chip keeps it; anywhere else lets the cell go.
      if (going && (going === chip || host?.contains(going))) return;
      host = null;
      editor = null;
    };
    root.addEventListener("focusin", focusIn);
    root.addEventListener("focusout", focusOut);
    return () => {
      root.removeEventListener("focusin", focusIn);
      root.removeEventListener("focusout", focusOut);
    };
  });

  $effect(() => {
    const target = host;
    const root = viewport;
    outOfView = false;
    if (!target || !root) return;
    const observer = new IntersectionObserver(
      ([entry]) => { outOfView = !entry.isIntersecting; },
      { root },
    );
    observer.observe(target);
    return () => observer.disconnect();
  });

  function back() {
    const target = host;
    if (!target?.isConnected) {
      host = null;
      editor = null;
      return;
    }
    target.scrollIntoView({ block: "center" });
    (editor?.isConnected ? editor : target.querySelector<HTMLElement>("[contenteditable='true']"))
      ?.focus({ preventScroll: true });
  }

  function chipFocusOut(event: FocusEvent) {
    const going = event.relatedTarget instanceof Node ? event.relatedTarget : null;
    if (!going || !host?.contains(going)) {
      host = null;
      editor = null;
    }
  }
</script>

{#if visible && page}
  <!-- Pressing it must not take focus from the editor first, or the page it pins would let go. -->
  <button
    bind:this={chip}
    type="button"
    class="back-to-cell"
    class:over-palette={overPalette}
    class:reduced-motion={reducedMotion}
    onpointerdown={(event) => event.preventDefault()}
    onclick={back}
    onfocusout={chipFocusOut}
  >{$t("workspace.backToCellOnPage", { page: pageNumber(page) })}</button>
{/if}

<style>
  .back-to-cell {
    position: absolute;
    left: 50%;
    bottom: 28px;
    z-index: 20;
    height: var(--control);
    padding: 0 16px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-pill);
    background: var(--panel);
    box-shadow: var(--shadow-menu);
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    cursor: pointer;
    transform: translateX(-50%);
    transition: background var(--fast) ease;
    animation: chip-in var(--enter) ease-out;
  }

  .back-to-cell.over-palette { bottom: 104px; }
  /* The wash is laid over the panel, not instead of it: alone it would let the page show through. */
  .back-to-cell:hover { background: linear-gradient(var(--wash), var(--wash)), var(--panel); }
  .back-to-cell:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }

  @keyframes chip-in {
    from { opacity: 0; transform: translate(-50%, 6px); }
  }

  .back-to-cell.reduced-motion { transition: none; animation: none; }

  @media (prefers-reduced-motion: reduce) {
    .back-to-cell { transition: none; animation: none; }
  }
</style>
