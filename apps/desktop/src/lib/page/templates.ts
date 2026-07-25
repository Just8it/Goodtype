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
// Rulings are quoted in millimetres, because that is how paper is sold, and converted once. The
// resolver centres each grid in its area, so these spacings are exact on any page size — a 5mm
// square is 5mm on A5 and on A3, with the leftover split evenly between both margins rather than
// dumped against one edge.

import type { Area, GridMajor, PageTemplate, TemplateElement } from "../model";

/** 1mm in points. */
const MM = 72 / 25.4;

/**
 * 5mm. Half an inch was a printer's margin, and it wasted a band of page all the way round on a
 * surface that has no printer and no gutter — the ruling should reach nearly to the edge, the
 * way squared paper in a pad does.
 */
const MARGIN_PT = 5 * MM;
/** 8.7mm, the ruling on a standard college pad. */
const RULED_PT = 8.7 * MM;
/** 1.75in — the cue column on a Cornell sheet. */
const CORNELL_CUE_PT = 126;
/** 2in — the summary band along the bottom of a Cornell sheet. */
const CORNELL_SUMMARY_PT = 144;
/** The margin rule on a legal pad, and the inset the ruling starts at to clear it. */
const LEGAL_RULE_PT = 42;

/** Insets from every edge. */
function inset(all: number): Area {
  return { topPt: all, rightPt: all, bottomPt: all, leftPt: all };
}

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
  const page = inset(MARGIN_PT);
  // Kept distinct per tone so a "recent templates" list can tell white ruled from black ruled.
  const at = (id: string) => `${id}-${tone.id}`;

  const ruled = (area: Area, spacingPt: number): TemplateElement => ({
    kind: "horizontal_lines",
    area,
    spacingPt,
    color: tone.rule,
    weightPt: 0.5,
  });
  const grid = (spacingPt: number, weightPt: number, major: GridMajor | null): TemplateElement => ({
    kind: "grid",
    area: page,
    spacingPt,
    color: tone.rule,
    weightPt,
    major,
  });

  return [
    {
      id: "plain",
      title: "Plain",
      templates: [
        // Cream used to be a template of its own. It is a paper colour, and every template can
        // have it now, so it does not need to be a separate sheet.
        { id: at("blank"), name: "Blank", backgroundColor: tone.backgroundColor, elements: [] },
      ],
    },
    {
      id: "lines",
      title: "Lines",
      templates: [
        {
          id: at("ruled-wide"),
          name: "Ruled",
          backgroundColor: tone.backgroundColor,
          elements: [ruled(page, RULED_PT)],
        },
        {
          id: at("ruled-narrow"),
          name: "Ruled narrow",
          backgroundColor: tone.backgroundColor,
          elements: [ruled(page, 6 * MM)],
        },
        {
          id: at("legal"),
          name: "Legal pad",
          backgroundColor: tone.backgroundColor,
          elements: [
            {
              kind: "rule",
              area: inset(MARGIN_PT),
              edge: "left",
              offsetPt: LEGAL_RULE_PT,
              color: tone.accent,
              weightPt: 0.7,
            },
            // Starts clear of the margin rule rather than crossing it.
            ruled(
              { topPt: MARGIN_PT, rightPt: MARGIN_PT, bottomPt: MARGIN_PT, leftPt: LEGAL_RULE_PT },
              RULED_PT,
            ),
          ],
        },
        {
          id: at("cornell"),
          name: "Cornell",
          backgroundColor: tone.backgroundColor,
          // The cue column and the summary band are what make it Cornell rather than ruled
          // paper, and they only work if they bound the ruling instead of being drawn over it.
          elements: [
            {
              kind: "rule",
              area: {
                topPt: MARGIN_PT,
                rightPt: MARGIN_PT,
                bottomPt: CORNELL_SUMMARY_PT,
                leftPt: MARGIN_PT,
              },
              edge: "left",
              offsetPt: CORNELL_CUE_PT,
              color: tone.ruleStrong,
              weightPt: 0.9,
            },
            {
              kind: "rule",
              area: inset(MARGIN_PT),
              edge: "bottom",
              offsetPt: CORNELL_SUMMARY_PT,
              color: tone.ruleStrong,
              weightPt: 0.9,
            },
            ruled(
              {
                topPt: MARGIN_PT,
                rightPt: MARGIN_PT,
                bottomPt: CORNELL_SUMMARY_PT,
                leftPt: CORNELL_CUE_PT,
              },
              RULED_PT,
            ),
          ],
        },
        {
          id: at("columns-two"),
          name: "Two columns",
          backgroundColor: tone.backgroundColor,
          elements: [
            {
              kind: "rule",
              area: inset(MARGIN_PT),
              // Centre-relative, so the divider stays in the middle on A5 and A3 alike.
              edge: "center_x",
              offsetPt: 0,
              color: tone.ruleStrong,
              weightPt: 0.7,
            },
            ruled(page, RULED_PT),
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
          backgroundColor: tone.backgroundColor,
          elements: [grid(5 * MM, 0.4, null)],
        },
        {
          id: at("squared-10mm"),
          name: "Squared wide",
          backgroundColor: tone.backgroundColor,
          elements: [grid(10 * MM, 0.4, null)],
        },
        {
          id: at("graph-5mm"),
          name: "Graph",
          backgroundColor: tone.backgroundColor,
          // Every fifth line heavier, which is what makes a graph grid countable rather than
          // just dense. As one grid rather than two, the heavy lines are the same lines — they
          // cannot drift off the fine ones or stop short of them.
          elements: [grid(5 * MM, 0.3, { every: 5, color: tone.ruleStrong, weightPt: 0.7 })],
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
          backgroundColor: tone.backgroundColor,
          elements: [{ kind: "dots", area: page, spacingPt: 5 * MM, color: tone.dot, radiusPt: 0.6 }],
        },
        {
          id: at("dotted-10mm"),
          name: "Dotted wide",
          backgroundColor: tone.backgroundColor,
          elements: [{ kind: "dots", area: page, spacingPt: 10 * MM, color: tone.dot, radiusPt: 0.7 }],
        },
      ],
    },
  ];
}
