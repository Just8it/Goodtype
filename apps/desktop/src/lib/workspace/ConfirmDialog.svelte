<script lang="ts">
  import { t } from "../i18n";
  import Dialog from "./Dialog.svelte";

  /**
   * Asks before a destructive step.
   *
   * In-app rather than `window.confirm`: inside Tauri the dialog plugin replaces that call with an
   * asynchronous native dialog the app has no permission for, and the promise it returns is truthy,
   * so the old check let the step run without asking at all.
   */
  let {
    title,
    detail,
    confirmLabel,
    onConfirm,
    onCancel,
  }: {
    title: string;
    detail: string;
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  let cancelButton = $state<HTMLButtonElement>();

  // Focus starts on Cancel, so an Enter pressed out of habit does not destroy anything.
  $effect(() => {
    cancelButton?.focus();
  });
</script>

<Dialog {title} lead={detail} size="sm" role="alertdialog" onClose={onCancel}>
  {#snippet actions()}
    <button bind:this={cancelButton} type="button" class="control" onclick={onCancel}>{$t("library.cancel")}</button>
    <button type="button" class="danger" onclick={onConfirm}>{confirmLabel}</button>
  {/snippet}
</Dialog>
