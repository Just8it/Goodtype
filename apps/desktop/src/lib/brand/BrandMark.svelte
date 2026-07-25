<script lang="ts">
  /**
   * The Goodtype mark: a page with one handwritten stroke above two lines of set type.
   *
   * Detail drops as the mark shrinks, matching `brand/icon-*.svg` and the icons the desktop build
   * ships. Below about 40px the second, lighter line stops reading as type and starts reading as
   * a smudge; below about 24px the page outline goes the same way and only the gesture survives.
   * Rendering the full artwork at every size would technically "work" and look worse.
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
  const pageWidth = $derived(detail === "full" ? 6 : 7);

  const SIGNATURE =
    "M18 30 C 24 18, 30 18, 34 28 C 37 35, 41 35, 45 26 C 48 19, 53 19, 57 24 C 60 28, 64 28, 67 23";
</script>

<svg
  width={(size * 104) / 70}
  height={size}
  viewBox="0 0 104 70"
  fill="none"
  role={title ? "img" : "presentation"}
  aria-label={title || undefined}
  aria-hidden={title ? undefined : "true"}
>
  {#if detail !== "minimal"}
    <rect
      x="6"
      y="6"
      width="92"
      height="58"
      rx="7"
      stroke={palette.ink}
      stroke-width={pageWidth}
    />
  {/if}

  <path
    d={SIGNATURE}
    transform={detail === "minimal"
      ? "translate(52 35) scale(1.68) translate(-42.5 -26.5)"
      : undefined}
    stroke={palette.hand}
    stroke-width={detail === "minimal" ? 10 : detail === "medium" ? 5.95 : 5.5}
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  {#if detail === "full"}
    <rect x="18" y="40" width="52" height="5.5" rx="2.75" fill={palette.ink} />
    <rect x="18" y="48" width="38" height="5.5" rx="2.75" fill={palette.set} />
  {:else if detail === "medium"}
    <rect x="18" y="41" width="52" height="6.5" rx="3.25" fill={palette.ink} />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
