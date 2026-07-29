<script lang="ts">
  import type { AppSettings } from "../settings";

  let {
    settings,
    onChange,
    onClose,
  }: {
    settings: AppSettings;
    onChange: (settings: AppSettings) => void;
    onClose: () => void;
  } = $props();

  function updateCalibration(patch: Partial<AppSettings["calibration"]>) {
    onChange({ ...settings, calibration: { ...settings.calibration, ...patch } });
  }
</script>

<!-- App-level preferences. Per-tool quick settings (pen width/color, highlighter, eraser)
     live in the palette popouts; this window holds what belongs to no single tool:
     pressure calibration, undo scope, and motion. Values are clamped again in Rust. -->
<div class="panel-scrim" role="presentation">
  <aside class="panel" aria-label="Settings">
    <div class="heading">
      <div><span>Local preferences</span><h2>Settings</h2></div>
      <button class="close" type="button" aria-label="Close settings" onclick={onClose}>×</button>
    </div>

    <section aria-labelledby="settings-pressure">
      <p class="hint">Stroke sizes and colors are on the palette bar — one tap, no submenu. This window holds pressure, calibration, and app behavior.</p>
      <h3 id="settings-pressure">Pressure calibration</h3>
      <label class="choice">
        <input
          type="checkbox"
          checked={settings.pressureEnabled}
          onchange={(event) =>
            onChange({ ...settings, pressureEnabled: event.currentTarget.checked })}
        />
        <span><strong>Use stylus pressure</strong><em>Disable for uniform-width strokes across all pressure-sensitive pens</em></span>
      </label>
      <div class="row">
        <label>
          <span>Curve</span>
          <input
            type="range"
            min="0.25"
            max="3"
            step="0.05"
            value={settings.calibration.curve}
            aria-label="Pressure response curve"
            oninput={(event) =>
              updateCalibration({ curve: Number(event.currentTarget.value) })}
          />
          <output>{settings.calibration.curve.toFixed(2)}</output>
        </label>
      </div>
      <div class="row">
        <label>
          <span>Smoothing</span>
          <input
            type="range"
            min="0"
            max="0.8"
            step="0.05"
            value={settings.calibration.smoothing}
            aria-label="Stroke smoothing"
            oninput={(event) =>
              updateCalibration({ smoothing: Number(event.currentTarget.value) })}
          />
          <output>{settings.calibration.smoothing.toFixed(2)}</output>
        </label>
      </div>
    </section>

    <section aria-labelledby="settings-undo">
      <h3 id="settings-undo">Undo</h3>
      <div class="choice-row" role="radiogroup" aria-labelledby="settings-undo">
        <label class="choice">
          <input
            type="radio"
            name="undo-scope"
            value="page"
            checked={settings.undoScope === "page"}
            onchange={() => onChange({ ...settings, undoScope: "page" })}
          />
          <span><strong>Current page</strong><em>Undo affects the page in view</em></span>
        </label>
        <label class="choice">
          <input
            type="radio"
            name="undo-scope"
            value="notebook"
            checked={settings.undoScope === "notebook"}
            onchange={() => onChange({ ...settings, undoScope: "notebook" })}
          />
          <span><strong>Whole notebook</strong><em>Undo the most recent change anywhere</em></span>
        </label>
      </div>
    </section>

    <section aria-labelledby="settings-packages">
      <h3 id="settings-packages">Typst packages</h3>
      <label class="choice">
        <input
          type="checkbox"
          checked={settings.remotePackages}
          onchange={(event) =>
            onChange({ ...settings, remotePackages: event.currentTarget.checked })}
        />
        <span
          ><strong>Download packages from Typst Universe</strong><em
            >Fetches an imported package the first time you use it, then keeps it on this device.
            Packages you already have keep working offline either way.</em
          ></span
        >
      </label>
    </section>

    <section aria-labelledby="settings-motion">
      <h3 id="settings-motion">Motion</h3>
      <label class="choice">
        <input
          type="checkbox"
          checked={settings.reducedMotion}
          onchange={(event) =>
            onChange({ ...settings, reducedMotion: event.currentTarget.checked })}
        />
        <span><strong>Reduce motion</strong><em>Skip smooth scrolling and animated transitions</em></span>
      </label>
    </section>
  </aside>
</div>

<style>
  .panel-scrim {
    position: absolute;
    inset: 0;
    z-index: 35;
    display: grid;
    justify-items: end;
    background: rgb(10 12 15 / 45%);
  }

  .panel {
    width: min(380px, 100%);
    height: 100%;
    overflow: auto;
    padding: 18px 20px 26px;
    border-left: 1px solid rgb(255 255 255 / 10%);
    background: #1b1e24;
    color: #e9ebee;
  }

  .heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 14px;
  }

  .heading span {
    color: #6a727c;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 2px 0 0;
    font-size: 17px;
  }

  .close {
    width: 28px;
    height: 28px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 8px;
    background: transparent;
    color: #aeb5be;
    font-size: 16px;
    cursor: pointer;
  }

  .close:hover {
    background: rgb(255 255 255 / 6%);
    color: #e9ebee;
  }

  section {
    padding: 14px 0;
    border-top: 1px solid rgb(255 255 255 / 8%);
  }

  h3 {
    margin: 0 0 10px;
    color: #aeb5be;
    font-size: 12px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .hint {
    margin: 0 0 14px;
    color: #6a727c;
    font-size: 12px;
    line-height: 1.5;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }

  label {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: #aeb5be;
  }

  label > span {
    flex: none;
    width: 68px;
  }

  input[type="range"] {
    flex: 1;
    accent-color: #4c8df0;
  }

  output {
    flex: none;
    width: 52px;
    color: #e9ebee;
    font-variant-numeric: tabular-nums;
    font-size: 11.5px;
    text-align: right;
  }

  .choice-row {
    display: grid;
    gap: 8px;
  }

  .choice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 9px 11px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 9px;
    cursor: pointer;
  }

  .choice:hover {
    background: rgb(255 255 255 / 4%);
  }

  .choice input {
    margin-top: 2px;
    accent-color: #4c8df0;
  }

  .choice span {
    display: grid;
    gap: 2px;
    width: auto;
  }

  .choice strong {
    color: #e9ebee;
    font-size: 12.5px;
    font-weight: 600;
  }

  .choice em {
    color: #6a727c;
    font-size: 11.5px;
    font-style: normal;
  }
</style>
