<script lang="ts">
  import { untrack } from "svelte";
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

  const problem = $derived(value ? nameProblem(value) : null);
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

<!-- On the window rather than the form, so Escape closes the prompt wherever focus has gone. -->
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
  <form class="prompt" onsubmit={submit}>
    <h2>{heading}</h2>
    <input
      bind:this={field}
      bind:value
      type="text"
      spellcheck="false"
      autocomplete="off"
      aria-label={heading}
      aria-invalid={problem !== null}
      maxlength="80"
    />
    <!-- Reserves its line whether or not there is a problem, so the buttons never jump. -->
    <p class="problem" class:shown={problem !== null}>{problem ?? " "}</p>
    <div class="actions">
      <button type="button" class="quiet" disabled={busy} onclick={onCancel}>Abbrechen</button>
      <button type="submit" class="primary" disabled={!ready}>{confirmLabel}</button>
    </div>
  </form>
</div>

<style>
  .scrim {
    position: absolute;
    z-index: 60;
    display: grid;
    background: rgb(0 0 0 / 45%);
    inset: 0;
    place-items: center;
  }

  .prompt {
    width: min(400px, calc(100vw - 48px));
    padding: 18px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel, #23272f);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  h2 {
    margin: 0 0 12px;
    color: var(--text, #e9ebee);
    font-size: 15px;
    font-weight: 600;
  }

  input {
    box-sizing: border-box;
    width: 100%;
    padding: 9px 11px;
    border: 1px solid rgb(255 255 255 / 16%);
    border-radius: 7px;
    background: rgb(0 0 0 / 25%);
    color: var(--text, #e9ebee);
    font: inherit;
    font-size: 14px;
  }

  input:focus-visible {
    border-color: var(--blueprint, #4c8df0);
    outline: none;
  }

  input[aria-invalid="true"] {
    border-color: var(--oxide, #e5645e);
  }

  .problem {
    min-height: 1.2em;
    margin: 7px 2px 14px;
    color: transparent;
    font-size: 11.5px;
  }

  .problem.shown {
    color: var(--oxide, #e5645e);
  }

  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .primary,
  .quiet {
    height: 34px;
    padding: 0 14px;
    border: 0;
    border-radius: 7px;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .primary {
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font-weight: 600;
  }

  .quiet {
    background: rgb(255 255 255 / 6%);
    color: var(--text, #e9ebee);
  }

  button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: 2px;
  }
</style>
