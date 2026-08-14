<script lang="ts">
  import { acceptCompletion, autocompletion } from "@codemirror/autocomplete";
  import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
  import { keymap, tooltips } from "@codemirror/view";
  import { Prec } from "@codemirror/state";
  import { basicSetup, EditorView } from "codemirror";
  import { onMount } from "svelte";
  import { createTypstCompletionSource, fromByteOffset, toByteOffset } from "./completion";
  import { setTypstHighlights, typstHighlighting } from "./highlighting";
  import { newlineInsidePair, spaceInsideMath, typstPairLanguageData } from "./smartPairs";
  import { typstSnippetSource } from "./snippets";
  import { applyWritingCommand as runWritingCommand, type WritingCommand } from "./writingCommands";
  import { analyzeTypst, formatTypst, hoverTypst, type TypstHover } from "../ipc/typst";

  let {
    value,
    root = null,
    ariaLabel = "Typst source",
    maxLines = 10,
    onChange,
    onExit,
  }: {
    value: string;
    /** Notebook root, needed to ask Rust for compiler-derived completions. */
    root?: string | null;
    ariaLabel?: string;
    /** Height ceiling, in lines. `null` fills the available height (the side view). */
    maxLines?: number | null;
    onChange: (value: string) => void;
    onExit: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let applyingExternalValue = false;
  let highlightTimer: ReturnType<typeof setTimeout> | undefined;
  let highlightGeneration = 0;
  let help = $state<TypstHover | null>(null);
  let helpStatus = $state("");
  const lineCount = $derived(value ? value.split("\n").length : 1);
  const hiddenLines = $derived(
    maxLines === null ? 0 : Math.max(0, lineCount - maxLines),
  );

  onMount(() => {
    view = new EditorView({
      doc: value,
      extensions: [
        basicSetup,
        typstPairLanguageData,
        typstHighlighting,
        // Compiler-derived completion alongside the multi-line STEM templates, which
        // the compiler has no equivalent for.
        autocompletion({
          override: [createTypstCompletionSource(() => root), typstSnippetSource],
        }),
        // The block editor lives inside the page, which clips its overflow and stacks the ink
        // layer above it. Rendering the completion popup into the body escapes both, so it can
        // sit above the editor, the rendered preview, and everything else while you type.
        tooltips({ parent: document.body, position: "fixed" }),
        // Declares the editor dark so CodeMirror applies its dark base theme. Without this its
        // `&light .cm-tooltip` rule paints the completion popup near-white, and that rule is
        // more specific than any plain `.cm-tooltip` override we could write.
        EditorView.theme({}, { dark: true }),
        // Tab accepts the highlighted candidate; when no completion is open this returns false
        // so Tab falls through to snippet-field navigation and then to indentation.
        Prec.highest(
          keymap.of([
            { key: "Tab", run: acceptCompletion },
            { key: "Space", run: spaceInsideMath },
            { key: "Enter", run: newlineInsidePair },
            { key: "Mod-b", run: (target) => runWritingCommand(target, "bold") },
            { key: "Mod-i", run: (target) => runWritingCommand(target, "italic") },
            { key: "Mod-u", run: (target) => runWritingCommand(target, "underline") },
            { key: "Mod-Alt-1", run: (target) => runWritingCommand(target, "heading-1") },
            { key: "Mod-Alt-2", run: (target) => runWritingCommand(target, "heading-2") },
            { key: "Mod-Alt-3", run: (target) => runWritingCommand(target, "heading-3") },
            {
              key: "F1",
              run: () => {
                void showHelp();
                return true;
              },
            },
            {
              key: "Mod-Shift-f",
              run: () => {
                void formatDocument();
                return true;
              },
            },
          ]),
        ),
        EditorView.contentAttributes.of({ "aria-label": ariaLabel }),
        EditorView.domEventHandlers({
          keydown(event) {
            if (event.key !== "Escape") return false;
            onExit();
            return true;
          },
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            const source = update.state.doc.toString();
            scheduleHighlight(source);
            if (applyingExternalValue) return;
            help = null;
            helpStatus = "";
            onChange(source);
          }
        }),
      ],
      parent: host,
    });
    view.focus();
    scheduleHighlight(value);

    return () => {
      clearTimeout(highlightTimer);
      highlightGeneration += 1;
      view?.destroy();
      view = undefined;
    };
  });

  /// Lets the side view take the caret back after a pen stroke, so drawing on the canvas never
  /// costs the writer their place in the source.
  export function focus() {
    view?.focus();
  }

  export function applyWritingCommand(command: WritingCommand) {
    if (view) runWritingCommand(view, command);
  }

  export async function showHelp() {
    if (!view || !root) return;
    const source = view.state.doc.toString();
    helpStatus = "Looking up Typst help...";
    try {
      help = await hoverTypst(
        root,
        source,
        toByteOffset(source, view.state.selection.main.head),
      );
      helpStatus = help ? "" : "No Typst help at the caret";
    } catch {
      help = null;
      helpStatus = "Typst help is unavailable";
    }
  }

  export async function formatDocument() {
    if (!view || !root) return;
    const source = view.state.doc.toString();
    helpStatus = "Formatting Typst...";
    try {
      const formatted = await formatTypst(root, source);
      if (formatted !== source) {
        view.dispatch({
          changes: { from: 0, to: view.state.doc.length, insert: formatted },
        });
      }
      helpStatus = formatted === source ? "Typst source is already formatted" : "Formatted Typst source";
    } catch {
      helpStatus = "Typst formatting failed";
    }
  }

  function scheduleHighlight(source: string) {
    clearTimeout(highlightTimer);
    const generation = ++highlightGeneration;
    const requestedRoot = root;
    if (!requestedRoot || !view) {
      clearAnalysis();
      return;
    }
    highlightTimer = setTimeout(async () => {
      try {
        const analysis = await analyzeTypst(requestedRoot, source);
        if (
          generation !== highlightGeneration ||
          root !== requestedRoot ||
          view?.state.doc.toString() !== source
        ) return;
        const diagnostics: Diagnostic[] = analysis.diagnostics.map((diagnostic) => ({
          severity: diagnostic.severity,
          message: diagnostic.message,
          from: fromByteOffset(source, diagnostic.from),
          to: fromByteOffset(source, diagnostic.to),
        }));
        view.dispatch(
          {
            effects: setTypstHighlights.of(analysis.highlights.map((token) => ({
              kind: token.kind,
              modifiers: token.modifiers,
              from: fromByteOffset(source, token.from),
              to: fromByteOffset(source, token.to),
            }))),
          },
          setDiagnostics(view.state, diagnostics),
        );
      } catch {
        // Highlighting is an assist; stale colors map with edits until the next successful pass.
      }
    }, 90);
  }

  function clearAnalysis() {
    if (!view) return;
    view.dispatch(
      { effects: setTypstHighlights.of([]) },
      setDiagnostics(view.state, []),
    );
  }

  $effect(() => {
    if (!view || view.state.doc.toString() === value) return;
    applyingExternalValue = true;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
    applyingExternalValue = false;
  });

  $effect(() => {
    const hasRoot = Boolean(root);
    if (!view) return;
    if (hasRoot) scheduleHighlight(view.state.doc.toString());
    else clearAnalysis();
  });
</script>

<div class="editor" class:filled={maxLines === null} style:--max-lines={maxLines ?? 0}>
  <div class="host" bind:this={host}></div>
  {#if help || helpStatus}
    <div class:code={help?.code} class="help" role="status" aria-live="polite">
      {help?.value ?? helpStatus}
    </div>
  {/if}
  {#if hiddenLines > 0}
    <!-- The box is capped on purpose, so say so rather than looking truncated. -->
    <p class="overflow-hint">
      {hiddenLines}
      {hiddenLines === 1 ? "more line" : "more lines"} — open the side view for the full source
    </p>
  {/if}
</div>

<style>
  .editor {
    min-width: 18rem;
    max-width: 42rem;
    border-radius: 9px;
    box-shadow: 0 14px 36px rgb(0 0 0 / 50%);
    text-align: left;
    /* It floats over the page now rather than sitting flush under the block, so it carries its
       own opaque surface instead of borrowing the block's. */
    overflow: hidden;
  }

  /* Side view: fill the panel instead of floating with a capped height. */
  .editor.filled {
    display: flex;
    max-width: none;
    min-width: 0;
    height: 100%;
    flex-direction: column;
    border-radius: 0;
    box-shadow: none;
  }

  .editor.filled .host {
    min-height: 0;
    flex: 1;
  }

  .editor.filled :global(.cm-editor) {
    max-height: none;
    height: 100%;
    border: 0;
  }

  .editor :global(.cm-editor) {
    /* Capped at `maxLines`; longer sources belong in the side view. 1.65 is the line-height
       set below, and the addend covers the editor's own vertical padding. */
    max-height: calc(var(--max-lines) * 1.65 * 12px + 10px);
    border: 1px solid rgb(255 255 255 / 10%);
    background: #16181d;
    color: #e9ebee;
    font: 12px/1.65 "Cascadia Mono", Consolas, monospace;
  }

  .editor :global(.cm-editor.cm-focused) {
    outline: 1.5px solid #4c8df0;
    outline-offset: 0;
  }

  .editor :global(.cm-scroller) {
    overflow: auto;
  }

  .overflow-hint {
    margin: 0;
    padding: 5px 9px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-top: 0;
    background: #1b1e24;
    color: #aeb5be;
    font: 10px/1.4 Bahnschrift, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }

  .help {
    padding: 7px 9px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-top: 0;
    background: #1b1e24;
    color: #aeb5be;
    font: 11px/1.45 Bahnschrift, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }

  .help.code {
    font-family: "Cascadia Mono", Consolas, monospace;
  }

  .editor :global(.cm-gutters) {
    border-right-color: rgb(255 255 255 / 8%);
    background: #16181d;
    color: #4a525c;
  }

  .editor :global(.cm-activeLine),
  .editor :global(.cm-activeLineGutter) {
    background: rgb(255 255 255 / 4%);
  }

  .editor :global(.cm-cursor) {
    border-left-color: #4c8df0;
  }

  .editor :global(.cm-typst-comment) { color: #737d89; font-style: italic; }
  .editor :global(.cm-typst-string) { color: #a8d1a0; }
  .editor :global(.cm-typst-keyword) { color: #c6a0f6; }
  .editor :global(.cm-typst-number),
  .editor :global(.cm-typst-bool) { color: #f4b76e; }
  .editor :global(.cm-typst-function) { color: #7fb0f7; }
  .editor :global(.cm-typst-type),
  .editor :global(.cm-typst-namespace) { color: #e3a6e8; }
  .editor :global(.cm-typst-label),
  .editor :global(.cm-typst-ref),
  .editor :global(.cm-typst-link),
  .editor :global(.cm-typst-decorator) { color: #75d2c6; }
  .editor :global(.cm-typst-heading) { color: #f1d17a; font-weight: 650; }
  .editor :global(.cm-typst-raw) { color: #d6a872; }
  .editor :global(.cm-typst-escape) { color: #f08c82; }
  .editor :global(.cm-typst-error) {
    text-decoration: underline wavy #e5645e;
    text-underline-offset: 2px;
  }
  .editor :global(.cm-typst-math) { color: #8fc8ff; }
  .editor :global(.cm-lintRange-error) {
    background-image: none;
    background: rgb(229 100 94 / 12%);
    text-decoration: underline wavy #ff6b66;
    text-decoration-thickness: 1.5px;
    text-underline-offset: 3px;
  }
  .editor :global(.cm-lintRange-warning) {
    background-image: none;
    text-decoration: underline wavy #f1c76d;
    text-underline-offset: 3px;
  }

  /* Tooltips render into the document body (see `tooltips()` above) so they are no longer
     descendants of `.editor`; these rules are therefore global. The popup needs a fully opaque
     surface of its own — CodeMirror's default is light and semi-transparent, which is
     unreadable against white paper. */
  :global(.cm-tooltip) {
    /* Above the page, the ink layer, the palette, and the rest of the workspace chrome. */
    z-index: 2000;
    /* Mirrors the workspace's floating panels (--panel/--text and the overflow menu's edge and
       shadow). The values are literal because the popup renders into the body, outside the
       `.workspace-app` subtree where those custom properties are defined.
       `!important` on the two properties CodeMirror's base theme sets itself: its `&dark
       .cm-tooltip` rule is more specific than anything we can write from outside, so this is
       the override point rather than a specificity war. */
    border: 1px solid rgb(255 255 255 / 12%) !important;
    border-radius: 11px;
    background: #23272f !important;
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    color: #e9ebee;
  }

  :global(.cm-tooltip-autocomplete > ul) {
    max-height: 16rem;
    margin: 0;
    padding: 7px;
    /* Monospace, because every candidate is inserted as code. */
    font: 12px/1.6 "Cascadia Mono", Consolas, monospace;
  }

  :global(.cm-tooltip-autocomplete > ul > li) {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 5px 9px;
    border-radius: 7px;
    color: #e9ebee;
  }

  :global(.cm-tooltip-autocomplete > ul > li:hover:not([aria-selected])) {
    background: rgb(255 255 255 / 6%);
  }

  :global(.cm-tooltip-autocomplete > ul > li[aria-selected]) {
    background: #4c8df0;
    color: #ffffff;
  }

  :global(.cm-completionLabel) {
    flex: 1;
  }

  /* The matched characters, so it is obvious why a candidate is in the list. */
  :global(.cm-completionMatchedText) {
    color: #7fb0f7;
    text-decoration: none;
    font-weight: 600;
  }

  :global(.cm-tooltip-autocomplete > ul > li[aria-selected] .cm-completionMatchedText) {
    color: #ffffff;
  }

  /* Carries the glyph for a math symbol (`∑`), so it must stay legible when not selected. */
  :global(.cm-completionDetail) {
    color: #aeb5be;
    font-style: normal;
  }

  :global(.cm-tooltip-autocomplete > ul > li[aria-selected] .cm-completionDetail) {
    color: rgb(255 255 255 / 78%);
  }

  :global(.cm-completionIcon) {
    width: 1.1em;
    padding-right: 0;
    opacity: 0.7;
  }

  :global(.cm-tooltip.cm-completionInfo) {
    margin-left: 4px;
    padding: 6px 8px;
  }
</style>
