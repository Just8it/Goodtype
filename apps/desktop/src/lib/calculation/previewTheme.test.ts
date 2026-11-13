import { expect, it } from "vitest";
import { calculationPreviewUrl } from "./previewTheme";

it("themes preview text and backgrounds without changing geometry or embedded images", () => {
  const svg = '<svg viewBox="0 0 100 50"><path fill="#ffffff" d="M0 0h100v50Z"/><use fill="#000000" href="#glyph"/><use fill="#b60157" href="#number"/><image href="data:image/png;base64,UNCHANGED"/></svg>';
  const themed = decodeURIComponent(calculationPreviewUrl(svg).split(",")[1]);
  expect(themed).toContain('fill="#16181d"');
  expect(themed).toContain('fill="#e9ebee"');
  expect(themed).toContain('viewBox="0 0 100 50"');
  expect(themed).toContain('d="M0 0h100v50Z"');
  expect(themed).toContain('href="data:image/png;base64,UNCHANGED"');
  expect(svg).toContain('fill="#ffffff"');
});

it("keeps code and errors distinct from the sheet", () => {
  const svg = '<svg><path fill="#f7f8f9" d="M0 0"/><path fill="#fbeeed" d="M1 1"/><use fill="#a43b36" href="#e"/></svg>';
  const themed = decodeURIComponent(calculationPreviewUrl(svg).split(",")[1]);
  expect(themed).toContain('fill="#1f2329"');
  expect(themed).toContain('fill="#3a2224"');
  expect(themed).toContain('fill="#f2928c"');
  expect(themed).not.toContain('fill="#16181d"');
});
