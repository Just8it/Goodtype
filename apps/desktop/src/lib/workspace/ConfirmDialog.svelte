<script lang="ts">
  import { t } from "../i18n";
  import { focusTrap } from "./focus";

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

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape") onCancel();
  }}
/>

<div
  class="scrim"
  role="presentation"
  onpointerdown={(event) => {
    if (event.target === event.currentTarget) onCancel();
  }}
>
  <div use:focusTrap class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-detail" tabindex="-1">
    <h2 id="confirm-title">{title}</h2>
    <p id="confirm-detail">{detail}</p>
    <div class="actions">
      <button bind:this={cancelButton} type="button" onclick={onCancel}>{$t("library.cancel")}</button>
      <button class="danger" type="button" onclick={onConfirm}>{confirmLabel}</button>
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
    width: min(400px, calc(100% - 48px));
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
    margin: 0;
    color: #aeb5be;
    font-size: 12.5px;
    line-height: 1.55;
  }

  .actions {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
    margin-top: 18px;
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

  .danger {
    border-color: transparent;
    background: var(--oxide, #e5645e);
    color: #1b0f0e;
    font-weight: 600;
  }

  .danger:hover {
    background: #ef7a74;
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: 2px;
  }
</style>
