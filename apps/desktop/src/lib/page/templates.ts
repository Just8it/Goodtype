// The templates Goodtype ships with, grouped the way the picker shows them.
//
// They live here rather than in Rust on purpose. Choosing a template copies its definition onto
// the page, so Rust only ever has to *render* what a page carries — it never needs a library of
// its own to keep in step with this one. Adding a template is one entry in this list.
//
// The grouping is here rather than on `PageTemplate` because a category is how the picker is
// organised, not something a page needs to record. Nothing on disk should have to change when
// the shelves get rearranged.
//
// Spacings are in points, because that is what the page is measured in. The familiar paper sizes
// in millimetres are noted where they came from.

import type { PageTemplate } from "../model";

/** 1mm in points. Rulings are quoted in millimetres everywhere paper is sold. */
const MM = 72 / 25.4;

const PAPER = "#FCFCFA";
const RULE = "#D4DAE0";
/** Heavier, for the rules that structure a page rather than guide handwriting. */
const RULE_STRONG = "#BFC7D0";
const DOT = "#C3CBD3";
/** Oxide, from the palette's own presets rather than a red invented for one template. */
const ACCENT = "#E5645E";
/** Half an inch. Wide enough that a rule near the edge does not read as a printing error. */
const MARGIN_PT = 36;
/** 8.7mm, the ruling on a standard college pad. */
const RULED_PT = 8.7 * MM;

export type TemplateGroup = {
  id: string;
  title: string;
  templates: PageTemplate[];
};

export const TEMPLATE_GROUPS: TemplateGroup[] = [
  {
    id: "plain",
    title: "Plain",
    templates: [
      { id: "blank", name: "Blank", backgroundColor: PAPER, marginPt: 0, elements: [] },
      {
        id: "blank-warm",
        name: "Cream",
        backgroundColor: "#F7F2E7",
        marginPt: 0,
        elements: [],
      },
    ],
  },
  {
    id: "lines",
    title: "Lines",
    templates: [
      {
        id: "ruled-wide",
        name: "Ruled",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [
          { kind: "horizontal_lines", spacingPt: RULED_PT, offsetPt: 0, color: RULE, weightPt: 0.5 },
        ],
      },
      {
        id: "ruled-narrow",
        name: "Ruled narrow",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [
          { kind: "horizontal_lines", spacingPt: 6 * MM, offsetPt: 0, color: RULE, weightPt: 0.5 },
        ],
      },
      {
        id: "legal",
        name: "Legal pad",
        backgroundColor: PAPER,
        // Wider on the left so the ruling starts clear of the margin rule.
        marginPt: 54,
        elements: [
          { kind: "rule", edge: "left", offsetPt: 42, color: ACCENT, weightPt: 0.7 },
          { kind: "horizontal_lines", spacingPt: RULED_PT, offsetPt: 0, color: RULE, weightPt: 0.5 },
        ],
      },
      {
        id: "cornell",
        name: "Cornell",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        // The cue column 1.75in from the left and the summary band 2in up from the bottom are
        // what make it Cornell rather than ruled paper.
        elements: [
          { kind: "horizontal_lines", spacingPt: RULED_PT, offsetPt: 0, color: RULE, weightPt: 0.5 },
          { kind: "rule", edge: "left", offsetPt: 126, color: RULE_STRONG, weightPt: 0.9 },
          { kind: "rule", edge: "bottom", offsetPt: 144, color: RULE_STRONG, weightPt: 0.9 },
        ],
      },
      {
        id: "columns-two",
        name: "Two columns",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [
          { kind: "rule", edge: "left", offsetPt: 297.64, color: RULE_STRONG, weightPt: 0.7 },
          { kind: "horizontal_lines", spacingPt: RULED_PT, offsetPt: 0, color: RULE, weightPt: 0.5 },
        ],
      },
    ],
  },
  {
    id: "squares",
    title: "Squares",
    templates: [
      {
        id: "squared-5mm",
        name: "Squared",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [
          { kind: "horizontal_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
          { kind: "vertical_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
        ],
      },
      {
        id: "squared-10mm",
        name: "Squared wide",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [
          { kind: "horizontal_lines", spacingPt: 10 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
          { kind: "vertical_lines", spacingPt: 10 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
        ],
      },
      {
        id: "graph-5mm",
        name: "Graph",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        // Every fifth line heavier, which is what makes a graph grid countable rather than just
        // dense. Both spacings share an origin, so the heavy lines land on light ones.
        elements: [
          { kind: "horizontal_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.3 },
          { kind: "vertical_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.3 },
          { kind: "horizontal_lines", spacingPt: 25 * MM, offsetPt: 0, color: RULE_STRONG, weightPt: 0.7 },
          { kind: "vertical_lines", spacingPt: 25 * MM, offsetPt: 0, color: RULE_STRONG, weightPt: 0.7 },
        ],
      },
    ],
  },
  {
    id: "dots",
    title: "Dots",
    templates: [
      {
        id: "dotted-5mm",
        name: "Dotted",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [{ kind: "dots", spacingPt: 5 * MM, offsetPt: 0, color: DOT, radiusPt: 0.6 }],
      },
      {
        id: "dotted-10mm",
        name: "Dotted wide",
        backgroundColor: PAPER,
        marginPt: MARGIN_PT,
        elements: [{ kind: "dots", spacingPt: 10 * MM, offsetPt: 0, color: DOT, radiusPt: 0.7 }],
      },
    ],
  },
];
