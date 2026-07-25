// Resolving a page template into geometry, and drawing it.
//
// The mirror of `crates/goodtype-core/src/template.rs`. The canvas needs this in the webview and
// export needs it in Rust, so it exists twice — and both are asserted against
// `fixtures/templates/resolved.json`, which is what stops the screen and the PDF disagreeing
// about where a line goes.
//
// Steps are computed as `start + index * spacing` rather than accumulated, so the two
// implementations agree bit for bit instead of drifting one rounding error at a time.

import type { PageGeometry, PageTemplate, TemplateElement } from "../model";

/** Matches `MIN_SPACING_PT`: below this a template stops being paper and becomes a fill pattern. */
export const MIN_SPACING_PT = 3;
export const MAX_SHAPES = 20_000;

export type TemplateShape =
  | { kind: "line"; x1: number; y1: number; x2: number; y2: number; color: string; weightPt: number }
  | { kind: "dot"; cx: number; cy: number; radiusPt: number; color: string };

export function resolveTemplate(template: PageTemplate, geometry: PageGeometry): TemplateShape[] {
  const left = template.marginPt;
  const top = template.marginPt;
  const right = geometry.widthPt - template.marginPt;
  const bottom = geometry.heightPt - template.marginPt;
  const shapes: TemplateShape[] = [];
  if (right <= left || bottom <= top) return shapes;

  for (const element of template.elements) {
    switch (element.kind) {
      case "horizontal_lines": {
        const start = top + element.offsetPt;
        for (let index = 0; index < steps(start, bottom, element.spacingPt); index += 1) {
          const y = start + index * element.spacingPt;
          shapes.push({ kind: "line", x1: left, y1: y, x2: right, y2: y, color: element.color, weightPt: element.weightPt });
        }
        break;
      }
      case "vertical_lines": {
        const start = left + element.offsetPt;
        for (let index = 0; index < steps(start, right, element.spacingPt); index += 1) {
          const x = start + index * element.spacingPt;
          shapes.push({ kind: "line", x1: x, y1: top, x2: x, y2: bottom, color: element.color, weightPt: element.weightPt });
        }
        break;
      }
      case "dots": {
        const startY = top + element.offsetPt;
        const startX = left + element.offsetPt;
        const rows = steps(startY, bottom, element.spacingPt);
        const columns = steps(startX, right, element.spacingPt);
        for (let row = 0; row < rows; row += 1) {
          const cy = startY + row * element.spacingPt;
          for (let column = 0; column < columns; column += 1) {
            shapes.push({ kind: "dot", cx: startX + column * element.spacingPt, cy, radiusPt: element.radiusPt, color: element.color });
          }
        }
        break;
      }
      case "rule":
        shapes.push(rule(element, geometry));
        break;
    }
    if (shapes.length >= MAX_SHAPES) {
      shapes.length = MAX_SHAPES;
      break;
    }
  }
  return shapes;
}

// Measured from the page edge rather than the margin: a legal pad's rule is a margin of its own,
// so making it relative to another one reads backwards.
function rule(
  element: Extract<TemplateElement, { kind: "rule" }>,
  geometry: PageGeometry,
): TemplateShape {
  const { offsetPt, color, weightPt } = element;
  switch (element.edge) {
    case "left":
      return { kind: "line", x1: offsetPt, y1: 0, x2: offsetPt, y2: geometry.heightPt, color, weightPt };
    case "right":
      return { kind: "line", x1: geometry.widthPt - offsetPt, y1: 0, x2: geometry.widthPt - offsetPt, y2: geometry.heightPt, color, weightPt };
    case "top":
      return { kind: "line", x1: 0, y1: offsetPt, x2: geometry.widthPt, y2: offsetPt, color, weightPt };
    case "bottom":
      return { kind: "line", x1: 0, y1: geometry.heightPt - offsetPt, x2: geometry.widthPt, y2: geometry.heightPt - offsetPt, color, weightPt };
  }
}

/** How many steps of `spacing` fit from `start` up to and including `end`. */
function steps(start: number, end: number, spacing: number): number {
  if (!Number.isFinite(spacing) || spacing < MIN_SPACING_PT || start > end) return 0;
  return Math.floor((end - start) / spacing) + 1;
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
