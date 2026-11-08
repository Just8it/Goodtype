<script lang="ts">
  import { t } from "../i18n";
  import type { RecoveryCandidate } from "../settings";
  import Dialog from "./Dialog.svelte";

  let {
    candidates,
    busy = false,
    onRestore,
    onDiscard,
    onClose,
  }: {
    candidates: RecoveryCandidate[];
    busy?: boolean;
    onRestore: (fileName: string) => void;
    onDiscard: (fileName: string) => void;
    onClose: () => void;
  } = $props();
</script>

<!--
  Interrupted saves leave their unconfirmed work under .goodtype/recovery. The user decides:
  restore lands the candidate as a new undoable revision; discard removes it. Nothing is
  applied or deleted silently (Pillar 3). "Decide later" is the only way out, so Escape and the
  backdrop do nothing here.
-->
<Dialog title={$t("workspace.recoveredWorkFound")} lead={$t("workspace.anEarlierSaveWasInterruptedTheConfirmed")} size="md" {onClose} dismissible={false}>
  {#snippet children()}
    <ul>
      {#each candidates as candidate (candidate.fileName)}
        <li>
          <div class="summary">
            {#if candidate.error}
              <strong>{$t("workspace.unreadableRecoveredWork")}</strong>
              <span>{candidate.fileName}</span>
              <span class="problem">{candidate.error}</span>
            {:else}
              <strong>{$t("workspace.page3", { "pageId": candidate.pageId ?? "—" })}</strong>
              <span>{$t("workspace.interruptedAtRevisionConfirmedRevision", { "candidateRevision": candidate.candidateRevision ?? "—", "confirmedRevision": candidate.confirmedRevision ?? "—" })}</span>
            {/if}
          </div>
          <div class="actions">
            <button class="control" type="button" disabled={busy} onclick={() => onDiscard(candidate.fileName)}>
              {$t("workspace.discard")}
            </button>
            <button class="primary" type="button" disabled={busy || !!candidate.error} onclick={() => onRestore(candidate.fileName)}>
              {$t("workspace.restore")}
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/snippet}
  {#snippet actions()}
    <button type="button" class="control" onclick={onClose}>{$t("workspace.decideLater")}</button>
  {/snippet}
</Dialog>

<style>
  ul { max-height: min(50vh, 400px); margin: 0; padding: 0; overflow: auto; list-style: none; }
  /* Rows divided by a soft rule rather than boxed: cards inside a panel read as clutter. */
  li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 0; border-top: 1px solid var(--edge-soft); }
  li:first-child { border-top: 0; padding-top: 0; }
  .summary { display: grid; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  .summary span { color: var(--muted); font-size: var(--text-sm); }
  .summary .problem { color: var(--oxide-text); }
  .actions { flex: none; }
</style>
