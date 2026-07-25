<script lang="ts">
  let {
    detail,
    onReload,
    onCancel,
  }: {
    detail: string;
    onReload: () => void;
    onCancel: () => void;
  } = $props();
</script>

<!--
  Shown when Rust refused a commit because canonical files changed outside this session.
  Reload discards unsaved in-memory changes and loads the on-disk state; Cancel keeps the
  session read-only until the user decides. There is deliberately no "overwrite anyway".
-->
<div class="scrim" role="presentation">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="conflict-title" aria-describedby="conflict-detail">
    <h2 id="conflict-title">Notebook changed outside Goodtype</h2>
    <p id="conflict-detail">{detail}</p>
    <p>
      Your latest unsaved change was not written. Reload to continue from the files on disk,
      or cancel to keep this view open without saving.
    </p>
    <div class="actions">
      <button class="primary" type="button" onclick={onReload}>Reload notebook</button>
      <button type="button" onclick={onCancel}>Cancel</button>
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
    width: min(440px, calc(100% - 48px));
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
    margin: 0 0 10px;
    color: #aeb5be;
    font-size: 12.5px;
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    margin-top: 14px;
  }

  button {
    padding: 7px 14px;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 8px;
    background: transparent;
    color: #e9ebee;
    font-size: 12.5px;
    cursor: pointer;
  }

  button:hover {
    background: rgb(255 255 255 / 6%);
  }

  .primary {
    border-color: #4c8df0;
    background: #4c8df0;
    color: #0d1117;
    font-weight: 600;
  }

  .primary:hover {
    background: #7fb0f7;
  }
</style>
