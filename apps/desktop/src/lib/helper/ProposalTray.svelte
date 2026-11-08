<script lang="ts">
  import { t } from "../i18n";
  import type { Proposal } from "../ipc/helper";
  let { proposals, discarded, selected, onOpen, onClearNotice }: {
    proposals: Proposal[]; discarded: boolean;
    selected?: string | null;
    onOpen: (id: string) => void; onClearNotice: () => void;
  } = $props();
  const pending = $derived(proposals.filter(p => p.status === "pending" || p.status === "stale" || (p.kind === "preset_install" && ["installed", "already_installed"].includes(p.status))));
</script>

{#if (pending.length && (pending.length > 1 || !selected)) || discarded}
  <aside class="proposal-tray" aria-label={$t("proposal.proposals")} data-preserve-selection>
    <strong>{$t("proposal.proposals")}</strong>
    {#each pending as proposal (proposal.id)}
      <button class:current={selected === proposal.id} onclick={() => onOpen(proposal.id)}>{proposal.kind === "preset_install" ? $t("proposal.presetTray", { name: proposal.preset?.name ?? "", status: $t(proposal.status === "installed" ? "proposal.presetInstalled" : proposal.status === "already_installed" ? "proposal.presetAlreadyInstalled" : "proposal.review") }) : proposal.summary}</button>
    {/each}
    {#if discarded}
      <p role="status">{$t("proposal.discarded")}</p>
      <button onclick={onClearNotice}>{$t("helper.close")}</button>
    {/if}
  </aside>
{/if}

<style>
  .proposal-tray {
    display: grid;
    gap: 6px;
    margin-bottom: 14px;
    color: var(--text);
    font-size: var(--text-sm);
  }
  button {
    display: block;
    min-height: var(--control);
    max-width: 100%;
    padding: 8px 12px;
    overflow-wrap: anywhere;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: var(--wash);
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  button:hover { background: var(--wash-strong); }
  button.current { border-color: var(--accent-edge); background: var(--accent-fill); }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
</style>
