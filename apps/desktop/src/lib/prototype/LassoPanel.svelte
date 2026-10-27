<script lang="ts">
  import { t } from "../i18n";
  import { lassoOptions, type LassoTargets } from "./lasso";
  let { targets, onChange }: { targets: LassoTargets; onChange: (next: LassoTargets) => void } = $props();
</script>

<div class="lasso-settings">
  {#each lassoOptions($t) as option (option.key)}
    <label class="setting">
      <span>{option.label}</span>
      <input type="checkbox" role="switch" aria-label={$t("workspace.select", { "value1": option.label.toLowerCase() })}
        checked={targets[option.key]} onchange={event => onChange({ ...targets, [option.key]: event.currentTarget.checked })} />
      <span class="toggle" aria-hidden="true"></span>
    </label>
  {/each}
</div>

<style>
  .lasso-settings { padding: 4px 12px 8px; }
  .setting { position: relative; display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 40px; font-size: var(--text-md); cursor: pointer; }
  input { position: absolute; right: 0; width: 40px; height: 40px; opacity: 0; margin: 0; cursor: pointer; }
  .toggle { flex: none; width: 32px; height: 18px; padding: 2px; border-radius: 10px; background: #505764; pointer-events: none; }
  .toggle::after { content: ""; display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--text); }
  input:checked + .toggle { background: var(--blueprint); }
  input:checked + .toggle::after { transform: translateX(14px); }
  input:focus-visible + .toggle { outline: 2px solid var(--blueprint); outline-offset: 3px; }
  @media (pointer: coarse) { .setting { min-height: 44px; } input { width: 44px; height: 44px; } }
</style>
