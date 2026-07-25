<script lang="ts">
  import type { RecoveryCandidate } from "../settings";

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
  applied or deleted silently (Pillar 3).
-->
<div class="scrim" role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="recovery-title">
    <h2 id="recovery-title">Recovered work found</h2>
    <p>
      An earlier save was interrupted. The confirmed notebook stayed intact, and the
      interrupted version was kept. Restore it as a new change, or discard it.
    </p>
    <ul>
      {#each candidates as candidate (candidate.fileName)}
        <li>
          <div class="summary">
            <strong>Page {candidate.pageId}</strong>
            <span>interrupted at revision {candidate.candidateRevision}; confirmed revision {candidate.confirmedRevision}</span>
          </div>
          <div class="actions">
            <button class="primary" type="button" disabled={busy} onclick={() => onRestore(candidate.fileName)}>
              Restore
            </button>
            <button type="button" disabled={busy} onclick={() => onDiscard(candidate.fileName)}>
              Discard
            </button>
          </div>
        </li>
      {/each}
    </ul>
    <div class="footer">
      <button type="button" onclick={onClose}>Decide later</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    background: rgb(10 12 15 / 62%);
  }

  .dialog {
    width: min(500px, calc(100% - 48px));
    max-height: min(70vh, 560px);
    display: flex;
    flex-direction: column;
    padding: 20px 22px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 12px;
    background: #23272f;
    color: #e9ebee;
    box-shadow: 0 18px 44px rgb(0 0 0 / 45%);
  }

  h2 {
    margin: 0 0 10px;
    font-size: 15px;
  }

  p {
    margin: 0 0 12px;
    color: #aeb5be;
    font-size: 12.5px;
    line-height: 1.55;
  }

  ul {
    flex: 1;
    overflow: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid rgb(255 255 255 / 8%);
    border-radius: 8px;
    margin-bottom: 8px;
  }

  .summary {
    display: grid;
    gap: 2px;
    font-size: 12.5px;
  }

  .summary span {
    color: #6a727c;
    font-size: 11.5px;
  }

  .actions,
  .footer {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .footer {
    margin-top: 10px;
  }

  button {
    padding: 6px 12px;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 8px;
    background: transparent;
    color: #e9ebee;
    font-size: 12px;
    cursor: pointer;
  }

  button:hover:enabled {
    background: rgb(255 255 255 / 6%);
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .primary {
    border-color: #4c8df0;
    background: #4c8df0;
    color: #0d1117;
    font-weight: 600;
  }

  .primary:hover:enabled {
    background: #7fb0f7;
  }
</style>
