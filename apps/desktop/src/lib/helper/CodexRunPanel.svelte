<script lang="ts">
  import { t, renderMessage } from "../i18n";
  import type { CodexRunState, CodexToolEvent, CodexProblem } from "../ipc/helper";
  import { codexProblem } from "./codex";
  let { state, text, tools, problem = null, onStop, stopping = false }:
    { state: CodexRunState; text: string; tools: CodexToolEvent[]; problem?: CodexProblem | null;
      onStop: () => void; onClose?: () => void; stopping?: boolean } = $props();
  const active = $derived(!["done", "failed", "stopped"].includes(state));
  const steps = $derived(tools.filter((step, index) => !tools.slice(index + 1).some(next => next.server === step.server && next.tool === step.tool && next.phase !== "start" && step.phase === "start")));
</script>

<section aria-label={$t("codex.title")}>
  <div class="status-row"><p class:active role="status"><span class="dot"></span>{$t(`codex.${state}`)}</p>
    {#if active}<button type="button" disabled={stopping} onclick={onStop}>{$t("codex.stop")}</button>{/if}
  </div>
  {#if problem}<p role="alert">{renderMessage(codexProblem(problem), $t)}</p>{/if}
  {#if problem?.params.detail && ["codex_crashed", "confinement_failed", "codex_version_unsupported", "codex_not_found"].includes(problem.code)}
    <details><summary>{$t("codex.details")}</summary><pre>{problem.params.detail}</pre></details>
  {/if}
  {#if text}<div class="transcript">{text}</div>{/if}
  {#if tools.length}
  <details class="activity">
  <summary>{$t("codex.activity", { count: steps.length })}{#if tools.some(step => step.phase === "error")} · {$t("codex.toolIssue")}{/if}</summary>
  <ol>
    {#each steps as step}
      <li class:external={step.external}>
        {$t("codex.tool", { server: step.server, tool: step.tool, phase: $t(`codex.phase_${step.phase}`) })}
        {#if step.external}<strong>{$t("codex.external")}</strong>{/if}
      </li>
    {/each}
  </ol>
  </details>
  {/if}
</section>

<style>
  section { display: grid; gap: 12px; margin-bottom: 16px; }
  .status-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .status-row p { display: flex; align-items: center; gap: 7px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--muted); }
  .active .dot { background: var(--blueprint); }
  p { margin: 0; font-size: var(--text-sm); color: var(--muted); }
  [role="alert"] { color: var(--oxide-text); }
  details { color: var(--muted); font-size: var(--text-sm); }
  summary { cursor: pointer; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
  .transcript { white-space: pre-wrap; overflow-wrap: anywhere; font-weight: 400; line-height: 1.65; user-select: text; }
  ol { margin: 0; padding-left: 20px; color: var(--muted); font-size: var(--text-sm); }
  li { padding: 4px; }
  .external { background: var(--wash); border-radius: var(--radius); }
  strong { display: block; }
  button { min-height: var(--control-dense); padding: 4px 10px; background: var(--wash); color: var(--text); border: 0; border-radius: var(--radius); cursor: pointer; font-size: var(--text-sm); }
  button:hover:not(:disabled) { background: var(--wash-strong); }
  button:disabled { cursor: not-allowed; opacity: var(--disabled); }
  @media (pointer: coarse) { button { min-height: var(--control-touch); } }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
</style>
