<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import { focusTrap } from "./focus";

  /**
   * The one modal frame. Every dialog used to copy its own backdrop, panel, title and buttons, and
   * each copy drifted (12px or 10px corners, bordered or borderless buttons, 15px or 20px titles),
   * so new dialogs never looked like the old ones. A dialog now supplies only its content.
   *
   * Shared classes for content and action rows, styled here so every caller matches:
   * `.control` (secondary button), `.primary`, `.danger`, `.actions` (button row, right-aligned),
   * `.spacer`, `.hint`, `.problem`, `.status`, `.facts` (a `<dl>`), `.progress` (+ `.indeterminate`).
   */
  let {
    title,
    lead,
    heading,
    size = "md",
    role = "dialog",
    onClose,
    dismissible = true,
    closeButton = false,
    closeTitle,
    labelledby,
    describedby,
    children,
    actions,
  }: {
    title: string;
    /** One muted line under the title: what this dialog is for. */
    lead?: string;
    /** Replaces the title text (the import panel's source tabs). `title` still names the dialog. */
    heading?: Snippet;
    /** sm 400px: a question or a name. md 480px: a decision with detail. lg 620px: a form. */
    size?: "sm" | "md" | "lg";
    role?: "dialog" | "alertdialog";
    /** Escape, a click on the backdrop and the close button call this. Omit for a dialog the
     *  writer must answer (a conflict). */
    onClose?: () => void;
    /** False keeps Escape and the backdrop inert while still allowing the close button. */
    dismissible?: boolean;
    closeButton?: boolean;
    closeTitle?: string;
    labelledby?: string;
    describedby?: string;
    children?: Snippet;
    actions?: Snippet;
  } = $props();

  const id = `dialog-${Math.random().toString(36).slice(2, 9)}`;
  let panel = $state<HTMLElement>();

  // Keyboard focus belongs inside an open modal, or Escape and Tab go nowhere. A dialog that
  // focuses its own field or a safe default button (Cancel) has done so by the next frame.
  $effect(() => {
    const node = panel;
    if (!node) return;
    const frame = requestAnimationFrame(() => {
      if (!node.contains(document.activeElement)) node.focus();
    });
    return () => cancelAnimationFrame(frame);
  });
  const canDismiss = $derived(Boolean(onClose) && dismissible);

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape" || !canDismiss) return;
    event.preventDefault();
    event.stopPropagation();
    onClose?.();
  }
</script>

<div class="scrim" role="presentation" onpointerdown={event => { if (event.target === event.currentTarget && canDismiss) onClose?.(); }}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={panel}
    use:focusTrap
    class="panel {size}"
    {role}
    aria-modal="true"
    aria-label={heading ? title : undefined}
    aria-labelledby={heading ? undefined : (labelledby ?? `${id}-title`)}
    aria-describedby={describedby ?? (lead ? `${id}-lead` : undefined)}
    tabindex="-1"
    onkeydown={keydown}
  >
    <header class:bare={!children}>
      <div class="titles">
        {#if heading}{@render heading()}{:else}<h2 id={`${id}-title`}>{title}</h2>{/if}
        {#if lead}<p id={`${id}-lead`} class="lead">{lead}</p>{/if}
      </div>
      {#if closeButton && onClose}
        <button type="button" class="close" aria-label={$t("library.close")} title={closeTitle ?? $t("library.close")} onclick={onClose}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
        </button>
      {/if}
    </header>
    {#if children}<div class="body">{@render children()}</div>{/if}
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </div>
</div>

<style>
  .scrim { position: fixed; z-index: 80; inset: 0; display: grid; padding: 20px; background: rgb(10 12 16 / 62%); place-items: center; animation: scrim-in var(--enter) ease-out; }
  .panel { box-sizing: border-box; display: flex; flex-direction: column; width: 100%; max-height: 100%; padding: 22px; overflow-y: auto; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); box-shadow: var(--shadow-modal); color: var(--text); font: var(--text-md) var(--font-ui); animation: panel-in var(--enter) cubic-bezier(0.16, 1, 0.3, 1); }
  .panel:focus { outline: none; }
  .sm { max-width: 400px; }
  .md { max-width: 480px; }
  .lg { max-width: 620px; }

  header { display: flex; gap: 12px; align-items: flex-start; margin-bottom: 18px; }
  header.bare { margin-bottom: 0; }
  .titles { flex: 1; min-width: 0; }
  h2 { margin: 0; font-size: 20px; font-weight: 600; line-height: 1.25; }
  .sm h2 { font-size: var(--text-lg); }
  .titles > :global(.lead) { margin-top: 5px; }
  /* minmax(0, 1fr): a long path or name must truncate or wrap, never widen the dialog. */
  .body { display: grid; grid-template-columns: minmax(0, 1fr); gap: 14px; min-height: 0; }

  .close { display: inline-grid; flex: none; width: var(--control-dense); height: var(--control-dense); place-items: center; padding: 0; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; }
  .close:hover { background: var(--wash); color: var(--text); }
  .close svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-width: 2; }

  /* Shared content vocabulary. Global inside the panel, because callers render their own markup. */
  .panel :global(p) { margin: 0; line-height: 1.5; }
  .panel :global(.lead) { color: var(--muted); }
  .panel :global(.status) { font-weight: 600; }
  .panel :global(.hint) { color: var(--muted); font-size: var(--text-sm); line-height: 1.45; }
  .panel :global(.problem) { color: var(--oxide-text); overflow-wrap: anywhere; }
  .panel :global(.facts) { display: grid; grid-template-columns: max-content 1fr; gap: 7px 18px; margin: 0; line-height: 1.45; }
  .panel :global(.facts dt) { color: var(--muted); }
  .panel :global(.facts dd) { margin: 0; }
  .panel :global(.spacer) { flex: 1; }
  .actions, .panel :global(.actions) { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; justify-content: flex-end; }
  .actions { margin-top: 20px; }

  .panel :global(:is(.control, .primary, .danger)) { display: inline-flex; min-height: var(--control); align-items: center; justify-content: center; gap: 7px; padding: 0 14px; border: 0; border-radius: var(--radius); font: inherit; font-size: var(--text-md); white-space: nowrap; cursor: pointer; touch-action: manipulation; }
  .panel :global(.control) { background: var(--wash); color: var(--text); }
  .panel :global(.control:hover:not(:disabled)) { background: var(--wash-strong); }
  .panel :global(.primary) { background: var(--blueprint); color: var(--on-accent); font-weight: 600; }
  .panel :global(.primary:hover:not(:disabled)) { background: var(--blueprint-hover); }
  .panel :global(.danger) { background: var(--oxide); color: var(--on-accent); font-weight: 600; }
  .panel :global(.danger:hover:not(:disabled)) { background: var(--oxide-hover); }
  .panel :global(button:disabled) { opacity: var(--disabled); cursor: default; }
  .panel :global(:is(button, summary, a):focus-visible) { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }

  .panel :global(.progress) { position: relative; height: 6px; overflow: hidden; border-radius: 3px; background: var(--wash); }
  /* Scaled, not resized: a transform never makes the page lay out again while the writer works. */
  .panel :global(.progress span) { position: absolute; inset: 0; border-radius: inherit; background: var(--blueprint); transform: scaleX(0); transform-origin: left; transition: transform 200ms ease-out; }
  .panel :global(.progress.indeterminate span) { right: auto; width: 30%; transform: none; animation: travel 1.3s ease-in-out infinite; }

  @keyframes scrim-in { from { opacity: 0; } }
  @keyframes panel-in { from { opacity: 0; transform: translateY(7px) scale(0.99); } }
  @keyframes travel { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  @media (max-width: 520px) { .panel { padding: 18px; } }
  @media (pointer: coarse) { .panel :global(:is(.control, .primary, .danger)) { min-height: var(--control-touch); } }
  @media (prefers-reduced-motion: reduce) {
    .scrim, .panel, .panel :global(.progress.indeterminate span) { animation: none; }
    .panel :global(.progress span) { transition: none; }
  }
</style>
