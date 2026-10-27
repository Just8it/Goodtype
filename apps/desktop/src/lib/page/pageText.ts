import { translate, type Translator, formatNumber } from "../i18n";
import type { Area, PageBackground, PageGeometry, TemplateElement } from "../model";
import { resolveTemplate } from "./template";

const DEFAULT_MARGIN_PT = 36;
const DEFAULT_FONT_PT = 11;
const GUIDE_ORIGIN_NUDGE_PT = { x: 2, y: -2 };

export type PageTextLayout = {
  x: number;
  y: number;
  width: number;
  height: number;
  lineSpacingPt: number;
  textColor: string;
  columns: 1 | 2;
  description: string;
};

/** Derive the writing frame from the stored paper geometry, never from a template name. */
export function pageTextLayout(
  background: PageBackground,
  geometry: PageGeometry,
  t: Translator = translate,
): PageTextLayout {
  const fallback = defaultLayout(background, geometry, t);
  if (background.kind !== "template") return fallback;

  const guide = background.template.elements.find(isWritingGuide);
  if (!guide) return fallback;
  const box = areaBounds(guide.area, geometry);
  if (!box) return fallback;

  const spacing = guide.spacingPt < 18 ? guide.spacingPt * 2 : guide.spacingPt;
  const shapes = resolveTemplate(background.template, geometry);
  const rows = [...new Set(shapes.flatMap((shape) => {
    if (shape.kind === "dot") return [shape.cy];
    return shape.y1 === shape.y2 ? [shape.y1] : [];
  }))].sort((left, right) => left - right);
  const columns = [...new Set(shapes.flatMap((shape) => {
    if (shape.kind === "dot") return [shape.cx];
    return shape.x1 === shape.x2 ? [shape.x1] : [];
  }))].sort((left, right) => left - right);
  const firstBaseline = rows.find((row) =>
    row >= box.top + (guide.kind === "horizontal_lines" ? 0 : guide.spacingPt)
  ) ?? box.top + spacing;
  const firstColumn = columns.find((column) => column >= box.left) ?? box.left;
  const x = (guide.kind === "horizontal_lines" ? box.left : firstColumn)
    + GUIDE_ORIGIN_NUDGE_PT.x;
  const y = Math.max(0, firstBaseline - DEFAULT_FONT_PT + GUIDE_ORIGIN_NUDGE_PT.y);
  const right = box.right - guide.spacingPt;

  return {
    x,
    y,
    width: Math.max(72, right - x),
    height: Math.max(1, box.bottom - y),
    lineSpacingPt: spacing,
    textColor: readableText(background.template.backgroundColor),
    columns: background.template.elements.some(
      (element) => element.kind === "rule" && element.edge === "center_x",
    ) ? 2 : 1,
    description: t("page.alignedToMmPaperRhythm", { "value1": millimetres(spacing) }),
  };
}

function defaultLayout(background: PageBackground, geometry: PageGeometry, t: Translator): PageTextLayout {
  const color = background.kind === "plain"
    ? background.color
    : background.kind === "template"
      ? background.template.backgroundColor
      : "#ffffff";
  return {
    x: DEFAULT_MARGIN_PT,
    y: DEFAULT_MARGIN_PT,
    width: Math.max(72, geometry.widthPt - 2 * DEFAULT_MARGIN_PT),
    height: Math.max(1, geometry.heightPt - 2 * DEFAULT_MARGIN_PT),
    lineSpacingPt: 16,
    textColor: readableText(color),
    columns: 1,
    description: t("page.standardPageMargins"),
  };
}

function isWritingGuide(
  element: TemplateElement,
): element is Extract<TemplateElement, { kind: "horizontal_lines" | "grid" | "dots" }> {
  return element.kind === "horizontal_lines" || element.kind === "grid" || element.kind === "dots";
}

function areaBounds(area: Area, geometry: PageGeometry) {
  const box = {
    left: area.leftPt,
    top: area.topPt,
    right: geometry.widthPt - area.rightPt,
    bottom: geometry.heightPt - area.bottomPt,
  };
  return box.right > box.left && box.bottom > box.top ? box : null;
}

function readableText(color: string): string {
  const match = /^#([0-9a-f]{6})$/i.exec(color);
  if (!match) return "#16212b";
  const value = Number.parseInt(match[1], 16);
  const luminance = ((value >> 16) * 299 + ((value >> 8) & 255) * 587 + (value & 255) * 114) / 1000;
  return luminance < 110 ? "#eef1f4" : "#16212b";
}

function millimetres(points: number): string {
  return formatNumber(points * 25.4 / 72, { maximumFractionDigits: 1 });
}
