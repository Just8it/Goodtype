<script lang="ts">
  /**
   * The Goodtype mark: a page standing on a base, with one handwritten stroke above two lines of
   * set type.
   *
   * The base was dropped once for reading like a keyboard beneath a screen, which made the mark
   * look like a PC rather than a notebook. It is back by decision: it is what the page stands on,
   * it gives the silhouette a foot to sit on, and without it the page floats.
   *
   * Detail drops as the mark shrinks, matching `brand/icon-*.svg` and the icons the desktop build
   * ships. Below about 40px the second, lighter line stops reading as type and starts reading as
   * a smudge; below about 24px the first line goes the same way. The base stays at every size —
   * it is filled rather than stroked, so it is the one part that does not thin as it scales.
   *
   * Teal is only ever the handwritten stroke and ink-grey is only ever set type — the same rule
   * the product follows, where teal means "your hand" wherever it appears.
   */
  let {
    size = 40,
    tone = "dark",
    title = "Goodtype",
  }: {
    /** Rendered height in pixels. Width follows the mark's aspect ratio. */
    size?: number;
    /** Which background the mark sits on. */
    tone?: "dark" | "light";
    /** Accessible name, or empty to hide it from assistive tech when a text label sits beside it. */
    title?: string;
  } = $props();

  const palette = $derived(
    tone === "light"
      ? { ink: "#16222E", hand: "#1497A6", set: "#9AA3AE" }
      : { ink: "#E9EBEE", hand: "#38B6C6", set: "#8A929C" },
  );

  const detail = $derived(size >= 40 ? "full" : size >= 24 ? "medium" : "minimal");
  // Thin strokes disappear into the pixel grid faster than thick ones, so the page edge is
  // weighted up as the mark shrinks rather than left to fade.
  const pageWidth = $derived(detail === "full" ? 6 : detail === "medium" ? 7 : 8);

  const SIGNATURE =
    "M26 34 C 32 22, 38 22, 42 32 C 45 39, 49 39, 53 30 C 56 23, 61 23, 65 28 C 68 32, 72 32, 75 27";
</script>

<svg
  width={(size * 120) / 80}
  height={size}
  viewBox="0 0 120 80"
  fill="none"
  role={title ? "img" : "presentation"}
  aria-label={title || undefined}
  aria-hidden={title ? undefined : "true"}
>
  <rect
    x="14"
    y="4"
    width="92"
    height="58"
    rx="7"
    stroke={palette.ink}
    stroke-width={pageWidth}
  />

  <rect x="4" y="66" width="112" height="10" rx="5" fill={palette.ink} />

  <path
    d={SIGNATURE}
    transform={detail === "minimal"
      ? "translate(60 33) scale(1.28) translate(-50.5 -30.5)"
      : undefined}
    stroke={palette.hand}
    stroke-width={detail === "minimal" ? 8 : detail === "medium" ? 6.4 : 5.5}
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  {#if detail === "full"}
    <rect x="26" y="44" width="52" height="5.5" rx="2.75" fill={palette.ink} />
    <rect x="26" y="52" width="38" height="5.5" rx="2.75" fill={palette.set} />
  {:else if detail === "medium"}
    <rect x="26" y="45" width="52" height="6.5" rx="3.25" fill={palette.ink} />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
