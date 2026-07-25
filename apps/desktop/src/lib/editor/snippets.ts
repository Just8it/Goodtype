import {
  type Completion,
  type CompletionContext,
  type CompletionResult,
  snippetCompletion,
} from "@codemirror/autocomplete";

/**
 * Static STEM snippets for the Typst editor (Phase 2 §3.6). Each inserts ordinary Typst
 * source; there is no template system, marketplace, or dynamic execution. `#{}` marks the
 * cursor stops that Tab moves through.
 */
export const STEM_SNIPPETS: ReadonlyArray<{
  label: string;
  detail: string;
  template: string;
}> = [
  { label: "math", detail: "display math", template: "$ ${content} $" },
  { label: "inline", detail: "inline math", template: "$${content}$" },
  { label: "frac", detail: "fraction", template: "frac(${numerator}, ${denominator})" },
  { label: "sqrt", detail: "square root", template: "sqrt(${radicand})" },
  { label: "root", detail: "nth root", template: "root(${index}, ${radicand})" },
  { label: "sum", detail: "sum with limits", template: "sum_(${i = 1})^(${n}) ${term}" },
  {
    label: "integral",
    detail: "definite integral",
    template: "integral_(${lower})^(${upper}) ${integrand} dif ${x}",
  },
  { label: "lim", detail: "limit", template: "lim_(${x -> oo}) ${expression}" },
  { label: "vec", detail: "column vector", template: "vec(${a}, ${b})" },
  { label: "arrow", detail: "vector arrow", template: "arrow(${v})" },
  {
    label: "matrix",
    detail: "2×2 matrix",
    template: "mat(\n  ${a}, ${b};\n  ${c}, ${d};\n)",
  },
  {
    label: "cases",
    detail: "case distinction",
    template: "cases(\n  ${value} & ${condition},\n  ${value2} & ${otherwise},\n)",
  },
  {
    label: "aligned",
    detail: "aligned equations",
    template: "$ ${lhs} &= ${rhs} \\\\\n  &= ${next} $",
  },
  { label: "binom", detail: "binomial coefficient", template: "binom(${n}, ${k})" },
  { label: "abs", detail: "absolute value", template: "abs(${x})" },
  { label: "norm", detail: "vector norm", template: "norm(${v})" },
  {
    label: "table",
    detail: "compact table",
    template: "#table(\n  columns: ${2},\n  [${Header A}], [${Header B}],\n  [${a}], [${b}],\n)",
  },
  {
    label: "figure",
    detail: "figure with caption",
    template: "#figure(\n  ${content},\n  caption: [${Caption}],\n)",
  },
  { label: "code", detail: "code block", template: "```${language}\n${code}\n```" },
  { label: "heading", detail: "section heading", template: "= ${Title}" },
];

const completions: Completion[] = STEM_SNIPPETS.map((snippet) =>
  snippetCompletion(snippet.template, {
    label: snippet.label,
    detail: snippet.detail,
    type: "keyword",
  }),
);

/**
 * Complete from the word before the cursor. Explicit invocation (Ctrl+Space) lists every
 * snippet; while typing, at least two letters must match so ordinary prose stays quiet.
 */
export function typstSnippetSource(context: CompletionContext): CompletionResult | null {
  const word = context.matchBefore(/[A-Za-z]+/);
  if (!word && !context.explicit) return null;
  if (word && word.text.length < 2 && !context.explicit) return null;
  return {
    from: word?.from ?? context.pos,
    options: completions,
    validFor: /^[A-Za-z]*$/,
  };
}
