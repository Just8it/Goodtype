import {
  snippetCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { completeTypst, type TypstCompletionItem } from "../ipc/typst";

// Compiler-derived completion. Candidates come from the same in-process Typst world
// that renders the block, so they always match the language the block is compiled with.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Rust indexes source as UTF-8 bytes; CodeMirror indexes it as UTF-16 code units. The two agree
 * only for ASCII, so every offset crossing the boundary is converted — otherwise a document
 * containing any math symbol (`∑` is 3 bytes, 1 unit) would splice completions at the wrong
 * place.
 */
export function toByteOffset(text: string, index: number): number {
  return encoder.encode(text.slice(0, index)).length;
}

export function fromByteOffset(text: string, byteOffset: number): number {
  const bytes = encoder.encode(text);
  if (byteOffset >= bytes.length) return text.length;
  return decoder.decode(bytes.slice(0, byteOffset)).length;
}

/** CodeMirror snippet fields are `#{…}`; Tinymist uses numbered LSP tab stops. */
export function toSnippetTemplate(apply: string): string {
  return apply
    .replace(/\$\{\d+:([^}]*)\}/g, (_match, fallback: string) => `#{${fallback}}`)
    .replace(/\$\{\d+\}/g, "#{}")
    .replace(/\$\d+/g, "#{}")
    .replace(/\$\{([^}]*)\}/g, (_match, name: string) => `#{${name}}`);
}

function toCompletion(item: TypstCompletionItem): Completion {
  // A math symbol shows its glyph, which is the whole point of completing `sum` to `∑`.
  const detail = item.symbol ? `${item.symbol}  ${item.detail ?? ""}`.trim() : item.detail;
  const base = {
    label: item.label,
    type: item.kind,
    detail: detail || undefined,
  };
  const apply = item.apply;
  if (apply && apply.includes("${")) {
    // Placeholders the writer tabs through, rather than an insertion they must clean up.
    return snippetCompletion(toSnippetTemplate(apply), base);
  }
  return apply ? { ...base, apply } : base;
}

/**
 * Ask Rust to complete at the caret. `getRoot` is a callback because the notebook root can
 * change (a different notebook opens) while one editor instance lives on.
 */
export function createTypstCompletionSource(getRoot: () => string | null) {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const root = getRoot();
    if (!root) return null;

    const source = context.state.doc.toString();
    const cursor = toByteOffset(source, context.pos);

    let items: TypstCompletionItem[];
    try {
      items = await completeTypst(root, source, cursor, context.explicit);
    } catch {
      // Completion is an assist: a failure must never interrupt writing.
      return null;
    }
    if (items.length === 0) return null;

    return {
      from: fromByteOffset(source, items[0].offset),
      options: items.map(toCompletion),
      // Results are position-specific; let CodeMirror re-query rather than filter stale ones.
      validFor: /^[\w.-]*$/,
    };
  };
}
