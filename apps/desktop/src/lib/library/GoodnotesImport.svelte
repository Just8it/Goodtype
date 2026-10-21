<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { focusTrap } from "../workspace/focus";
  import {
    cancelImporter,
    checkImporterUpdate,
    dismissImporter,
    importerBusy,
    importerStatus,
    installImporter,
    megabytes,
    pickImportSource,
    startImport,
    summarize,
    updateImporter,
    type ImporterStatus,
    type NotebookOutcome,
  } from "../ipc/importer";

  /**
   * Goodnotes import, in the same panel as a new notebook. A conversion can run for minutes, so
   * closing the panel never stops it: the job lives in Rust, the Library shows an "Import running"
   * button that brings the panel back, and leaving the Library and returning finds it where
   * it was. Nothing is downloaded until the download button is pressed.
   */
  let {
    folder,
    folderName,
    onClose,
    onShowFolder,
    onImported,
  }: {
    /** The Library folder the import goes into (relative; empty is the Library root). */
    folder: string;
    folderName: string;
    onClose: () => void;
    onShowFolder: (path: string) => void;
    onImported: () => void;
  } = $props();

  let status = $state<ImporterStatus | null>(null);
  let failure = $state<string | null>(null);
  let allowArtwork = $state(false);
  let heading = $state<HTMLElement>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let announcedResult = false;

  const activity = $derived(status?.activity ?? { kind: "idle" as const });
  const busy = $derived(importerBusy(activity));
  const checking = $derived(activity.kind === "checking");
  const downloadRelease = $derived(status?.update ?? status?.release);

  async function refresh() {
    try {
      status = await importerStatus();
      failure = null;
    } catch (error) {
      failure = message(error);
    }
    const running = status ? importerBusy(status.activity) : false;
    if (running && !timer) timer = setInterval(() => void refresh(), 400);
    if (!running && timer) {
      clearInterval(timer);
      timer = undefined;
    }
    if (status?.activity.kind === "finished" && !announcedResult) {
      announcedResult = true;
      onImported();
    }
  }

  onMount(() => {
    void refresh();
    void tick().then(() => heading?.focus());
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  function message(error: unknown) {
    return error instanceof Error ? error.message : String(error);
  }

  async function act(action: () => Promise<unknown>) {
    failure = null;
    let actionProblem: string | null = null;
    try {
      await action();
    } catch (error) {
      actionProblem = message(error);
    }
    await refresh();
    if (actionProblem) failure = actionProblem;
  }

  const download = () => act(status?.update ? updateImporter : installImporter);
  const checkUpdate = () => act(checkImporterUpdate);
  const update = () => act(updateImporter);
  const cancel = () => act(cancelImporter);
  const pick = () => act(pickImportSource);
  const begin = (into: string | null = null, artwork = allowArtwork) => {
    announcedResult = false;
    return act(() => startImport(folder, artwork, into));
  };
  async function finish() {
    await act(dismissImporter);
    onClose();
  }
  /** Back to the form after a result, keeping the chosen file. */
  const again = () => act(dismissImporter);

  /** Closing hides the panel; a running job carries on and the Library keeps showing it. */
  function close() {
    if (busy || activity.kind === "finished" || activity.kind === "importFailed") onClose();
    else void finish();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      close();
    }
  }

  const today = new Date().toISOString().slice(0, 10);

  const outcomeLabel: Record<NotebookOutcome["status"], string> = {
    imported: "Imported",
    alreadyThere: "Already imported",
    needsArtwork: "Needs vector artwork",
    failed: "Failed",
    cancelled: "Cancelled",
  };

  function pagesLabel(count: number) {
    return count === 1 ? "1 page" : `${count} pages`;
  }

  /** One line under a notebook's name: what happened, in this panel's words. */
  function detail(notebook: NotebookOutcome): string {
    const parts = [outcomeLabel[notebook.status]];
    if (notebook.status === "imported") {
      parts.push(pagesLabel(notebook.pages));
      if (notebook.artworkPages) parts.push(`${pagesLabel(notebook.artworkPages)} with vector artwork`);
    }
    if (notebook.status === "needsArtwork" && notebook.error) {
      // The importer names the first page that needs it and why.
      const match = /page (\d+) needs vector artwork \(([^)]+)\)/.exec(notebook.error);
      if (match) parts.push(`Page ${match[1]}: ${match[2]}`);
    }
    return parts.join(" · ");
  }

  function resultSentence(result: Extract<typeof activity, { kind: "finished" }>["result"]) {
    const counts = summarize(result);
    const parts: string[] = [];
    if (counts.imported) parts.push(`${counts.imported} ${counts.imported === 1 ? "notebook" : "notebooks"} imported`);
    if (counts.alreadyThere) parts.push(`${counts.alreadyThere} already imported`);
    if (counts.needsArtwork) parts.push(`${counts.needsArtwork} need vector artwork`);
    if (counts.failed) parts.push(`${counts.failed} failed`);
    if (counts.cancelled) parts.push(`${counts.cancelled} cancelled`);
    if (!parts.length) return result.cancelled ? "Import cancelled. Nothing was imported." : "Nothing was imported.";
    return `${parts.join(", ")}.`;
  }
</script>

<svelte:window onkeydown={keydown} />
<div class="scrim" role="presentation" onpointerdown={(event) => { if (event.target === event.currentTarget) close(); }}>
  <div use:focusTrap class="panel" role="dialog" aria-modal="true" aria-labelledby="goodnotes-import-title" tabindex="-1">
  <header>
    <div class="titles">
      <h2 id="goodnotes-import-title" tabindex="-1" bind:this={heading}>Import from Goodnotes</h2>
      <p>
        Turn your Goodnotes notebooks into ordinary Goodtype notebooks.
        {#if status?.installed && (!busy || checking)}<span class="version">Importer {status.installed} · available offline</span>{/if}
      </p>
    </div>
    <button type="button" class="close" aria-label="Close" title={busy ? "Close — the import keeps running" : "Close"} onclick={close}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
    </button>
  </header>

  <div class="body" aria-live="polite">
    {#if !status}
      <p class="lead">Reading importer status…</p>
    {:else if !status.platformSupported}
      <p class="lead">The Goodnotes importer is currently available for Windows (x64). Goodtype works without it.</p>
    {:else if activity.kind === "downloading"}
      <p class="status">Downloading the importer from GitHub…</p>
      <div class="progress" role="progressbar" aria-label="Download" aria-valuemin="0" aria-valuemax={activity.total} aria-valuenow={activity.received}>
        <span style:transform={`scaleX(${activity.received / Math.max(activity.total, 1)})`}></span>
      </div>
      <div class="row">
        <span class="figure">{megabytes(activity.received)} of {megabytes(activity.total)}</span>
        <div class="spacer"></div>
        <button type="button" class="control" onclick={cancel}>Cancel</button>
      </div>
    {:else if activity.kind === "installing"}
      <p class="status">Verifying and installing the package…</p>
      <div class="progress indeterminate" role="progressbar" aria-label="Installation"><span></span></div>
    {:else if activity.kind === "importing" || activity.kind === "publishing"}
      <p class="status">
        {#if activity.kind === "publishing"}
          Checking notebooks and adding them to the Library…
        {:else if activity.cancelling}
          Cancelling. Completed notebooks will be kept…
        {:else if activity.total > 0}
          Converting notebook {Math.min(activity.done + 1, activity.total)} of {activity.total}…
        {:else}
          Reading the file…
        {/if}
      </p>
      {#if activity.kind === "importing" && activity.total > 0}
        <div class="progress" role="progressbar" aria-label="Import" aria-valuemin="0" aria-valuemax={activity.total} aria-valuenow={activity.done}>
          <span style:transform={`scaleX(${activity.done / activity.total})`}></span>
        </div>
      {:else}
        <div class="progress indeterminate" role="progressbar" aria-label="Import"><span></span></div>
      {/if}
      <div class="row">
        <span class="hint">You can keep working in Goodtype while the import runs.</span>
        <div class="spacer"></div>
        {#if activity.kind === "importing"}
          <button type="button" class="control" disabled={activity.cancelling} onclick={cancel}>Cancel</button>
        {/if}
      </div>
    {:else if activity.kind === "finished"}
      {@const result = activity.result}
      {@const counts = summarize(result)}
      <p class="status">{resultSentence(result)}</p>
      {#if result.problem}<p class="problem" role="alert">{result.problem}</p>{/if}
      {#if result.notebooks.length}
        <ul class="outcomes">
          {#each result.notebooks as notebook (notebook.source)}
            <li class={notebook.status}>
              <svg viewBox="0 0 24 24" aria-hidden="true">
                {#if notebook.status === "imported" || notebook.status === "alreadyThere"}
                  <path d="m5 12.5 4.5 4.5L19 7.5" />
                {:else if notebook.status === "cancelled"}
                  <path d="M7 12h10" />
                {:else}
                  <circle cx="12" cy="12" r="8" />
                  <path d="M12 8v4.5M12 15.8v.2" />
                {/if}
              </svg>
              <div class="outcome">
                <span class="name">{notebook.source}</span>
                <span class="detail">{detail(notebook)}</span>
                {#if notebook.error && notebook.status !== "needsArtwork"}<span class="error">{notebook.error}</span>{/if}
                {#if notebook.warnings.length}
                  <details>
                    <summary>{notebook.warnings.length === 1 ? "1 note" : `${notebook.warnings.length} notes`}</summary>
                    <ul>{#each notebook.warnings as warning}<li>{warning}</li>{/each}</ul>
                  </details>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if counts.needsArtwork}
        <p class="note">
          Some content needs vector artwork, including outline or dashed strokes, unsupported
          shapes and cropped images. It stays visible and can be moved as artwork, but its
          individual strokes cannot be edited.
        </p>
      {/if}
      <div class="actions">
        {#if counts.needsArtwork && status.source}
          <button type="button" class="control" onclick={() => void begin(result.batch, true)}>
            Import remaining notebooks with artwork
          </button>
        {/if}
        <button type="button" class="control" onclick={again}>Import another file</button>
        <div class="spacer"></div>
        {#if result.batch}
          <button type="button" class="control" onclick={() => { const batch = result.batch; void finish().then(() => batch && onShowFolder(batch)); }}>Open folder</button>
        {/if}
        <button type="button" class="primary" onclick={() => void finish()}>Done</button>
      </div>
    {:else if activity.kind === "importFailed"}
      <p class="status">The import could not be completed.</p>
      <p class="problem" role="alert">{activity.message}</p>
      <p class="hint">Your Goodnotes file is unchanged. No incomplete notebooks were added to the Library.</p>
      <div class="actions">
        <div class="spacer"></div>
        <button type="button" class="control" onclick={again}>Back</button>
        <button type="button" class="primary" onclick={() => void begin()}>Try again</button>
      </div>
    {:else if !status.installed}
      <p class="lead">
        Goodnotes files are converted by an optional importer. Goodtype downloads it only when
        you request it here, and works without it.
      </p>
      <dl class="facts">
        <dt>Source</dt>
        <dd>
          GitHub release “Goodtype Goodnotes Importer”{#if downloadRelease}, version {downloadRelease.version} ·
            {megabytes(downloadRelease.sizeBytes)}{/if}
        </dd>
        <dt>What is sent</dt>
        <dd>Only the download request. Your notes stay on this device; no account is needed.</dd>
        <dt>After download</dt>
        <dd>Import works offline. Imported notebooks do not need the importer.</dd>
      </dl>
      {#if status.installedProblem}
        <p class="problem" role="alert">The installed importer cannot be used: {status.installedProblem}</p>
      {/if}
      {#if activity.kind === "installFailed"}
        <p class="problem" role="alert">Download failed: {activity.message}. The installed version has been kept.</p>
      {/if}
      <div class="actions">
        <button type="button" class="control" onclick={close}>Cancel</button>
        {#if downloadRelease}
          <button type="button" class="primary" disabled={checking} onclick={download}>
            {activity.kind === "installFailed" ? "Retry download" : `Download importer (${megabytes(downloadRelease.sizeBytes)})`}
          </button>
        {:else}
          <p class="hint">No verified importer release is available for this version of Goodtype yet.</p>
        {/if}
        {#if !status.update}
          <button type="button" class="control" disabled={checking} onclick={checkUpdate}>{checking ? "Checking…" : "Check for updates"}</button>
        {/if}
      </div>
      {#if status.updateProblem && !checking}
        <p class="problem" role="alert">Update check failed: {status.updateProblem}</p>
      {:else if status.updateChecked && !status.update && !checking}
        <p class="hint" role="status">No newer importer version is available.</p>
      {/if}
    {:else}
      <div class="updates">
        {#if status.update}
          <div class="row">
            <span class="hint">Version {status.update.version} available · {megabytes(status.update.sizeBytes)}</span>
            <button type="button" class="primary" disabled={checking} onclick={update}>Update</button>
          </div>
        {:else}
          <div class="row">
            <span class="hint" role="status">
              {#if checking}
                Checking GitHub for a newer importer…
              {:else if status.updateChecked && !status.updateProblem}
                No updates available. Importer {status.installed} is up to date.
              {:else}
                New versions are downloaded from GitHub only when requested.
              {/if}
            </span>
            <button type="button" class="control" disabled={checking} onclick={checkUpdate}>{checking ? "Checking…" : "Check for updates"}</button>
          </div>
        {/if}
        {#if status.updateProblem && !checking}
          <p class="problem" role="alert">Update check failed: {status.updateProblem}. You can keep importing offline.</p>
        {/if}
        {#if activity.kind === "installFailed"}
          <p class="problem" role="alert">Update failed: {activity.message}. The previous version is still available.</p>
        {/if}
      </div>
      <div class="form">
        <div class="field">
          <span class="label" id="goodnotes-source-label">File</span>
          <div class="pickrow">
            <button type="button" class="control" aria-describedby="goodnotes-source-help" onclick={pick}>
              {status.source ? "Choose another file…" : "Choose file…"}
            </button>
            {#if status.source}
              <span class="chosen" aria-labelledby="goodnotes-source-label">{status.source.name} · {megabytes(status.source.sizeBytes)}</span>
            {/if}
          </div>
          <small id="goodnotes-source-help">A single Goodnotes document (.goodnotes) or backup (.zip). The source file is not changed.</small>
        </div>
        <div class="field">
          <span class="label">Destination</span>
          <span class="destination">New folder “Goodnotes Import {today}” in “{folderName}”</span>
          <small>Each notebook stays separate and backup folders are preserved. Every imported notebook includes its original file.</small>
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={allowArtwork} />
          <span>
            <span class="label">Keep unsupported content as vector artwork</span>
            <small>
              Outline and dashed strokes, unsupported shapes and cropped images stay visible as
              movable artwork. Without this option, affected notebooks are skipped and listed in the results.
            </small>
          </span>
        </label>
      </div>
      <div class="actions">
        <span class="hint">Text boxes, sticky notes, audio and partial erasures are not supported.</span>
        <div class="spacer"></div>
        <button type="button" class="control" onclick={close}>Cancel</button>
        <button type="button" class="primary" disabled={!status.source || checking} onclick={() => void begin()}>Import</button>
      </div>
    {/if}
    {#if failure}<p class="problem" role="alert">{failure}</p>{/if}
  </div>
  </div>
</div>

<style>
  /* The new-notebook panel's frame, so both Library dialogs are one family. */
  .scrim {
    position: absolute;
    z-index: 60;
    display: grid;
    padding: 20px;
    background: rgb(10 12 16 / 62%);
    inset: 0;
    place-items: center;
    animation: scrim-in 140ms ease-out;
  }

  .panel {
    box-sizing: border-box;
    width: min(620px, 100%);
    max-height: 100%;
    padding: 22px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius-lg, 10px);
    background: var(--panel, #23272f);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    color: var(--text, #e9ebee);
    font-size: var(--text-md, 13px);
    overflow-y: auto;
    animation: panel-in 160ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  header {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    margin-bottom: 18px;
  }

  .titles {
    flex: 1;
    min-width: 0;
  }

  .titles p {
    margin: 5px 0 0;
    color: var(--muted, #aeb5be);
  }

  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  h2:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 3px;
    border-radius: 2px;
  }

  .version {
    display: block;
    margin-top: 2px;
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm, 11px);
  }

  .spacer {
    flex: 1;
  }

  .body {
    display: grid;
    gap: 14px;
  }

  .updates {
    display: grid;
    gap: 10px;
  }

  .updates .row {
    justify-content: space-between;
    flex-wrap: wrap;
  }

  p {
    margin: 0;
    line-height: 1.5;
  }

  .lead {
    color: var(--muted, #aeb5be);
  }

  /* What is happening now, or what happened: the line the eye should land on. */
  .status {
    color: var(--text, #e9ebee);
    font-weight: 500;
  }

  .hint,
  small {
    color: var(--muted, #aeb5be);
    font-size: var(--text-sm, 11px);
    line-height: 1.45;
  }

  .note {
    color: var(--muted, #aeb5be);
    font-size: var(--text-sm, 11px);
  }

  .problem,
  .outcome .error {
    color: #f0aaa6;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 9px 18px;
    margin: 2px 0 0;
    line-height: 1.45;
  }

  .facts dt {
    color: var(--muted, #aeb5be);
  }

  .facts dd {
    margin: 0;
  }

  .row,
  .actions,
  .pickrow {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .actions {
    justify-content: flex-end;
    margin-top: 6px;
  }

  .figure {
    color: var(--muted, #aeb5be);
    font-variant-numeric: tabular-nums;
  }

  .progress {
    position: relative;
    height: 6px;
    overflow: hidden;
    border-radius: 3px;
    background: rgb(255 255 255 / 8%);
  }

  /* Scaled, not resized: the bar updates while the writer may be drawing elsewhere, and a
     transform never makes the page lay out again. */
  .progress span {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--blueprint, #4c8df0);
    transform: scaleX(0);
    transform-origin: left;
    transition: transform 200ms ease-out;
  }

  .progress.indeterminate span {
    right: auto;
    width: 30%;
    transform: none;
    animation: travel 1.3s ease-in-out infinite;
  }

  .form {
    display: grid;
    gap: 14px;
  }

  .field {
    display: grid;
    gap: 5px;
  }

  .label {
    color: var(--text, #e9ebee);
    font-weight: 500;
  }

  .chosen {
    color: var(--text, #e9ebee);
    overflow-wrap: anywhere;
  }

  .destination {
    color: var(--text, #e9ebee);
  }

  .check {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 10px;
    align-items: start;
    cursor: pointer;
  }

  .check > span {
    display: grid;
    gap: 3px;
  }

  .check input {
    width: 16px;
    height: 16px;
    margin: 2px 0 0;
    accent-color: var(--blueprint, #4c8df0);
  }

  .outcomes {
    display: grid;
    gap: 2px;
    max-height: 300px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }

  .outcomes > li {
    display: grid;
    grid-template-columns: 18px 1fr;
    gap: 10px;
    padding: 7px 0;
    border-top: 1px solid var(--edge-soft, rgb(255 255 255 / 7%));
  }

  .outcomes > li:first-child {
    border-top: 0;
  }

  .outcomes svg {
    width: 18px;
    height: 18px;
    margin-top: 1px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 2;
  }

  .imported svg,
  .alreadyThere svg {
    color: #7fd1a3;
  }

  .needsArtwork svg {
    color: #e9c46a;
  }

  .failed svg {
    color: #f0aaa6;
  }

  .cancelled svg {
    color: var(--quiet, #6a727c);
  }

  .outcome {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .outcome .name {
    overflow-wrap: anywhere;
  }

  .outcome .detail {
    color: var(--muted, #aeb5be);
    font-size: var(--text-sm, 11px);
  }

  details {
    color: var(--muted, #aeb5be);
    font-size: var(--text-sm, 11px);
  }

  summary {
    width: max-content;
    cursor: pointer;
  }

  details ul {
    margin: 4px 0 0;
    padding-left: 16px;
  }

  .control,
  .primary,
  .close {
    display: inline-flex;
    min-height: var(--control, 36px);
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius, 6px);
    font: inherit;
    font-size: var(--text-md, 13px);
    white-space: nowrap;
    cursor: pointer;
    touch-action: manipulation;
  }

  .control {
    background: rgb(255 255 255 / 5%);
    color: var(--text, #e9ebee);
  }

  .control:hover:not(:disabled) {
    background: var(--wash, rgb(255 255 255 / 8%));
  }

  .primary {
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font-weight: 600;
  }

  .primary:hover:not(:disabled) {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 20%);
  }

  .close {
    width: var(--control-dense, 30px);
    min-height: var(--control-dense, 30px);
    padding: 0;
    background: transparent;
    color: var(--muted, #aeb5be);
  }

  .close:hover {
    background: var(--wash, rgb(255 255 255 / 8%));
    color: var(--text, #e9ebee);
  }

  .close svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: 2;
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  button:focus-visible,
  input:focus-visible,
  summary:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 2px;
  }

  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }

  @keyframes panel-in {
    from {
      opacity: 0;
      transform: translateY(7px) scale(0.99);
    }
  }

  @keyframes travel {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  @media (pointer: coarse) {
    .control,
    .primary {
      min-height: var(--control-touch, 44px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .panel,
    .progress.indeterminate span {
      animation: none;
    }

    .progress span {
      transition: none;
    }
  }
</style>
