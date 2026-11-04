<script lang="ts">
  import { t } from "../i18n";
  import { untrack } from "svelte";
  import Dialog from "../workspace/Dialog.svelte";
  import { nameProblem } from "./library";

  /**
   * Ask for a name, and refuse a bad one before it reaches the disk.
   *
   * The check here is the same rule Rust enforces (`validate_name`), duplicated on purpose: this
   * copy makes the objection immediate and typed-into, that copy makes it true. The message
   * appears as you type rather than on submit, because a name is short enough that finding out
   * at the end is worse than being told along the way.
   */
  let {
    heading,
    confirmLabel,
    initial = "",
    busy = false,
    onConfirm,
    onCancel,
  }: {
    heading: string;
    confirmLabel: string;
    initial?: string;
    busy?: boolean;
    onConfirm: (name: string) => void;
    onCancel: () => void;
  } = $props();

  // Seeded once and then owned by the field. A fresh prompt is a fresh component — the caller
  // mounts it behind `{#if}` — so following `initial` afterwards would only fight the typist.
  let value = $state(untrack(() => initial));
  let field = $state<HTMLInputElement>();

  const problem = $derived(value ? nameProblem(value, $t) : null);
  const ready = $derived(value.length > 0 && problem === null && !busy);

  $effect(() => {
    field?.focus();
    field?.select();
  });

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (ready) onConfirm(value);
  }
</script>

<Dialog title={heading} size="sm" onClose={onCancel}>
  {#snippet children()}
    <form id="name-prompt-form" onsubmit={submit}>
      <input
        bind:this={field}
        bind:value
        type="text"
        spellcheck="false"
        autocomplete="off"
        aria-label={heading}
        aria-invalid={problem !== null}
        aria-describedby="name-prompt-problem"
        maxlength="80"
      />
      <!-- Reserves its line whether or not there is a problem, so the buttons never jump. -->
      <p id="name-prompt-problem" class="name-problem" class:shown={problem !== null}>{problem ?? " "}</p>
    </form>
  {/snippet}
  {#snippet actions()}
    <button type="button" class="control" disabled={busy} onclick={onCancel}>{$t("library.cancel")}</button>
    <button type="submit" form="name-prompt-form" class="primary" disabled={!ready}>{confirmLabel}</button>
  {/snippet}
</Dialog>

<style>
  input {
    box-sizing: border-box;
    width: 100%;
    min-height: var(--control, 36px);
    padding: 9px 11px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius, 6px);
    background: rgb(0 0 0 / 25%);
    color: var(--text, #e9ebee);
    font: inherit;
    font-size: var(--text-md, 13px);
  }
  input:hover { border-color: rgb(255 255 255 / 20%); }
  input:focus-visible { border-color: var(--blueprint-light, #7fb0f7); outline: 2px solid var(--blueprint-light, #7fb0f7); outline-offset: 1px; }
  input[aria-invalid="true"] { border-color: var(--oxide, #e5645e); }
  .name-problem { min-height: 1.2em; margin: 7px 2px 0; color: transparent; font-size: var(--text-sm, 11px); }
  .name-problem.shown { color: var(--oxide, #e5645e); }
  @media (pointer: coarse) { input { min-height: var(--control-touch, 44px); } }
</style>
