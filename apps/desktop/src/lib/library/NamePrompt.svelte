<script lang="ts">
  import { untrack } from "svelte";
  import { focusTrap } from "../workspace/focus";
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
  <div use:focusTrap class="prompt" role="dialog" aria-modal="true" aria-labelledby="name-prompt-title" tabindex="-1">
    <form onsubmit={submit}>
      <h2 id="name-prompt-title">{heading}</h2>
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
      <p id="name-prompt-problem" class="problem" class:shown={problem !== null}>{problem ?? " "}</p>
      <div class="actions">
        <button type="button" class="quiet" disabled={busy} onclick={onCancel}>Abbrechen</button>
        <button type="submit" class="primary" disabled={!ready}>{confirmLabel}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .scrim {
    position: absolute;
    z-index: 60;
    display: grid;
    padding: 20px;
    background: rgb(10 12 16 / 62%);
    inset: 0;
    place-items: center;
    animation: scrim-in 140ms ease-out;
  }

  .prompt {
    width: min(400px, 100%);
    padding: 18px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius-lg, 10px);
    background: var(--panel, #23272f);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    animation: prompt-in 160ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  h2 {
    margin: 0 0 12px;
    color: var(--text, #e9ebee);
    font-size: var(--text-lg, 15px);
    font-weight: 600;
  }

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

  input:focus-visible {
    border-color: var(--blueprint-light, #7fb0f7);
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  input:hover {
    border-color: rgb(255 255 255 / 20%);
  }

  input[aria-invalid="true"] {
    border-color: var(--oxide, #e5645e);
  }

  .problem {
    min-height: 1.2em;
    margin: 7px 2px 14px;
    color: transparent;
    font-size: var(--text-sm, 11px);
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
    min-height: var(--control, 36px);
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius, 6px);
    font: inherit;
    font-size: var(--text-md, 13px);
    cursor: pointer;
    touch-action: manipulation;
  }

  .primary {
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font-weight: 600;
  }

  .quiet {
    background: rgb(255 255 255 / 5%);
    color: var(--text, #e9ebee);
  }

  button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  button:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  .quiet:hover:not(:disabled) {
    background: var(--wash, rgb(255 255 255 / 8%));
  }

  .primary:hover:not(:disabled) {
    background: var(--blueprint-light, #7fb0f7);
  }

  @keyframes scrim-in {
    from { opacity: 0; }
  }

  @keyframes prompt-in {
    from { opacity: 0; transform: translateY(7px) scale(0.99); }
  }

  @media (pointer: coarse) {
    input,
    .primary,
    .quiet {
      min-height: var(--control-touch, 44px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .prompt {
      animation: none;
    }
  }
</style>
