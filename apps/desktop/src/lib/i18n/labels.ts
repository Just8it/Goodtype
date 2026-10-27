import { translate, notice, renderMessage, type UiText, type Translator } from ".";
import type { StrokePattern } from "../model";

export function patternLabel(value: StrokePattern, t: Translator = translate): string {
  return t(({ solid: "workspace.solid", dashed: "workspace.dashed", dotted: "workspace.dotted" } as const)[value]).toLowerCase();
}

export function shapeLabel(value: string, t: Translator = translate): string {
  return renderMessage(shapeMessage(value), t);
}

/** Safe to construct during a live hold preview; translation happens only in the view. */
export function shapeMessage(value: string): UiText {
  switch (value) {
    case "line": return notice("workspace.line");
    case "rectangle": return notice("workspace.rectangle");
    case "ellipse": return notice("workspace.ellipse");
    case "spline": return notice("workspace.spline");
    default: return notice("workspace.shape");
  }
}

export function toolLabel(value: string, t: Translator = translate): string {
  return renderMessage(toolMessage(value), t);
}

export function toolMessage(value: string): UiText {
  switch (value) {
    case "pen": return notice("workspace.pen");
    case "highlighter": return notice("workspace.highlighter");
    case "eraser": return notice("workspace.eraser");
    case "lasso": return notice("workspace.lasso");
    case "select": return notice("workspace.inkSelection");
    default: return notice("workspace.shape");
  }
}

export function subjectLabel(value: string, t: Translator = translate): string {
  switch (value) {
    case "ink": return t("terms.ink");
    case "image": return t("terms.image");
    case "shape": return t("terms.shape");
    case "Typst block": return t("terms.typst");
    case "page": return t("terms.page");
    default: return t("terms.items");
  }
}

export function positionMessage(value: string): UiText {
  switch (value.replaceAll(" ", "-")) {
    case "left": return notice("terms.left");
    case "right": return notice("terms.right");
    case "top": return notice("terms.top");
    case "bottom": return notice("terms.bottom");
    case "top-left": return notice("terms.topLeft");
    case "top-right": return notice("terms.topRight");
    case "bottom-left": return notice("terms.bottomLeft");
    case "bottom-right": return notice("terms.bottomRight");
    default: return value;
  }
}
