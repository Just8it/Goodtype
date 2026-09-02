<script lang="ts">
  import { tick } from "svelte";
  import type { AppSettings } from "../settings";
  import {
    SETTING_GROUPS,
    searchSettings,
    type SettingActions,
    type SettingGroup,
    type SettingItem,
  } from "./settingsSchema";

  /**
   * App-level preferences: a list of categories and the settings inside the one you picked.
   *
   * It used to be a single scrolling column of hand-written sections, which worked at five and
   * would not have worked at fifteen — the column simply got longer, with no way to reach a
   * setting except to scroll past every setting before it. Now the categories are a list, and
   * a search field reaches anything by name.
   *
   * What is in here is what belongs to no single tool. Nib, width and colour are on the palette,
   * one tap from the page; putting them behind a window would be a step backwards from that.
   *
   * The settings themselves are described in `settingsSchema.ts`. This file knows how to draw a
   * toggle, a choice and a slider, and nothing about what any particular preference means.
   */
  let {
    settings,
    actions,
    onChange,
    onClose,
  }: {
    settings: AppSettings;
    /** What the entries that *do* something rather than store something should call. */
    actions: SettingActions;
    onChange: (settings: AppSettings) => void;
    onClose: () => void;
  } = $props();

  let groupId = $state(SETTING_GROUPS[0].id);
  let query = $state("");
  let panel = $state<HTMLElement>();
  let field = $state<HTMLInputElement>();

  const group = $derived<SettingGroup>(
    SETTING_GROUPS.find((entry) => entry.id === groupId) ?? SETTING_GROUPS[0],
  );
  const hits = $derived(searchSettings(query));
  const searching = $derived(query.trim().length > 0);

  function apply(item: SettingItem, value: boolean | string | number) {
    const control = item.control;
    if (control.kind === "toggle") onChange(control.write(settings, value as boolean));
    else if (control.kind === "choice") onChange(control.write(settings, value as string));
    else if (control.kind === "slider") onChange(control.write(settings, value as number));
  }

  /// Arrow keys walk the category list, the way they walk every other group in this app.
  function moveWithin(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const list = (event.currentTarget as HTMLElement).closest<HTMLElement>("[role='tablist']");
    const items = list ? [...list.querySelectorAll<HTMLButtonElement>("button")] : [];
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    if (current < 0) return;
    event.preventDefault();
    const next = items[(current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length];
    next?.focus();
    next?.click();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      // Escape clears a search before it closes the window: the first press should undo the
      // thing that changed the view, not the thing that opened it.
      if (searching) {
        query = "";
        void tick().then(() => field?.focus());
      } else onClose();
      return;
    }
    if (event.key !== "Tab" || !panel) return;
    const stops = [...panel.querySelectorAll<HTMLElement>("button, input, select")].filter(
      (stop) => !(stop as HTMLButtonElement).disabled && stop.tabIndex >= 0,
    );
    if (stops.length < 2) return;
    const first = stops[0];
    const last = stops[stops.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  /// Opens on the search field: with enough preferences, naming one is faster than finding it.
  $effect(() => {
    field?.focus();
  });
</script>

{#snippet control(item: SettingItem)}
  {#if item.control.kind === "toggle"}
    {@const on = item.control.read(settings)}
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={item.label}
      class:on
      class="toggle"
      onclick={() => apply(item, !on)}
    ><span></span></button>
  {:else if item.control.kind === "action"}
    {@const control = item.control}
    <button type="button" class="run" onclick={() => control.run(actions)}>{control.buttonLabel}</button>
  {:else if item.control.kind === "choice"}
    {@const current = item.control.read(settings)}
    <div class="choices" role="radiogroup" aria-label={item.label}>
      {#each item.control.options as option (option.value)}
        <button
          type="button"
          role="radio"
          class="choice"
          class:on={option.value === current}
          aria-checked={option.value === current}
          title={option.hint}
          onclick={() => apply(item, option.value)}
        >{option.label}</button>
      {/each}
    </div>
  {:else}
    {@const value = item.control.read(settings)}
    <div class="slider">
      <input
        type="range"
        min={item.control.min}
        max={item.control.max}
        step={item.control.step}
        {value}
        aria-label={item.label}
        oninput={(event) => apply(item, Number(event.currentTarget.value))}
      />
      <output>{item.control.format(value)}</output>
    </div>
  {/if}
{/snippet}

{#snippet setting(item: SettingItem, where?: string)}
  <div class="setting" class:stacked={item.control.kind !== "toggle"}>
    <div class="words">
      <span class="name">{item.label}</span>
      {#if where}<span class="where">{where}</span>{/if}
      {#if item.hint}<span class="hint">{item.hint}</span>{/if}
    </div>
    {@render control(item)}
  </div>
{/snippet}

<div class="panel-scrim" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={panel}
    class="panel"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label="Settings"
    onkeydown={keydown}
  >
    <header>
      <div class="subject">
        <span class="eyebrow">Local preferences</span>
        <strong>Settings</strong>
      </div>

      <label class="search">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.6-3.6" /></svg>
        <input
          bind:this={field}
          bind:value={query}
          type="search"
          placeholder="Search settings"
          aria-label="Search settings"
          spellcheck="false"
        />
      </label>

      <button type="button" class="close" aria-label="Close settings" onclick={onClose}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </header>

    <div class="body">
      <!-- Categories rather than a longer column: reaching a preference should not mean
           scrolling past every preference that was added before it. -->
      <div class="nav" role="tablist" aria-label="Setting categories" aria-orientation="vertical">
        {#each SETTING_GROUPS as entry (entry.id)}
          <button
            type="button"
            role="tab"
            class:on={!searching && entry.id === groupId}
            aria-selected={!searching && entry.id === groupId}
            tabindex={entry.id === groupId ? 0 : -1}
            onkeydown={moveWithin}
            onclick={() => {
              query = "";
              groupId = entry.id;
            }}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={entry.icon} /></svg>
            {entry.title}
            <span class="tally">{entry.items.length}</span>
          </button>
        {/each}
      </div>

      <div class="pane">
        {#if searching}
          <div class="pane-head">
            <h2>{hits.length} {hits.length === 1 ? "result" : "results"}</h2>
          </div>
          {#if hits.length}
            <div class="settings">
              {#each hits as hit (hit.item.id)}
                {@render setting(hit.item, hit.group.title)}
              {/each}
            </div>
          {:else}
            <p class="empty">
              Nothing matches &ldquo;{query}&rdquo;. Nib, width and colour are on the palette
              rather than in here.
            </p>
          {/if}
        {:else}
          <div class="pane-head">
            <h2>{group.title}</h2>
            {#if group.blurb}<p class="blurb">{group.blurb}</p>{/if}
          </div>
          <div class="settings">
            {#each group.items as item (item.id)}
              {@render setting(item)}
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .panel-scrim {
    position: fixed;
    z-index: 80;
    inset: 0;
    display: grid;
    background: rgb(10 12 15 / 55%);
    place-items: center;
    animation: scrim-in 140ms ease-out;
  }

  @keyframes scrim-in {
    from { opacity: 0; }
  }

  .panel {
    display: flex;
    width: min(760px, calc(100vw - 32px));
    height: min(520px, calc(100vh - 80px));
    flex-direction: column;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--surround);
    box-shadow: 0 30px 70px rgb(0 0 0 / 55%);
    color: var(--text);
    overflow: hidden;
    animation: panel-in 150ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }

  @keyframes panel-in {
    from { opacity: 0; transform: translateY(-8px) scale(0.985); }
  }

  header {
    display: flex;
    flex: none;
    align-items: center;
    padding: 12px 12px 12px 20px;
    border-bottom: 1px solid var(--edge-soft);
    background: var(--panel);
    gap: 14px;
  }

  .subject { display: flex; min-width: 0; flex-direction: column; }
  .subject strong { font-size: var(--text-lg); font-weight: 600; }

  .eyebrow {
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .search {
    display: flex;
    height: var(--control);
    flex: 1;
    align-items: center;
    max-width: 300px;
    margin-left: auto;
    padding: 0 10px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: var(--surround);
    color: var(--muted);
    gap: 8px;
    transition: border-color 120ms ease;
  }

  .search:focus-within { border-color: var(--blueprint); }

  .search svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .search input {
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    outline: none;
  }

  .search input::placeholder { color: var(--quiet); }

  .close {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }

  .close:hover { background: var(--wash); color: var(--text); }

  .close svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .body { display: flex; min-height: 0; flex: 1; }

  .nav {
    display: flex;
    width: 190px;
    flex: none;
    flex-direction: column;
    padding: 12px;
    border-right: 1px solid var(--edge-soft);
    gap: 2px;
    overflow-y: auto;
  }

  .nav button {
    display: flex;
    height: var(--control);
    align-items: center;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-md);
    text-align: left;
    cursor: pointer;
    gap: 10px;
    transition: background 120ms ease, color 120ms ease;
  }

  .nav button:hover { background: var(--wash); color: var(--text); }
  .nav button.on { background: rgb(76 141 240 / 16%); color: var(--text); }

  .nav svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense);
  }

  .tally {
    margin-left: auto;
    color: var(--quiet);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }

  .pane {
    min-width: 0;
    flex: 1;
    padding: 18px 22px 22px;
    overflow-y: auto;
  }

  .pane-head { margin-bottom: 14px; }
  .pane-head h2 { margin: 0; font-size: var(--text-lg); font-weight: 600; }

  .blurb {
    max-width: 46ch;
    margin: 6px 0 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .settings { display: flex; flex-direction: column; }

  .setting {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 14px 0;
    border-top: 1px solid var(--edge-soft);
    gap: 20px;
  }

  .setting:first-child { border-top: 0; padding-top: 4px; }

  /* A slider or a set of choices needs the width, so it drops under its own label rather than
     fighting the description for the same line. */
  .setting.stacked { flex-direction: column; gap: 10px; }

  .words { display: flex; min-width: 0; flex-direction: column; gap: 3px; }
  .name { font-size: var(--text-md); }

  .where {
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    order: -1;
  }

  .hint {
    max-width: 52ch;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .toggle {
    position: relative;
    width: 38px;
    height: 22px;
    flex: none;
    margin-top: 2px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-pill);
    background: rgb(255 255 255 / 14%);
    cursor: pointer;
    transition: background 140ms ease;
  }

  .toggle span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 140ms ease, background 140ms ease;
  }

  .toggle.on { background: var(--blueprint); }
  .toggle.on span { background: #fff; transform: translateX(16px); }

  /* An action is not a preference, so it is a button rather than a control with a state. */
  .run {
    height: var(--control);
    flex: none;
    padding: 0 14px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background 120ms ease;
  }

  .run:hover { background: var(--wash); }

  .choices {
    display: flex;
    align-self: stretch;
    gap: 6px;
  }

  .choice {
    height: var(--control);
    flex: 1;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }

  .choice:hover { background: var(--wash); color: var(--text); }

  .choice.on {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: var(--text);
  }

  .slider {
    display: flex;
    align-self: stretch;
    align-items: center;
    gap: 12px;
  }

  .slider input { flex: 1; accent-color: var(--blueprint); }

  .slider output {
    min-width: 46px;
    color: var(--text);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .empty {
    max-width: 46ch;
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.55;
  }

  button:focus-visible,
  .search:focus-within { outline: none; }

  .nav button:focus-visible,
  .close:focus-visible,
  .toggle:focus-visible,
  .choice:focus-visible,
  .run:focus-visible,
  .slider input:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: 1px;
  }

  @media (max-width: 720px) {
    .nav { width: 62px; padding: 12px 8px; }
    .nav button { justify-content: center; padding: 0; font-size: 0; gap: 0; }
    .nav .tally { display: none; }
  }

  @media (prefers-reduced-motion: reduce) {
    .panel-scrim,
    .panel { animation: none; }

    .toggle,
    .toggle span,
    .nav button,
    .choice,
    .search { transition: none; }
  }
</style>
