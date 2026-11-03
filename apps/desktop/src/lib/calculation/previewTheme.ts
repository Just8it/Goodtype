/** Dark surfaces for the export's light fills: the sheet, the raised code box and the error band. */
const SURFACES: Record<string, string> = { ffffff: "16181d", f7f8f9: "1f2329", fbeeed: "3a2224" };
/** Colours whose meaning must survive the theme: plain text, error text and the stale-output note. */
const INKS: Record<string, string> = { "000000": "e9ebee", a43b36: "f2928c", "885717": "e0a458" };

/** Theme only Rust-generated preview colors. Geometry and embedded plot pixels stay unchanged. */
export function calculationPreviewUrl(svg: string): string {
  const themed = svg
    .replace(/(<path\b[^>]*\bfill=")#([a-f\d]{6})(")/gi, (match, before: string, color: string, after: string) => {
      const surface = SURFACES[color.toLowerCase()];
      return surface ? `${before}#${surface}${after}` : match;
    })
    .replace(/(<use\b[^>]*\bfill=")#([a-f\d]{6})(")/gi, (_match, before: string, color: string, after: string) => {
      // Other syntax colours are lifted toward white so they stay legible on the dark sheet.
      const text = INKS[color.toLowerCase()] ?? color.match(/../g)!
        .map(channel => Math.round(Number.parseInt(channel, 16) * .45 + 255 * .55).toString(16).padStart(2, "0")).join("");
      return `${before}#${text}${after}`;
    });
  return `data:image/svg+xml,${encodeURIComponent(themed)}`;
}
