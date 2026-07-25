// Resolving a page template into geometry, and drawing it.
//
// The mirror of `crates/goodtype-core/src/template.rs`, which carries the reasoning for why a
// template is numbers rather than artwork and why repeating elements are laid out symmetrically
// about the centre of their area. The canvas needs this in the webview and export needs it in
// Rust, so it exists twice — and both are asserted against `fixtures/templates/resolved.json`,
// which is what stops the screen and the PDF disagreeing about where a line goes.
//
// Steps are computed as `first + index * spacing` rather than accumulated, so the two
// implementations agree bit for bit instead of drifting one rounding error at a time.

import type { Area, PageGeometry, PageTemplate, TemplateElement } from "../model";

/** Matches `MIN_SPACING_PT`: below this a template stops being paper and becomes a fill pattern. */
export const MIN_SPACING_PT = 3;
export const MAX_SHAPES = 20_000;

export type TemplateShape =
  | { kind: "line"; x1: number; y1: number; x2: number; y2: number; color: string; weightPt: number }
  | { kind: "dot"; cx: number; cy: number; radiusPt: number; color: string };

type Bounds = { left: number; top: number; right: number; bottom: number };

export function resolveTemplate(template: PageTemplate, geometry: PageGeometry): TemplateShape[] {
  const shapes: TemplateShape[] = [];

  for (const element of template.elements) {
    const box = bounds(element.area, geometry);
    if (!box) continue;
    switch (element.kind) {
      case "horizontal_lines":
        for (const y of centred(box.top, box.bottom, element.spacingPt)) {
          shapes.push({ kind: "line", x1: box.left, y1: y, x2: box.right, y2: y, color: element.color, weightPt: element.weightPt });
        }
        break;
      case "vertical_lines":
        for (const x of centred(box.left, box.right, element.spacingPt)) {
          shapes.push({ kind: "line", x1: x, y1: box.top, x2: x, y2: box.bottom, color: element.color, weightPt: element.weightPt });
        }
        break;
      case "grid": {
        const xs = centred(box.left, box.right, element.spacingPt);
        const ys = centred(box.top, box.bottom, element.spacingPt);
        if (xs.length === 0 || ys.length === 0) break;
        const x0 = xs[0];
        const x1 = xs[xs.length - 1];
        const y0 = ys[0];
        const y1 = ys[ys.length - 1];
        const major = element.major;
        const heavy = (index: number, count: number) =>
          major && major.every > 0 && Math.abs(index - Math.floor(count / 2)) % major.every === 0
            ? major
            : null;
        // Minor lines first, then the major ones over them, so a heavy rule reads as sitting on
        // the grid rather than being interrupted by it.
        for (const wantMajor of [false, true]) {
          ys.forEach((y, index) => {
            const found = heavy(index, ys.length);
            if (Boolean(found) !== wantMajor) return;
            shapes.push({ kind: "line", x1: x0, y1: y, x2: x1, y2: y, color: found?.color ?? element.color, weightPt: found?.weightPt ?? element.weightPt });
          });
          xs.forEach((x, index) => {
            const found = heavy(index, xs.length);
            if (Boolean(found) !== wantMajor) return;
            shapes.push({ kind: "line", x1: x, y1: y0, x2: x, y2: y1, color: found?.color ?? element.color, weightPt: found?.weightPt ?? element.weightPt });
          });
        }
        break;
      }
      case "dots": {
        const columns = centred(box.left, box.right, element.spacingPt);
        for (const cy of centred(box.top, box.bottom, element.spacingPt)) {
          for (const cx of columns) {
            shapes.push({ kind: "dot", cx, cy, radiusPt: element.radiusPt, color: element.color });
          }
        }
        break;
      }
      case "rule":
        shapes.push(rule(element, box, geometry));
        break;
    }
    if (shapes.length >= MAX_SHAPES) {
      shapes.length = MAX_SHAPES;
      break;
    }
  }
  return shapes;
}

// Measured from the page edge rather than the area, because the offset is the whole point of it:
// "1.75in from the left" has to mean that.
function rule(
  element: Extract<TemplateElement, { kind: "rule" }>,
  box: Bounds,
  geometry: PageGeometry,
): TemplateShape {
  const { color, weightPt, offsetPt, edge } = element;
  const along =
    edge === "left" || edge === "top"
      ? offsetPt
      : edge === "right"
        ? geometry.widthPt - offsetPt
        : edge === "bottom"
          ? geometry.heightPt - offsetPt
          : edge === "center_x"
            ? geometry.widthPt / 2 + offsetPt
            : geometry.heightPt / 2 + offsetPt;
  return edge === "left" || edge === "right" || edge === "center_x"
    ? { kind: "line", x1: along, y1: box.top, x2: along, y2: box.bottom, color, weightPt }
    : { kind: "line", x1: box.left, y1: along, x2: box.right, y2: along, color, weightPt };
}

function bounds(area: Area, geometry: PageGeometry): Bounds | null {
  const box = {
    left: area.leftPt,
    top: area.topPt,
    right: geometry.widthPt - area.rightPt,
    bottom: geometry.heightPt - area.bottomPt,
  };
  return box.right > box.left && box.bottom > box.top ? box : null;
}

/**
 * Positions of `spacing`-apart steps laid out symmetrically about the middle of `start..end`.
 *
 * This is what gives equal margins on both sides and makes multiples of a spacing land on each
 * other. See the Rust module comment for why that matters.
 */
function centred(start: number, end: number, spacing: number): number[] {
  if (!Number.isFinite(spacing) || spacing < MIN_SPACING_PT || end <= start) return [];
  const centre = (start + end) / 2;
  // Whole steps that fit between the centre and either edge.
  const reach = Math.floor((end - start) / 2 / spacing);
  const first = centre - reach * spacing;
  return Array.from({ length: reach * 2 + 1 }, (_, index) => first + index * spacing);
}

/**
 * The template as SVG markup, at page coordinates. Used for the page itself and for the picker's
 * previews, so a preview is never a stored bitmap that can disagree with what gets drawn.
 */
export function templateSvg(template: PageTemplate, geometry: PageGeometry): string {
  return svgFor(template, geometry, geometry.widthPt, geometry.heightPt);
}

/**
 * The fraction of the page a preview shows, measured from the top-left corner.
 *
 * A preview is a swatch, not a thumbnail. Shrunk to tile width, a 5mm grid lands about two
 * pixels apart with hairlines well under one pixel, and the browser resolves that to an even
 * grey — squared and dotted paper come out looking identical and both look blank. Showing a
 * corner at closer to true scale is what makes them tell each other apart. The fraction keeps
 * the page's aspect ratio, so the swatch is not distorted, and it starts at the corner so the
 * margin and any edge rule are inside it.
 */
const PREVIEW_FRACTION = 0.45;

/** The template as a swatch for the picker. See `PREVIEW_FRACTION` for why it is not the page. */
export function templatePreviewSvg(template: PageTemplate, geometry: PageGeometry): string {
  return svgFor(
    template,
    geometry,
    geometry.widthPt * PREVIEW_FRACTION,
    geometry.heightPt * PREVIEW_FRACTION,
  );
}

function svgFor(
  template: PageTemplate,
  geometry: PageGeometry,
  viewWidthPt: number,
  viewHeightPt: number,
): string {
  const body = resolveTemplate(template, geometry)
    .map((shape) =>
      shape.kind === "line"
        ? `<line x1="${round(shape.x1)}" y1="${round(shape.y1)}" x2="${round(shape.x2)}" y2="${round(shape.y2)}" stroke="${shape.color}" stroke-width="${round(shape.weightPt)}"/>`
        : `<circle cx="${round(shape.cx)}" cy="${round(shape.cy)}" r="${round(shape.radiusPt)}" fill="${shape.color}"/>`,
    )
    .join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${round(viewWidthPt)} ${round(viewHeightPt)}" preserveAspectRatio="none"><rect width="100%" height="100%" fill="${template.backgroundColor}"/>${body}</svg>`;
}

// Three decimals is finer than a printer resolves and keeps the markup from doubling in size on
// spacings that do not divide evenly.
function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}
