<script lang="ts">
  import { helper } from "./state";
  import { helperInstallRelay } from "../ipc/helper";
  import { t, notice, renderMessage, type UiText } from "../i18n";
  let status = $state<UiText>("");
  let command = $derived($helper?.relayPath ? `codex mcp add goodtype -- "${$helper.relayPath}"` : "");
  async function copy() { try { await navigator.clipboard.writeText(command); status = notice("helper.copied"); } catch { status = notice("helper.error"); } }
  async function retry() { try { helper.set(await helperInstallRelay()); } catch { status = notice("helper.error"); } }
</script>
<details>
  <summary>{$t("helper.terminalSetup")}</summary>
  <section aria-label={$t("helper.setup")}>
  <p>{$helper?.listening ? $t("helper.listening", { count: $helper.sessions.length }) : $t("helper.off")}</p>
  {#if command}<code>{command}</code><button type="button" onclick={copy}>{$t("helper.copy")}</button>{/if}
  {#if $helper?.skillPath}<p>{$t("helper.skill", { path: $helper.skillPath })}</p>{/if}
  {#if $helper?.relayProblem}<p>{$t(`helper.${$helper.relayProblem.code}`)}</p><button type="button" onclick={retry}>{$t("helper.retry")}</button>{/if}
  {#if status}<p role="status">{renderMessage(status, $t)}</p>{/if}
</section>
</details>
<style>
  details { padding: 16px 0; border-top: 1px solid var(--edge-soft); font-size: var(--text-md); }
  summary { cursor: pointer; color: var(--muted); }
  section { display: flex; flex-direction: column; align-items: flex-start; gap: 10px; padding-top: 12px; overflow-wrap: anywhere; }
  p { margin: 0; color: var(--muted); font-size: var(--text-sm); line-height: 1.5; }
  code { display: block; width: 100%; padding: 10px; border-radius: var(--radius); background: var(--inset); font-size: var(--text-sm); line-height: 1.5; user-select: text; }
  button { min-height: var(--control); padding: 8px 12px; background: var(--wash); color: var(--text); border: 0; border-radius: var(--radius); cursor: pointer; font-size: var(--text-sm); font-weight: 600; }
  button:hover { background: var(--wash-strong); }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  @media (pointer: coarse) { button, summary { min-height: var(--control-touch); } }
</style>
