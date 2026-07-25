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

/** Half an inch. Wide enough that a rule near the edge does not read as a printing error. */
const MARGIN_PT = 36;
/** 8.7mm, the ruling on a standard college pad. */
const RULED_PT = 8.7 * MM;

/**
 * Paper colour, and the rulings that go with it.
 *
 * Ruling colours travel with the paper rather than being fixed per template, because a ruling is
 * defined by how far it sits from the page, not by a hex value. The greys that read as a faint
 * guide on white are a glare on black, so each tone brings its own.
 */
export type PaperTone = {
  id: string;
  name: string;
  backgroundColor: string;
  /** The guide a hand writes along. Barely there by design. */
  rule: string;
  /** Heavier, for rules that structure a page rather than guide handwriting. */
  ruleStrong: string;
  dot: string;
  /** A rule that is meant to be noticed — the margin on a legal pad. */
  accent: string;
};

export const PAPER_TONES: PaperTone[] = [
  {
    id: "white",
    name: "White",
    backgroundColor: "#FCFCFA",
    rule: "#D4DAE0",
    ruleStrong: "#BFC7D0",
    dot: "#C3CBD3",
    // Oxide, from the palette's own presets rather than a red invented for one template.
    accent: "#E5645E",
  },
  {
    id: "cream",
    name: "Cream",
    backgroundColor: "#F7F2E7",
    rule: "#DED5C2",
    ruleStrong: "#C8BCA3",
    dot: "#CEC4AE",
    accent: "#CE6E5E",
  },
  {
    id: "black",
    name: "Black",
    backgroundColor: "#14181C",
    // Lighter than the paper, not lighter than the ink: on a dark page the ruling has to be
    // findable without competing with what gets written on it.
    rule: "#2B323A",
    ruleStrong: "#3C4550",
    dot: "#333B45",
    accent: "#8C4B45",
  },
];

export type TemplateGroup = {
  id: string;
  title: string;
  templates: PageTemplate[];
};

/**
 * The library, in one tone. Definitions come out with concrete colours because that is what gets
 * stored: a page carries the paper it was made on, not a reference to a palette that might not
 * exist wherever the notebook is opened next.
 */
export function templateGroups(tone: PaperTone): TemplateGroup[] {
  const PAPER = tone.backgroundColor;
  const RULE = tone.rule;
  const RULE_STRONG = tone.ruleStrong;
  const DOT = tone.dot;
  const ACCENT = tone.accent;
  // Kept distinct per tone so a "recent templates" list can tell white ruled from black ruled.
  const at = (id: string) => `${id}-${tone.id}`;

  return [
    {
      id: "plain",
      title: "Plain",
      templates: [
        // Cream used to be a template of its own. It is a paper colour, and every template can
        // have it now, so it does not need to be a separate sheet.
        { id: at("blank"), name: "Blank", backgroundColor: PAPER, marginPt: 0, elements: [] },
      ],
    },
    {
      id: "lines",
      title: "Lines",
      templates: [
        {
          id: at("ruled-wide"),
          name: "Ruled",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [
            { kind: "horizontal_lines", spacingPt: RULED_PT, offsetPt: 0, color: RULE, weightPt: 0.5 },
          ],
        },
        {
          id: at("ruled-narrow"),
          name: "Ruled narrow",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [
            { kind: "horizontal_lines", spacingPt: 6 * MM, offsetPt: 0, color: RULE, weightPt: 0.5 },
          ],
        },
        {
          id: at("legal"),
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
          id: at("cornell"),
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
          id: at("columns-two"),
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
          id: at("squared-5mm"),
          name: "Squared",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [
            { kind: "horizontal_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
            { kind: "vertical_lines", spacingPt: 5 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
          ],
        },
        {
          id: at("squared-10mm"),
          name: "Squared wide",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [
            { kind: "horizontal_lines", spacingPt: 10 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
            { kind: "vertical_lines", spacingPt: 10 * MM, offsetPt: 0, color: RULE, weightPt: 0.4 },
          ],
        },
        {
          id: at("graph-5mm"),
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
          id: at("dotted-5mm"),
          name: "Dotted",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [{ kind: "dots", spacingPt: 5 * MM, offsetPt: 0, color: DOT, radiusPt: 0.6 }],
        },
        {
          id: at("dotted-10mm"),
          name: "Dotted wide",
          backgroundColor: PAPER,
          marginPt: MARGIN_PT,
          elements: [{ kind: "dots", spacingPt: 10 * MM, offsetPt: 0, color: DOT, radiusPt: 0.7 }],
        },
      ],
    },
  ];
}
