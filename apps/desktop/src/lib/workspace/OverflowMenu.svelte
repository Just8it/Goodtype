<script lang="ts">
  import { dismissable } from "./dismiss";
  import type { MenuAction, MenuEntry, MenuSection } from "./menu";

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
  }: {
    /** The subject the menu acts on, e.g. "Page 3 of 12". */
    title: string;
    /** Optional second line — a path, a state. */
    subtitle?: string;
    sections: MenuSection[];
    onClose: () => void;
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
    const items = menu
      ? [...menu.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)")]
      : [];
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

  function commitNumber(entry: MenuEntry, raw: string) {
    if (entry.kind !== "number") return;
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    entry.onCommit(Math.min(entry.max, Math.max(entry.min, parsed)));
    onClose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  bind:this={menu}
  use:dismissable={onClose}
  class="overflow-menu"
  aria-label={title}
  onkeydown={moveWithin}
>
  <div class="menu-subject">
    <strong>{title}</strong>
    {#if subtitle}<span title={subtitle}>{subtitle}</span>{/if}
  </div>

  {#each sections as section, index (section.title ?? index)}
    {#if index > 0}<div class="menu-divider"></div>{/if}
    {#if section.title}<div class="menu-heading">{section.title}</div>{/if}

    {#each section.entries as entry (entry.id)}
      {#if entry.kind === "number"}
        <div class="menu-row" class:disabled={entry.disabled}>
          <span class="menu-icon" aria-hidden="true">
            {#if entry.icon}
              <svg viewBox="0 0 24 24"><path d={entry.icon} /></svg>
            {/if}
          </span>
          <label for={`menu-${entry.id}`}>{entry.label}</label>
          <input
            id={`menu-${entry.id}`}
            type="number"
            min={entry.min}
            max={entry.max}
            value={entry.value}
            disabled={entry.disabled}
            onkeydown={(event) => {
              if (event.key === "Enter") commitNumber(entry, event.currentTarget.value);
            }}
            onblur={(event) => commitNumber(entry, event.currentTarget.value)}
          />
          {#if entry.hint}<span class="menu-hint">{entry.hint}</span>{/if}
        </div>
      {:else}
        <button
          type="button"
          class:destructive={entry.destructive}
          disabled={entry.disabled}
          onclick={() => run(entry)}
        >
          <span class="menu-icon" aria-hidden="true">
            {#if entry.icon}
              <svg viewBox="0 0 24 24"><path d={entry.icon} /></svg>
            {/if}
          </span>
          <span class="menu-label">{entry.label}</span>
          {#if entry.hint}
            <span class="menu-hint">{entry.hint}</span>
          {/if}
        </button>
      {/if}
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
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    animation: menu-in 130ms cubic-bezier(0.2, 0.7, 0.3, 1);
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
    /* A path is read from its end — the folder it is in matters more than the drive it is on. */
    direction: rtl;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-heading {
    padding: 9px 10px 5px;
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  /* One row shape for actions and for the number entry, so a leading icon never shifts the
     column of labels next to it. */
  button,
  .menu-row {
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

  button { cursor: pointer; transition: background 110ms ease; }
  button:hover:not(:disabled) { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: -2px; }
  button:disabled, .menu-row.disabled { color: var(--quiet); cursor: default; }

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

  .menu-row label { flex: 1; color: inherit; }

  .menu-row input {
    box-sizing: border-box;
    width: 62px;
    height: var(--control-dense);
    padding: 0 8px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: var(--surround);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .menu-row input:focus-visible { border-color: var(--blueprint); outline: none; }

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
