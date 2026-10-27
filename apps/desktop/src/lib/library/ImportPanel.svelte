<script lang="ts">
  import { t } from "../i18n";
  import { onDestroy, onMount, tick } from "svelte";
  import { focusTrap } from "../workspace/focus";
  import { errorDetails } from "../ipc/errors";
  import {
    IMPORTERS,
    allImporterStatus,
    cancelImporter,
    checkImporterUpdate,
    dismissImporter,
    importerBusy,
    installImporter,
    megabytes,
    pickImportSource,
    startImport,
    summarize,
    updateImporter,
    type ImporterId,
    type ImporterStatus,
    type NotebookOutcome,
  } from "../ipc/importer";

  /**
   * Import from Goodnotes or OneNote, in the same panel as a new notebook. The two sources are
   * tabs on the title row; each importer keeps its own download, chosen file and job in Rust, so
   * switching tabs never disturbs the other. A conversion can run for minutes, so closing the
   * panel never stops it: the Library shows an "Import running" button that brings the panel
   * back. Nothing is downloaded until the download button is pressed.
   */
  let {
    tool = $bindable(),
    folder,
    folderName,
    onClose,
    onShowFolder,
    onImported,
  }: {
    /** The source on show; the Library remembers it for the next time the panel opens. */
    tool: ImporterId;
    /** The Library folder the import goes into (relative; empty is the Library root). */
    folder: string;
    folderName: string;
    onClose: () => void;
    onShowFolder: (path: string) => void;
    onImported: () => void;
  } = $props();

  /** What differs between the sources: names, file types, and what each cannot carry over. */
  const SOURCES: Record<
    ImporterId,
    {
      name: string;
      lead: string;
      release: string;
      fileHelp: string;
      destinationHelp: string;
      /** Goodnotes content Goodtype cannot edit natively may come over as vector artwork. */
      artwork: boolean;
      limits: string;
      batch: string;
    }
  > = $derived({
    goodnotes: {
      name: "Goodnotes",
      lead: $t("library.turnYourGoodnotesNotebooksIntoOrdinaryGoodtype"),
      release: "Goodtype Goodnotes Importer",
      fileHelp: $t("library.aSingleGoodnotesDocumentGoodnotesOrBackup"),
      destinationHelp:
        $t("library.eachNotebookStaysSeparateAndBackupFolders"),
      artwork: true,
      limits: $t("library.textBoxesStickyNotesAudioAndPartial"),
      batch: $t("library.goodnotesImport"),
    },
    onenote: {
      name: "OneNote",
      lead: $t("library.turnYourOneNoteNotebooksAndSectionsInto"),
      release: "Goodtype OneNote Importer",
      fileHelp:
        $t("library.aSectionOneOrNotebookPackageOnepkg"),
      destinationHelp:
        $t("library.eachSectionBecomesANotebookNotebooksAnd"),
      artwork: false,
      limits: $t("library.audioAndVideoPlaybackLinksBetweenOneNote"),
      batch: $t("library.onenoteImport"),
    },
  });

  let statuses = $state<Record<ImporterId, ImporterStatus> | null>(null);
  let failure = $state<string | null>(null);
  let allowArtwork = $state(false);
  let tabs = $state<Partial<Record<ImporterId, HTMLButtonElement>>>({});
  let timer: ReturnType<typeof setInterval> | undefined;
  /** Each source's finished result is announced to the Library once. */
  const announced = new Set<ImporterId>();

  const source = $derived(SOURCES[tool]);
  const status = $derived(statuses?.[tool] ?? null);
  const activity = $derived(status?.activity ?? { kind: "idle" as const });
  const busy = $derived(importerBusy(activity));
  const checking = $derived(activity.kind === "checking");
  const downloadRelease = $derived(status?.update ?? status?.release);

  async function refresh() {
    try {
      statuses = await allImporterStatus();
      failure = null;
    } catch (error) {
      failure = errorDetails(error);
    }
    const running = statuses ? IMPORTERS.some((id) => importerBusy(statuses![id].activity)) : false;
    if (running && !timer) timer = setInterval(() => void refresh(), 400);
    if (!running && timer) {
      clearInterval(timer);
      timer = undefined;
    }
    for (const id of IMPORTERS) {
      const kind = statuses?.[id].activity.kind;
      if (kind === "finished" && !announced.has(id)) {
        announced.add(id);
        onImported();
      } else if (kind !== "finished") {
        announced.delete(id);
      }
    }
  }

  onMount(() => {
    void refresh();
    void tick().then(() => tabs[tool]?.focus());
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });


  async function act(action: () => Promise<unknown>) {
    failure = null;
    let actionProblem: string | null = null;
    try {
      await action();
    } catch (error) {
      actionProblem = errorDetails(error);
    }
    await refresh();
    if (actionProblem) failure = actionProblem;
  }

  const download = () => act(() => (status?.update ? updateImporter(tool) : installImporter(tool)));
  const checkUpdate = () => act(() => checkImporterUpdate(tool));
  const update = () => act(() => updateImporter(tool));
  const cancel = () => act(() => cancelImporter(tool));
  const pick = () => act(() => pickImportSource(tool));
  const begin = (into: string | null = null, artwork = allowArtwork) => {
    announced.delete(tool);
    return act(() => startImport(tool, folder, artwork, into));
  };
  async function finish() {
    await act(() => dismissImporter(tool));
    onClose();
  }
  /** Back to the form after a result, keeping the chosen file. */
  const again = () => act(() => dismissImporter(tool));

  function choose(id: ImporterId) {
    if (id === tool) return;
    tool = id;
    failure = null;
    allowArtwork = false;
    void refresh();
  }

  /** Arrow keys move between the sources, as in any tab list. */
  function tabKey(event: KeyboardEvent) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    const target =
      event.key === "Home"
        ? IMPORTERS[0]
        : event.key === "End"
          ? IMPORTERS[IMPORTERS.length - 1]
          : step
            ? IMPORTERS[(IMPORTERS.indexOf(tool) + step + IMPORTERS.length) % IMPORTERS.length]
            : null;
    if (!target) return;
    event.preventDefault();
    choose(target);
    void tick().then(() => tabs[target]?.focus());
  }

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

  const outcomeLabel: Record<NotebookOutcome["status"], string> = $derived({
    imported: $t("library.imported"),
    alreadyThere: $t("library.alreadyImported"),
    needsArtwork: $t("library.needsVectorArtwork"),
    failed: $t("library.failed"),
    cancelled: $t("library.cancelled"),
  });

  function pagesLabel(count: number) {
    return $t("counts.pages", { count });
  }

  /** One line under a notebook's name: what happened, in this panel's words. */
  function detail(notebook: NotebookOutcome): string {
    const parts = [outcomeLabel[notebook.status]];
    if (notebook.status === "imported") {
      parts.push(pagesLabel(notebook.pages));
      if (notebook.artworkPages) parts.push($t("library.withVectorArtwork", { "value1": pagesLabel(notebook.artworkPages) }));
    }
    if (notebook.status === "needsArtwork" && notebook.error) {
      // The importer names the first page that needs it and why.
      const match = /page (\d+) needs vector artwork \(([^)]+)\)/.exec(notebook.error);
      if (match) parts.push($t("library.page2", { "value1": match[1], "value2": match[2] }));
    }
    return parts.join(" · ");
  }

  function resultSentence(result: Extract<typeof activity, { kind: "finished" }>["result"]) {
    const counts = summarize(result);
    const parts: string[] = [];
    if (counts.imported) parts.push($t("library.imported2", { "imported": counts.imported, "value2": $t("counts.notebook", { count: counts.imported }).replace(/^\S+\s+/, "") }));
    if (counts.alreadyThere) parts.push($t("library.alreadyImported2", { "alreadyThere": counts.alreadyThere }));
    if (counts.needsArtwork) parts.push($t("library.needVectorArtwork", { "needsArtwork": counts.needsArtwork }));
    if (counts.failed) parts.push($t("library.failed2", { "failed": counts.failed }));
    if (counts.cancelled) parts.push($t("library.cancelled2", { "cancelled": counts.cancelled }));
    if (!parts.length) return result.cancelled ? $t("library.importCancelledNothingWasImported") : $t("library.nothingWasImported");
    return `${parts.join(", ")}.`;
  }
</script>

<svelte:window onkeydown={keydown} />
<div class="scrim" role="presentation" onpointerdown={(event) => { if (event.target === event.currentTarget) close(); }}>
  <div use:focusTrap class="panel" role="dialog" aria-modal="true" aria-label={$t("library.importNotebooks")} tabindex="-1">
  <header>
    <div class="titles">
      <!-- The sources are the title: choosing where notebooks come from is the panel's first step. -->
      <div class="sources" role="tablist" aria-label={$t("library.importFrom")}>
        {#each IMPORTERS as id (id)}
          {@const running = statuses ? importerBusy(statuses[id].activity) : false}
          <button
            type="button"
            role="tab"
            id={`import-tab-${id}`}
            aria-selected={id === tool}
            aria-controls="import-source-panel"
            tabindex={id === tool ? 0 : -1}
            class:on={id === tool}
            bind:this={tabs[id]}
            onclick={() => choose(id)}
            onkeydown={tabKey}
          >
            {SOURCES[id].name}
            {#if running && id !== tool}<span class="running" title={$t("library.importRunning2")}><span class="sr">{$t("library.importRunning")}</span></span>{/if}
          </button>
        {/each}
      </div>
      <p>
        {source.lead}
        {#if status?.installed && (!busy || checking)}<span class="version">{$t("library.importerAvailableOffline", { "installed": status.installed })}</span>{/if}
      </p>
    </div>
    <button type="button" class="close" aria-label={$t("library.close")} title={busy ? $t("library.closeTheImportKeepsRunning") : $t("library.close")} onclick={close}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
    </button>
  </header>

  <div class="body" id="import-source-panel" role="tabpanel" aria-labelledby={`import-tab-${tool}`} aria-live="polite">
    {#if !status}
      <p class="lead">{$t("library.readingImporterStatus")}</p>
    {:else if !status.platformSupported}
      <p class="lead">{$t("library.theImporterIsCurrentlyAvailableForWindows", { "name": source.name })}</p>
    {:else if activity.kind === "downloading"}
      <p class="status">{$t("library.downloadingTheImporterFromGitHub")}</p>
      <div class="progress" role="progressbar" aria-label={$t("library.download")} aria-valuemin="0" aria-valuemax={activity.total} aria-valuenow={activity.received}>
        <span style:transform={`scaleX(${activity.received / Math.max(activity.total, 1)})`}></span>
      </div>
      <div class="row">
        <span class="figure">{$t("library.of", { "value1": megabytes(activity.received), "value3": megabytes(activity.total) })}</span>
        <div class="spacer"></div>
        <button type="button" class="control" onclick={cancel}>{$t("library.cancel")}</button>
      </div>
    {:else if activity.kind === "installing"}
      <p class="status">{$t("library.verifyingAndInstallingThePackage")}</p>
      <div class="progress indeterminate" role="progressbar" aria-label={$t("library.installation")}><span></span></div>
    {:else if activity.kind === "importing" || activity.kind === "publishing"}
      <p class="status">
        {#if activity.kind === "publishing"}
          {$t("library.checkingNotebooksAndAddingThemToThe")}
        {:else if activity.cancelling}
          {$t("library.cancellingCompletedNotebooksWillBeKept")}
        {:else if activity.total > 0}
          {$t("library.convertingNotebookOf", { "value2": Math.min(activity.done + 1, activity.total), "total": activity.total })}
        {:else}
          {$t("library.readingTheFile")}
        {/if}
      </p>
      {#if activity.kind === "importing" && activity.total > 0}
        <div class="progress" role="progressbar" aria-label={$t("library.import")} aria-valuemin="0" aria-valuemax={activity.total} aria-valuenow={activity.done}>
          <span style:transform={`scaleX(${activity.done / activity.total})`}></span>
        </div>
      {:else}
        <div class="progress indeterminate" role="progressbar" aria-label={$t("library.import")}><span></span></div>
      {/if}
      <div class="row">
        <span class="hint">{$t("library.youCanKeepWorkingInGoodtypeWhile")}</span>
        <div class="spacer"></div>
        {#if activity.kind === "importing"}
          <button type="button" class="control" disabled={activity.cancelling} onclick={cancel}>{$t("library.cancel")}</button>
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
                    <summary>{$t("counts.notes", { count: notebook.warnings.length })}</summary>
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
          {$t("library.someContentNeedsVectorArtworkIncludingOutline")}
        </p>
      {/if}
      <div class="actions">
        {#if counts.needsArtwork && status.source}
          <button type="button" class="control" onclick={() => void begin(result.batch, true)}>
            {$t("library.importRemainingNotebooksWithArtwork")}
          </button>
        {/if}
        <button type="button" class="control" onclick={again}>{$t("library.importAnotherFile")}</button>
        <div class="spacer"></div>
        {#if result.batch}
          <button type="button" class="control" onclick={() => { const batch = result.batch; void finish().then(() => batch && onShowFolder(batch)); }}>{$t("library.openFolder")}</button>
        {/if}
        <button type="button" class="primary" onclick={() => void finish()}>{$t("library.done")}</button>
      </div>
    {:else if activity.kind === "importFailed"}
      <p class="status">{$t("library.theImportCouldNotBeCompleted")}</p>
      <p class="problem" role="alert">{activity.message}</p>
      <p class="hint">{$t("library.yourFileIsUnchangedNoIncompleteNotebooks", { "name": source.name })}</p>
      <div class="actions">
        <div class="spacer"></div>
        <button type="button" class="control" onclick={again}>{$t("library.back")}</button>
        <button type="button" class="primary" onclick={() => void begin()}>{$t("library.tryAgain")}</button>
      </div>
    {:else if !status.installed}
      <p class="lead">
        {$t("library.filesAreConvertedByAnOptionalImporter", { "name": source.name })}
      </p>
      <dl class="facts">
        <dt>{$t("library.source")}</dt>
        <dd>
          {$t("library.githubRelease", { "release": source.release })}{#if downloadRelease}{$t("library.version", { "version": downloadRelease.version, "value4": megabytes(downloadRelease.sizeBytes) })}{/if}
        </dd>
        <dt>{$t("library.whatIsSent")}</dt>
        <dd>{$t("library.onlyTheDownloadRequestYourNotesStay")}</dd>
        <dt>{$t("library.afterDownload")}</dt>
        <dd>{$t("library.importWorksOfflineImportedNotebooksDoNot")}</dd>
      </dl>
      {#if status.installedProblem}
        <p class="problem" role="alert">{$t("library.theInstalledImporterCannotBeUsed", { "installedProblem": status.installedProblem })}</p>
      {/if}
      {#if activity.kind === "installFailed"}
        <p class="problem" role="alert">{$t("library.downloadFailedTheInstalledVersionHasBeen", { "message": activity.message })}</p>
      {/if}
      <div class="actions">
        <button type="button" class="control" onclick={close}>{$t("library.cancel")}</button>
        {#if downloadRelease}
          <button type="button" class="primary" disabled={checking} onclick={download}>
            {activity.kind === "installFailed" ? $t("library.retryDownload") : $t("library.downloadImporter", { "value1": megabytes(downloadRelease.sizeBytes) })}
          </button>
        {:else}
          <p class="hint">{$t("library.noVerifiedImporterReleaseIsAvailableFor")}</p>
        {/if}
        {#if !status.update}
          <button type="button" class="control" disabled={checking} onclick={checkUpdate}>{checking ? $t("library.checking") : $t("library.checkForUpdates")}</button>
        {/if}
      </div>
      {#if status.updateProblem && !checking}
        <p class="problem" role="alert">{$t("library.updateCheckFailed", { "updateProblem": status.updateProblem })}</p>
      {:else if status.updateChecked && !status.update && !checking}
        <p class="hint" role="status">{$t("library.noNewerImporterVersionIsAvailable")}</p>
      {/if}
    {:else}
      <div class="updates">
        {#if status.update}
          <div class="row">
            <span class="hint">{$t("library.versionAvailable", { "version": status.update.version, "value4": megabytes(status.update.sizeBytes) })}</span>
            <button type="button" class="primary" disabled={checking} onclick={update}>{$t("library.update")}</button>
          </div>
        {:else}
          <div class="row">
            <span class="hint" role="status">
              {#if checking}
                {$t("library.checkingGitHubForANewerImporter")}
              {:else if status.updateChecked && !status.updateProblem}
                {$t("library.noUpdatesAvailableImporterIsUpTo", { "installed": status.installed })}
              {:else}
                {$t("library.newVersionsAreDownloadedFromGitHubOnly")}
              {/if}
            </span>
            <button type="button" class="control" disabled={checking} onclick={checkUpdate}>{checking ? $t("library.checking") : $t("library.checkForUpdates")}</button>
          </div>
        {/if}
        {#if status.updateProblem && !checking}
          <p class="problem" role="alert">{$t("library.updateCheckFailedYouCanKeepImporting", { "updateProblem": status.updateProblem })}</p>
        {/if}
        {#if activity.kind === "installFailed"}
          <p class="problem" role="alert">{$t("library.updateFailedThePreviousVersionIsStill", { "message": activity.message })}</p>
        {/if}
      </div>
      <div class="form">
        <div class="field">
          <span class="label" id="import-source-label">{$t("library.file")}</span>
          <div class="pickrow">
            <button type="button" class="control" aria-describedby="import-source-help" onclick={pick}>
              {status.source ? $t("library.chooseAnotherFile") : $t("library.chooseFile")}
            </button>
            {#if status.source}
              <span class="chosen" aria-labelledby="import-source-label">{status.source.name} · {megabytes(status.source.sizeBytes)}</span>
            {/if}
          </div>
          <small id="import-source-help">{source.fileHelp}</small>
        </div>
        <div class="field">
          <span class="label">{$t("library.destination")}</span>
          <span class="destination">{$t("library.newFolderIn", { "batch": source.batch, "today": today, "folderName": folderName })}</span>
          <small>{source.destinationHelp}</small>
        </div>
        {#if source.artwork}
        <label class="check">
          <input type="checkbox" bind:checked={allowArtwork} />
          <span>
            <span class="label">{$t("library.keepUnsupportedContentAsVectorArtwork")}</span>
            <small>
              {$t("library.outlineAndDashedStrokesUnsupportedShapesAnd")}
            </small>
          </span>
        </label>
        {/if}
      </div>
      <div class="actions">
        <span class="hint">{source.limits}</span>
        <div class="spacer"></div>
        <button type="button" class="control" onclick={close}>{$t("library.cancel")}</button>
        <button type="button" class="primary" disabled={!status.source || checking} onclick={() => void begin()}>{$t("library.import")}</button>
      </div>
    {/if}
    {#if failure}<p class="problem" role="alert">{$t("errors.operation", { details: failure })}</p>{/if}
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

  /* The sources sit where the title was, at the title's size: the active one reads as the
     heading, the other as a quieter alternative one step away. */
  .sources {
    display: flex;
    gap: 20px;
    margin-bottom: 6px;
  }

  .sources button {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 2px 0 7px;
    border: 0;
    background: none;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: 20px;
    font-weight: 500;
    letter-spacing: -0.02em;
    cursor: pointer;
    transition: color 150ms ease-out;
  }

  .sources button::after {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    height: 2px;
    border-radius: 1px;
    background: var(--blueprint, #4c8df0);
    content: "";
    transform: scaleX(0);
    transition: transform 180ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .sources button:hover {
    color: var(--text, #e9ebee);
  }

  .sources button.on {
    color: var(--text, #e9ebee);
    font-weight: 600;
  }

  .sources button.on::after {
    transform: scaleX(1);
  }

  .sources button:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 3px;
    border-radius: 2px;
  }

  /* A job still running on the other source: the same pulse as the Library's import button. */
  .running {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--blueprint, #4c8df0);
    animation: running 1.2s ease-in-out infinite;
  }

  @keyframes running {
    50% {
      opacity: 0.35;
    }
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
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
    .progress.indeterminate span,
    .running {
      animation: none;
    }

    .progress span,
    .sources button,
    .sources button::after {
      transition: none;
    }
  }
</style>
