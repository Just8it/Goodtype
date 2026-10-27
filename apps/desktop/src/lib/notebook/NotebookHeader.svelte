<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import NotebookTabs from "./NotebookTabs.svelte";
  import type { NotebookTab } from "./tabs";
  let { tabs, activeRoot, switchingRoot, busy, saving, warning, onSelect, onClose, onLibrary, actions }: {
    tabs: NotebookTab[]; activeRoot: string; switchingRoot: string | null;
    busy: boolean; saving: boolean; warning: boolean;
    onSelect: (root: string) => Promise<boolean>; onClose: (root: string) => Promise<void>;
    onLibrary: () => unknown; actions: Snippet;
  } = $props();
</script>

  <header class="command-strip">
    <div class="notebook-identity">
      <button
        class="home-button"
        type="button"
        aria-label={$t("workspace.openTheNotebookLibrary")}
        title={$t("workspace.notebookLibrary")}
        disabled={busy}
        onclick={onLibrary}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M3.5 11.2 12 4.5l8.5 6.7" />
          <path d="M5.8 10v8.2a1 1 0 0 0 1 1h3.4v-4.9h3.6v4.9h3.4a1 1 0 0 0 1-1V10" />
        </svg>
      </button>
      <NotebookTabs
        tabs={tabs}
        {activeRoot}
        {switchingRoot}
        {busy}
        saving={saving}
        warning={warning}
        onSelect={onSelect}
        onClose={onClose}
      />
      <button
        class="new-tab-button"
        type="button"
        aria-label={$t("workspace.openAnotherNotebook")}
        title={$t("workspace.openAnotherNotebook")}
        disabled={busy}
        onclick={onLibrary}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>
    {@render actions()}
  </header>

<style>
  button { border: 0; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  button:disabled { cursor: not-allowed; opacity: .32; }
  .command-strip {
    z-index: 30;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0 14px 0 18px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    background: var(--charcoal);
  }

  .notebook-identity {
    display: flex;
    align-items: center;
  }

  /* The gap matches the one between tabs, so the trailing "+" reads as the next seat in the row
     rather than a separate control that happens to sit nearby. */
  .notebook-identity { flex: 1; min-width: 0; gap: 4px; }
  .notebook-identity .home-button { margin-right: 6px; }

  /* Sized like the strip's other icon controls so the row reads as one set, and it replaces the
     decorative square that used to sit here — the corner was already the eye's first stop. */
  .home-button {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }

  .home-button:hover:not(:disabled) { background: var(--wash); }
  .new-tab-button {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }
  .new-tab-button:hover:not(:disabled) { background: var(--wash); color: var(--text); }


  .command-strip svg { width: var(--icon); height: var(--icon); fill: none;
    stroke: currentColor; stroke-width: var(--stroke); stroke-linecap: round; stroke-linejoin: round; }
</style>
