import { translate, type Translator } from "../i18n";
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
export function typstSnippets(t: Translator = translate): ReadonlyArray<{
  label: string;
  detail: string;
  template: string;
}> { return [
  { label: "math", detail: t("editor.displayMath"), template: "$ ${content} $" },
  { label: "inline", detail: t("editor.inlineMath2"), template: "$${content}$" },
  { label: "frac", detail: "fraction", template: "frac(${numerator}, ${denominator})" },
  { label: "sqrt", detail: t("editor.squareRoot"), template: "sqrt(${radicand})" },
  { label: "root", detail: t("editor.nthRoot"), template: "root(${index}, ${radicand})" },
  { label: "sum", detail: t("editor.sumWithLimits"), template: "sum_(${i = 1})^(${n}) ${term}" },
  {
    label: "integral",
    detail: t("editor.definiteIntegral"),
    template: "integral_(${lower})^(${upper}) ${integrand} dif ${x}",
  },
  { label: "lim", detail: "limit", template: "lim_(${x -> oo}) ${expression}" },
  { label: "vec", detail: t("editor.columnVector"), template: "vec(${a}, ${b})" },
  { label: "arrow", detail: t("editor.vectorArrow"), template: "arrow(${v})" },
  {
    label: "matrix",
    detail: t("editor.matrix"),
    template: "mat(\n  ${a}, ${b};\n  ${c}, ${d};\n)",
  },
  {
    label: "cases",
    detail: t("editor.caseDistinction"),
    template: "cases(\n  ${value} & ${condition},\n  ${value2} & ${otherwise},\n)",
  },
  {
    label: "aligned",
    detail: t("editor.alignedEquations"),
    template: "$ ${lhs} &= ${rhs} \\\\\n  &= ${next} $",
  },
  { label: "binom", detail: t("editor.binomialCoefficient"), template: "binom(${n}, ${k})" },
  { label: "abs", detail: t("editor.absoluteValue"), template: "abs(${x})" },
  { label: "norm", detail: t("editor.vectorNorm"), template: "norm(${v})" },
  {
    label: "table",
    detail: t("editor.compactTable"),
    template: "#table(\n  columns: ${2},\n  [${Header A}], [${Header B}],\n  [${a}], [${b}],\n)",
  },
  {
    label: "figure",
    detail: t("editor.figureWithCaption"),
    template: "#figure(\n  ${content},\n  caption: [${Caption}],\n)",
  },
  { label: "code", detail: t("editor.codeBlock"), template: "```${language}\n${code}\n```" },
  { label: "quote", detail: t("editor.blockQuote"), template: "> ${quote}" },
  { label: "heading", detail: t("editor.sectionHeading"), template: "= ${Title}" },
]; }
export const TYPST_SNIPPETS = typstSnippets();

/** Kept as a public alias for existing callers; completion and the writing bar use one registry. */
export const STEM_SNIPPETS = TYPST_SNIPPETS;

function completions(): Completion[] { return typstSnippets().map((snippet) =>
  snippetCompletion(snippet.template, {
    label: snippet.label,
    detail: snippet.detail,
    type: "keyword",
  }),
); }

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
    options: completions(),
    validFor: /^[A-Za-z]*$/,
  };
}
