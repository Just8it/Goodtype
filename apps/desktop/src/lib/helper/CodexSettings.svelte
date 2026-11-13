<script lang="ts">
  import { onMount } from "svelte";
  import { t, renderMessage, type UiText } from "../i18n";
  import type { AppSettings } from "../settings";
  import { helperCodexStatus } from "../ipc/helper";
  import { codexStatus } from "./state";
  import { codexProblem, withMcpServer } from "./codex";
  let { settings, onChange }: { settings: AppSettings; onChange: (settings: AppSettings) => void } = $props();
  let loading = $state(false), error = $state<UiText>("");
  function update(fields: Partial<AppSettings["aiHelper"]>) {
    onChange({ ...settings, aiHelper: { ...settings.aiHelper, ...fields } });
  }
  async function refresh() {
    loading = true; error = "";
    try { codexStatus.set(await helperCodexStatus(true)); }
    catch (problem) { error = codexProblem(problem); }
    finally { loading = false; }
  }
  onMount(() => { if ("__TAURI_INTERNALS__" in window) void refresh(); });
</script>

<section aria-label={$t("codex.title")}>
  <header>
    <h3>{$t("codex.title")}</h3>
    <button type="button" disabled={loading} onclick={refresh}>{$t(loading ? "codex.checking" : "codex.refresh")}</button>
  </header>
  <p class="hint">{$t("codex.experimentalHint")}</p>
  <div class="connection">
    <p>{$t("codex.account", { account: $t(`codex.account_${$codexStatus?.account ?? "unknown"}`) })}</p>
    <p class="hint">{$t("codex.version", { version: $codexStatus?.version ?? $t("codex.unknown") })}</p>
  </div>
  {#if $codexStatus?.problem}<p class="problem" role="status">{renderMessage(codexProblem($codexStatus.problem), $t)}</p>{/if}
  {#if error}<p class="problem" role="status">{renderMessage(error, $t)}</p>{/if}
  <label>
    {$t("codex.model")}
    <span class="select-shell"><select value={settings.aiHelper.codexModel ?? ""} onchange={event => update({ codexModel: event.currentTarget.value || null })}>
      <option value="">{$t("codex.defaultModel")}</option>
      {#each $codexStatus?.models ?? [] as model (model.id)}
        <option value={model.id}>{model.label}</option>
      {/each}
    </select><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg></span>
  </label>
  <details>
    <summary>{$t("codex.advanced")}</summary>
  <label>
    {$t("codex.path")}
    <input value={settings.aiHelper.codexPath ?? ""} placeholder={$t("codex.automatic")}
      onchange={event => update({ codexPath: event.currentTarget.value.trim() || null })} />
  </label>
  <p class="path hint">{$codexStatus?.path ?? $t("codex.unknown")}</p>
  <h4>{$t("codex.servers")}</h4>
  <p class="hint">{$t("codex.warning")}</p>
  {#each $codexStatus?.mcpServers ?? [] as server (server.name)}
    <label class="server">
      <span>{server.name}</span>
      <span class="toggle">
      <input type="checkbox" role="switch" checked={settings.aiHelper.allowedMcpServers.includes(server.name)}
        onchange={event => update({ allowedMcpServers: withMcpServer(settings.aiHelper.allowedMcpServers, server.name, event.currentTarget.checked) })} />
      <span class="track"><span></span></span>
      </span>
    </label>
  {:else}
    <p class="hint">{$t("codex.noServers")}</p>
  {/each}
  </details>
</section>

<style>
  details { display: grid; gap: 12px; }
  details > label { margin: 12px 0; }
  summary { cursor: pointer; color: var(--muted); padding: 8px 0; }
  section { display: flex; flex-direction: column; gap: 12px; padding: 18px 0; border-top: 1px solid var(--edge-soft); font-size: var(--text-md); overflow-wrap: anywhere; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h3, h4, p { margin: 0; }
  h3 { font-size: var(--text-lg); font-weight: 600; }
  h4 { margin-top: 8px; font-size: var(--text-md); font-weight: 600; }
  .hint { color: var(--muted); font-size: var(--text-sm); line-height: 1.5; }
  .connection { display: flex; flex-direction: column; gap: 4px; }
  .problem { color: var(--oxide-text); font-size: var(--text-sm); line-height: 1.5; }
  label { display: grid; min-width: 0; gap: 8px; }
  .path { margin-top: -6px; user-select: text; }
  input:not([type="checkbox"]), select, button { min-width: 0; min-height: var(--control); max-width: 100%; padding: 8px 10px; background: var(--wash); color: var(--text); border: 0; border-radius: var(--radius); font: inherit; }
  input:not([type="checkbox"]), select { border: 1px solid var(--edge); background: var(--inset); }
  input:not([type="checkbox"]):hover, select:hover { border-color: var(--edge-strong); }
  input:not([type="checkbox"]):focus, select:focus { border-color: var(--blueprint); }
  input:not([type="checkbox"]), select { width: 100%; }
  input::placeholder { color: var(--quiet); }
  button { cursor: pointer; font-size: var(--text-sm); font-weight: 600; }
  button:hover:not(:disabled) { background: var(--wash-strong); }
  button:disabled { cursor: not-allowed; opacity: var(--disabled); }
  .select-shell { position: relative; display: flex; }
  select { padding-right: 32px; appearance: none; color-scheme: dark; cursor: pointer; }
  option { background: var(--panel); color: var(--text); }
  .select-shell svg { position: absolute; top: 50%; right: 8px; width: var(--icon-dense); height: var(--icon-dense); transform: translateY(-50%); fill: none; stroke: var(--muted); stroke-width: var(--stroke-dense); stroke-linecap: round; pointer-events: none; }
  .server { display: flex; align-items: center; justify-content: space-between; min-height: var(--control); border-top: 1px solid var(--edge-soft); gap: 20px; }
  .toggle { position: relative; display: grid; flex: none; width: 34px; height: var(--control); align-items: center; }
  .toggle input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer; }
  .track { height: 20px; border-radius: var(--radius-pill); background: var(--track); pointer-events: none; transition: background var(--fast); }
  .track > span { display: block; width: 14px; height: 14px; margin: 3px; border-radius: 50%; background: var(--muted); transition: transform var(--fast); }
  input:checked + .track { background: var(--blueprint); }
  input:checked + .track > span { background: var(--text); transform: translateX(14px); }
  :focus-visible, input:focus-visible + .track { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .toggle input:focus-visible { outline: none; }
  @media (prefers-reduced-motion: reduce) { .track, .track > span { transition: none; } }
  @media (pointer: coarse) { button, select, .server, .toggle { min-height: var(--control-touch); } }
</style>
