<script lang="ts">
  import { dismissable } from "./dismiss";
  import type { MenuAction, MenuSection } from "./menu";

  /**
   * The overflow menu. Renders whatever `sections` describe, so a new page-level feature costs
   * one entry in the caller rather than markup here.
   *
   * Closing is the component's job, not the caller's — see `dismissable`.
   */
  let {
    title,
    subtitle,
    sections,
    onClose,
    trigger,
    compact = false,
  }: {
    /** The subject the menu acts on, e.g. "Page 3 of 12". */
    title: string;
    /** Optional second line — the notebook name, a state. */
    subtitle?: string;
    sections: MenuSection[];
    onClose: () => void;
    trigger?: HTMLElement;
    /** In-surface menu: the trigger already names the subject, and rows take the dense size. */
    compact?: boolean;
  } = $props();

  let menu = $state<HTMLElement>();

  function run(entry: MenuAction) {
    if (entry.disabled) return;
    entry.onSelect();
    onClose();
  }

  /// Arrow keys walk the whole menu, skipping the headings and anything disabled. Without this
  /// the only way through eleven entries was eleven presses of Tab.
  function moveWithin(event: KeyboardEvent) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const items = menu ? [...menu.querySelectorAll<HTMLElement>("button:not(:disabled)")] : [];
    const current = items.indexOf(document.activeElement as HTMLElement);
    if (current < 0 || !items.length) return;
    event.preventDefault();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  bind:this={menu}
  use:dismissable={{ onClose, trigger }}
  class="overflow-menu"
  class:compact
  aria-label={title}
  onkeydown={moveWithin}
>
  {#if !compact}
    <div class="menu-subject">
      <strong>{title}</strong>
      {#if subtitle}<span title={subtitle}>{subtitle}</span>{/if}
    </div>
  {/if}

  {#each sections as section, index (section.title ?? index)}
    {#if index > 0}<div class="menu-divider"></div>{/if}
    {#if section.title}<div class="menu-heading">{section.title}</div>{/if}

    {#each section.entries as entry (entry.id)}
      <button
        type="button"
        class:destructive={entry.destructive}
        disabled={entry.disabled}
        onclick={() => run(entry)}
      >
        {#if !compact || entry.icon}
          <span class="menu-icon" aria-hidden="true">
            {#if entry.icon}
              <svg viewBox="0 0 24 24"><path d={entry.icon} /></svg>
            {/if}
          </span>
        {/if}
        <span class="menu-label">{entry.label}</span>
        {#if entry.hint}
          <span class="menu-hint">{entry.hint}</span>
        {/if}
      </button>
    {/each}
  {/each}
</aside>

<style>
  .overflow-menu {
    position: absolute;
    z-index: 50;
    /* Offsets against the workspace, not against the button: this menu is rendered as a sibling
       of the command strip rather than inside it, so a percentage `top` would measure the whole
       app's height and drop the menu below the window. */
    top: 52px;
    right: 14px;
    width: min(304px, calc(100vw - 28px));
    padding: 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow-menu);
    animation: menu-in var(--enter) cubic-bezier(0.2, 0.7, 0.3, 1);
    transform-origin: top right;
  }

  @keyframes menu-in {
    from { opacity: 0; transform: translateY(-6px) scale(0.98); }
  }

  .menu-subject {
    padding: 8px 10px 10px;
    border-bottom: 1px solid var(--edge-soft);
    margin-bottom: 6px;
  }

  .menu-subject strong {
    display: block;
    color: var(--text);
    font-size: var(--text-md);
    font-weight: 600;
  }

  .menu-subject span {
    display: block;
    overflow: hidden;
    margin-top: 3px;
    color: var(--quiet);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-heading {
    padding: 9px 10px 5px;
    color: var(--muted);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  button {
    display: flex;
    width: 100%;
    min-height: var(--control);
    align-items: center;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    text-align: left;
    gap: 10px;
  }

  button { cursor: pointer; transition: background var(--fast) ease; }
  button:hover:not(:disabled) { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: -2px; }
  button:disabled { color: var(--quiet); cursor: default; }

  .menu-icon {
    display: grid;
    width: var(--icon-dense);
    height: var(--icon-dense);
    flex: none;
    place-items: center;
  }

  .menu-icon svg {
    width: 100%;
    height: 100%;
    fill: none;
    stroke: var(--muted);
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense);
  }

  button:hover:not(:disabled) .menu-icon svg { stroke: var(--text); }
  .menu-label { overflow: hidden; flex: 1; text-overflow: ellipsis; white-space: nowrap; }

  .menu-hint {
    flex: none;
    color: var(--quiet);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  /* Red is spent only on entries that destroy work the writer cannot retype, so it still means
     something when it appears. */
  .destructive { color: var(--oxide); }
  .destructive .menu-icon svg { stroke: var(--oxide); }
  .destructive:hover:not(:disabled) { background: rgb(229 100 94 / 12%); }
  .destructive:hover:not(:disabled) .menu-icon svg { stroke: var(--oxide); }

  /* Sized to its labels, with dense rows: it sits on the work, not in the window chrome. */
  .overflow-menu.compact {
    width: max-content;
    min-width: 168px;
    max-width: min(280px, calc(100vw - 28px));
    padding: 4px;
    border-radius: var(--radius);
    box-shadow: var(--shadow-menu);
  }

  .compact button { min-height: var(--control-dense); padding: 0 8px; }
  .compact .menu-heading { padding: 6px 8px 3px; }
  .compact .menu-divider { margin: 4px 6px; }

  .menu-divider {
    height: 1px;
    margin: 6px 8px;
    background: var(--edge-soft);
  }

  @media (prefers-reduced-motion: reduce) {
    .overflow-menu { animation: none; }
    button { transition: none; }
  }
</style>
