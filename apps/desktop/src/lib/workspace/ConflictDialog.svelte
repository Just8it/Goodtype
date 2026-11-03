<script lang="ts">
  import { t } from "../i18n";
  import Dialog from "./Dialog.svelte";
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
  session read-only until the user decides. There is deliberately no "overwrite anyway", and no
  Escape or backdrop dismissal: the writer has to choose.
-->
<Dialog title={$t("workspace.notebookChangedOutsideGoodtype")} lead={detail} size="md" role="alertdialog">
  {#snippet children()}
    <p class="hint">{$t("workspace.yourLatestUnsavedChangeWasNotWritten")}</p>
  {/snippet}
  {#snippet actions()}
    <button type="button" class="control" onclick={onCancel}>{$t("library.cancel")}</button>
    <button type="button" class="primary" onclick={onReload}>{$t("workspace.reloadNotebook")}</button>
  {/snippet}
</Dialog>
