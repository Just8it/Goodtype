<script lang="ts">
  import type { MenuEntry, MenuSection } from "./menu";

  /**
   * The overflow menu. Renders whatever `sections` describe, so a new page-level feature costs
   * one entry in the caller rather than markup here.
   *
   * Closing is the component's job, not the caller's: a menu that outlives a press somewhere else
   * is the bug every popout in this app has had at least once.
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

  let panel = $state<HTMLElement>();

  // Pointer rather than click, so the menu is gone before the press lands on what is underneath.
  $effect(() => {
    const dismiss = (event: PointerEvent) => {
      if (panel && !panel.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    // Deferred a frame: the same press that opened the menu is still propagating.
    const timer = setTimeout(() => window.addEventListener("pointerdown", dismiss, true));
    window.addEventListener("keydown", key);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointerdown", dismiss, true);
      window.removeEventListener("keydown", key);
    };
  });

  function run(entry: MenuEntry) {
    if (entry.disabled) return;
    if (entry.kind === "action") {
      entry.onSelect();
      onClose();
    } else if (entry.kind === "toggle") {
      // Stays open: flipping one setting often precedes flipping the next.
      entry.onChange(!entry.value);
    }
  }

  function commitNumber(entry: MenuEntry, raw: string) {
    if (entry.kind !== "number") return;
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    entry.onCommit(Math.min(entry.max, Math.max(entry.min, parsed)));
    onClose();
  }
</script>

<aside bind:this={panel} class="overflow-menu" aria-label={title}>
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
          class:destructive={entry.kind === "action" && entry.destructive}
          disabled={entry.disabled}
          role={entry.kind === "toggle" ? "switch" : undefined}
          aria-checked={entry.kind === "toggle" ? entry.value : undefined}
          onclick={() => run(entry)}
        >
          {entry.label}
          {#if entry.kind === "toggle"}
            <span class="menu-switch" class:on={entry.value} aria-hidden="true"></span>
          {:else if entry.hint}
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
    top: 52px;
    right: 14px;
    width: min(300px, calc(100vw - 28px));
    padding: 7px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .menu-subject {
    padding: 8px 10px 10px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    margin-bottom: 5px;
  }

  .menu-subject strong {
    display: block;
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
  }

  .menu-subject span {
    display: block;
    overflow: hidden;
    margin-top: 4px;
    color: var(--muted);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-heading {
    padding: 7px 11px 5px;
    color: var(--quiet);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  button {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 10px 11px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  button:hover:enabled {
    background: rgb(255 255 255 / 6%);
  }

  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* Oxide, from the palette's own presets rather than a red invented for this menu. */
  .destructive {
    color: #e5645e;
  }

  .destructive:hover:enabled {
    background: rgb(229 100 94 / 12%);
  }

  .menu-hint {
    margin-left: auto;
    padding-left: 12px;
    color: var(--quiet);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .menu-divider {
    height: 1px;
    margin: 5px 8px;
    background: rgb(255 255 255 / 9%);
  }

  .menu-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 11px;
  }

  .menu-row.disabled {
    opacity: 0.45;
  }

  .menu-row label {
    color: var(--text);
    font-size: 13px;
  }

  .menu-row input {
    width: 62px;
    margin-left: auto;
    padding: 5px 8px;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 6px;
    background: rgb(0 0 0 / 25%);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .menu-row input:focus-visible {
    outline: 2px solid #38b6c6;
    outline-offset: 1px;
  }

  .menu-switch {
    width: 30px;
    height: 17px;
    margin-left: auto;
    border-radius: 9px;
    background: rgb(255 255 255 / 16%);
    transition: background 120ms ease;
  }

  .menu-switch::after {
    display: block;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    margin: 2px;
    background: var(--text);
    content: "";
    transition: transform 120ms ease;
  }

  .menu-switch.on {
    background: #38b6c6;
  }

  .menu-switch.on::after {
    transform: translateX(13px);
  }
</style>
