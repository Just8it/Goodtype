/**
 * Dash patterns for the small preview lines in the palette and editors.
 *
 * A plain `stroke-dasharray` starts at the left end and stops wherever the line runs out, so one
 * end showed a clipped half-dash and the marks sat off-centre. These previews instead fit a whole
 * number of marks and share the leftover length equally between both ends. Canonical ink and
 * shapes keep their origin-anchored spacing; this is only for previews.
 */
export type CentredDash = { dasharray: string; dashoffset: number };

export function centredDashArray(length: number, dash: number, gap: number): CentredDash | null {
  const period = dash + gap;
  if (!(length > 0) || !(period > 0)) return null;
  const count = Math.max(1, Math.floor((length + gap) / period));
  const used = count * dash + (count - 1) * gap;
  const lead = Math.max(0, (length - used) / 2);
  const marks: number[] = [];
  for (let index = 0; index < count; index += 1) {
    marks.push(dash, index < count - 1 ? gap : length);
  }
  // The margin is an offset, not a leading zero-length dash: with round caps a zero-length dash
  // is itself drawn as a dot, which put a stray mark at the start of every line.
  return {
    dasharray: marks.map(tidy).join(" "),
    dashoffset: tidy(-lead),
  };
}

function tidy(value: number): number {
  return Math.round(value * 1000) / 1000 || 0;
}

/**
 * Applies a centred pattern to a path once its length is known. `pattern` is `[dash, gap]` in the
 * path's own units, or null for a solid line.
 */
export function centredDash(node: SVGGeometryElement, pattern: readonly number[] | null) {
  function apply(next: readonly number[] | null) {
    const centred = next && centredDashArray(node.getTotalLength(), next[0], next[1]);
    if (!centred) {
      node.removeAttribute("stroke-dasharray");
      node.removeAttribute("stroke-dashoffset");
      return;
    }
    node.setAttribute("stroke-dasharray", centred.dasharray);
    node.setAttribute("stroke-dashoffset", String(centred.dashoffset));
  }
  apply(pattern);
  return { update: apply };
}
