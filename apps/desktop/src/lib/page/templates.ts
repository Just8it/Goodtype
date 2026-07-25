// The templates Goodtype ships with.
//
// They live here rather than in Rust on purpose. Choosing a template copies its definition onto
// the page, so Rust only ever has to *render* what a page carries — it never needs a library of
// its own to keep in step with this one. Adding a template is one entry in this list.
//
// Spacings are in points, because that is what the page is measured in. The familiar paper sizes
// in millimetres are noted where they came from.

import type { PageTemplate } from "../model";

/** 1mm in points. Rulings are quoted in millimetres everywhere paper is sold. */
const MM = 72 / 25.4;

const PAPER = "#FCFCFA";
const RULE = "#D4DAE0";
const DOT = "#C3CBD3";
/** Oxide, from the palette's own presets rather than a red invented for one template. */
const ACCENT = "#E5645E";
/** Half an inch. Wide enough that a rule near the edge does not read as a printing error. */
const MARGIN_PT = 36;

export const BUILT_IN_TEMPLATES: PageTemplate[] = [
  {
    id: "blank",
    name: "Blank",
    backgroundColor: PAPER,
    marginPt: 0,
    elements: [],
  },
  {
    id: "ruled-wide",
    name: "Ruled",
    backgroundColor: PAPER,
    marginPt: MARGIN_PT,
    // 8.7mm, the ruling on a standard college pad.
    elements: [
      { kind: "horizontal_lines", spacingPt: 8.7 * MM, offsetPt: 0, color: RULE, weightPt: 0.5 },
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
    id: "dotted-5mm",
    name: "Dotted",
    backgroundColor: PAPER,
    marginPt: MARGIN_PT,
    elements: [{ kind: "dots", spacingPt: 5 * MM, offsetPt: 0, color: DOT, radiusPt: 0.6 }],
  },
  {
    id: "legal",
    name: "Legal pad",
    backgroundColor: PAPER,
    // Wider on the left so the ruling starts clear of the margin rule.
    marginPt: 54,
    elements: [
      { kind: "rule", edge: "left", offsetPt: 42, color: ACCENT, weightPt: 0.7 },
      { kind: "horizontal_lines", spacingPt: 8.7 * MM, offsetPt: 0, color: RULE, weightPt: 0.5 },
    ],
  },
  {
    id: "cornell",
    name: "Cornell",
    backgroundColor: PAPER,
    marginPt: MARGIN_PT,
    // The cue column at 1.75in from the left and the summary band 2in up from the bottom are
    // what make it Cornell rather than ruled paper.
    elements: [
      { kind: "horizontal_lines", spacingPt: 8.7 * MM, offsetPt: 0, color: RULE, weightPt: 0.5 },
      { kind: "rule", edge: "left", offsetPt: 126, color: RULE, weightPt: 0.9 },
      { kind: "rule", edge: "bottom", offsetPt: 144, color: RULE, weightPt: 0.9 },
    ],
  },
];
