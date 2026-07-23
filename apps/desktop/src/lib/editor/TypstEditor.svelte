<script lang="ts">
  import { basicSetup, EditorView } from "codemirror";
  import { onMount } from "svelte";

  let {
    value,
    ariaLabel = "Typst source",
    onChange,
    onExit,
  }: {
    value: string;
    ariaLabel?: string;
    onChange: (value: string) => void;
    onExit: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let applyingExternalValue = false;

  onMount(() => {
    view = new EditorView({
      doc: value,
      extensions: [
        basicSetup,
        EditorView.contentAttributes.of({ "aria-label": ariaLabel }),
        EditorView.domEventHandlers({
          keydown(event) {
            if (event.key !== "Escape") return false;
            onExit();
            return true;
          },
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !applyingExternalValue) {
            onChange(update.state.doc.toString());
          }
        }),
      ],
      parent: host,
    });
    view.focus();

    return () => {
      view?.destroy();
      view = undefined;
    };
  });

  $effect(() => {
    if (!view || view.state.doc.toString() === value) return;
    applyingExternalValue = true;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
    applyingExternalValue = false;
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    min-width: 18rem;
    max-width: 42rem;
    text-align: left;
  }

  .editor :global(.cm-editor) {
    max-height: 14rem;
    border: 1px solid rgb(255 255 255 / 10%);
    border-top: 0;
    border-radius: 0 0 8px 8px;
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
</style>
