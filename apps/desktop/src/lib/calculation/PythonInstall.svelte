<script lang="ts">
  import { t, formatNumber, renderMessage } from "../i18n";
  import Dialog from "../workspace/Dialog.svelte";
  import type { CalculationWorkspace } from "./calculationWorkspace.svelte";
  let { workspace }: { workspace: CalculationWorkspace } = $props();

  /**
   * Where calculations get their Python. Same family as the Library's import panel: a title, a
   * short lead, plain facts about what leaves the device, and one primary action. The complete
   * download comes first because it is the choice that works for every calculation.
   */
  const local = $derived(workspace.runtime?.progress?.startsWith("local-") ?? false);
  /** The headline while busy: removing, checking an installed Python, or installing the download. */
  const activity = $derived(workspace.removing && workspace.installMessage ? renderMessage(workspace.installMessage)
    : workspace.runtime?.progress === "local-copy" ? $t("calculation.copyingLocal")
    : local || workspace.installMessage ? $t("calculation.checkingLocal") : $t("calculation.installing"));
  const size = $derived(formatNumber((workspace.runtime?.downloadBytes ?? 0) / 1048576, { maximumFractionDigits: 1 }));
  const allowed = $derived(workspace.runtime?.downloadsAllowed ?? true);
  const busy = $derived(workspace.installing || workspace.discovering);
  /** The dialog only opens with a runtime present after Settings › Change Python, which drops any
   *  local copy first — so an installed runtime here is the complete download. */
  const downloaded = $derived(workspace.runtime?.installed ?? false);
  const PROBLEMS = new Set(["calculation.setupFailed", "calculation.changeFailed", "calculation.localMissing", "calculation.localIncompatible"]);
  const failed = $derived(typeof workspace.installMessage === "object" && workspace.installMessage !== null && PROBLEMS.has(workspace.installMessage.key));
  const downloadFailed = $derived(failed && typeof workspace.installMessage === "object" && workspace.installMessage?.key === "calculation.setupFailed");
  /** Windows extended-length paths read as noise; the folder is what the writer recognises. */
  const display = (path: string) => path.replace(/^\\\\\?\\/, "");

  function dismiss() { if (workspace.installing) void workspace.cancelInstall(); else workspace.installOpen = false; }
</script>

{#if workspace.installOpen}
  <!-- Escape and the backdrop stay inert while installing; the close button cancels instead. -->
  <Dialog title={$t("calculation.installTitle")} lead={$t("calculation.installLead")} size="md"
    onClose={dismiss} dismissible={!workspace.installing} closeButton closeTitle={$t("calculation.close")}>
    {#snippet children()}
      <div class="body" aria-live="polite">
        {#if workspace.installing}
          <p class="status">{activity}</p>
          <div class="progress" role="progressbar" aria-label={activity}><span></span></div>
          {#if !workspace.removing}
            <div class="row">
              <!-- The archive being fetched, e.g. "numpy": shows the download is moving. -->
              <span class="figure">{local || workspace.installMessage ? "" : (workspace.runtime?.progress ?? "")}</span>
              <div class="spacer"></div>
              <button type="button" class="control" onclick={dismiss}>{$t("calculation.cancel")}</button>
            </div>
          {/if}
        {:else}
          <section aria-labelledby="python-download-title">
            <h3 id="python-download-title">{$t("calculation.downloadOption")}<span class="tag">{downloaded ? $t("calculation.installedTag") : $t("calculation.recommended")}</span></h3>
            <p class="lead">{$t("calculation.downloadContents")}</p>
            <dl class="facts">
              <dt>{$t("calculation.factSource")}</dt>
              <dd><code>www.python.org</code>, <code>files.pythonhosted.org</code></dd>
              <dt>{$t("calculation.factSent")}</dt>
              <dd>{$t("calculation.factSentValue")}</dd>
              <dt>{$t("calculation.factAfter")}</dt>
              <dd>{$t("calculation.factAfterValue")}</dd>
            </dl>
            {#if !allowed && !downloaded}<p class="problem">{$t("calculation.downloadsDisabled")}</p>{/if}
          </section>

          <section aria-labelledby="python-local-title">
            <h3 id="python-local-title">{$t("calculation.localOption")}</h3>
            <p class="lead">{$t("calculation.localLead")}</p>
            {#if workspace.discovering}
              <p class="hint" role="status">{$t("calculation.discoveringLocal")}</p>
            {:else if workspace.candidates.length}
              <ul class="candidates">
                {#each workspace.candidates as candidate (candidate.id)}
                  <li>
                    <code title={display(candidate.executable)}>{display(candidate.executable)}</code>
                    <button type="button" class="control" disabled={busy} onclick={() => void workspace.useLocal(candidate.id)}>{$t("calculation.useLocal")}</button>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="hint">{$t("calculation.noLocal")}</p>
            {/if}
            <button type="button" class="link" disabled={busy} onclick={() => void workspace.discoverLocal()}>{$t("calculation.scanLocal")}</button>
          </section>

          {#if workspace.installMessage}
            <p class:problem={failed} class:hint={!failed} role={failed ? "alert" : "status"}>{renderMessage(workspace.installMessage)}</p>
          {/if}

          <div class="actions">
            <button type="button" class="control" onclick={dismiss}>{$t("calculation.cancel")}</button>
            {#if downloaded}
              <button type="button" class="primary" disabled={busy} onclick={() => { workspace.installOpen = false; }}>{$t("calculation.useDownload")}</button>
            {:else}
              <button type="button" class="primary" disabled={busy || !allowed} onclick={() => void workspace.install()}>
                {downloadFailed ? $t("calculation.retryDownload") : $t("calculation.downloadSize", { size })}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {/snippet}
  </Dialog>
{/if}

<style>
  .body, section { display: grid; grid-template-columns: minmax(0, 1fr); }
  .body { gap: 18px; }
  .row { display: flex; gap: 8px; align-items: center; }
  .spacer { flex: 1; }

  section { gap: 8px; }
  section + section { padding-top: 18px; border-top: 1px solid var(--edge-soft, rgb(255 255 255 / 7%)); }
  h3 { display: flex; align-items: baseline; gap: 8px; margin: 0; font-size: var(--text-md, 13px); font-weight: 600; }
  .tag { color: var(--blueprint-light, #7fb0f7); font-size: var(--text-sm, 11px); font-weight: 500; }

  code { font-family: 'DejaVu Sans Mono', Consolas, monospace; font-size: var(--text-sm, 11px); }

  .candidates { display: grid; grid-template-columns: minmax(0, 1fr); gap: 2px; margin: 2px 0 0; padding: 0; list-style: none; }
  .candidates li { display: flex; align-items: center; gap: 12px; padding: 6px 0; }
  .candidates code { flex: 1; min-width: 0; overflow: hidden; color: var(--text, #e9ebee); direction: rtl; text-align: left; text-overflow: ellipsis; white-space: nowrap; }

  .figure { color: var(--muted, #aeb5be); font-variant-numeric: tabular-nums; }

  .link { justify-self: start; padding: 2px 0; border: 0; background: none; color: var(--blueprint-light, #7fb0f7); font: inherit; font-size: var(--text-sm, 11px); cursor: pointer; }
  .link:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 2px; }
</style>
