<script lang="ts">
  import { t } from "../i18n";
  import { onDestroy, onMount } from "svelte";
  import {
    applyTypstCompileResult,
    emptyTypstPreview,
    requestTypstCompile,
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import type { PageBackground, PageGeometry } from "../model";
  import { pageTextLayout } from "../page/pageText";
  import type { TypstCompileRequest } from "../ipc/typst";

  let {
    source,
    compileResult = null,
    compileContext = "",
    background,
    geometry,
    snapBlocksToGrid = true,
    compileDependency = 0,
    readingOrder = 0,
    onCompile,
  }: {
    source: string;
    compileResult?: TypstCompileResult | null;
    compileContext?: string;
    background: PageBackground;
    geometry: PageGeometry;
    snapBlocksToGrid?: boolean;
    compileDependency?: number;
    readingOrder?: number;
    onCompile: (request: TypstCompileRequest) => void;
  } = $props();

  let preview = $state(emptyTypstPreview());
  let svgUrl = $state<string | null>(null);
  let compileTimer: ReturnType<typeof setTimeout> | undefined;
  let mounted = $state(false);
  let appliedSource = $state("");
  let appliedContext = $state("");
  let appliedLayoutKey = $state("");
  let appliedDependency = $state(0);
  const layout = $derived(pageTextLayout(background, geometry, $t));
  const layoutKey = $derived(`${layout.x}:${layout.y}:${layout.width}:${layout.lineSpacingPt}:${layout.columns}:${layout.textColor}:${snapBlocksToGrid}`);
  const svgWidth = $derived((preview.widthPt ?? layout.width) + 2 * preview.padPt);
  const overflow = $derived((preview.heightPt ?? 0) > layout.height);

  onMount(() => {
    mounted = true;
    appliedSource = source;
    appliedContext = compileContext;
    appliedLayoutKey = layoutKey;
    appliedDependency = compileDependency;
    requestCompile(0);
  });

  $effect(() => {
    if (!compileResult) return;
    const next = applyTypstCompileResult(preview, compileResult);
    if (next === preview) return;
    if (next.svg !== preview.svg && next.svg) {
      const url = URL.createObjectURL(new Blob([next.svg], { type: "image/svg+xml" }));
      if (svgUrl) URL.revokeObjectURL(svgUrl);
      svgUrl = url;
    }
    preview = next;
  });

  $effect(() => {
    if (!mounted) return;
    if (source === appliedSource && compileContext === appliedContext && layoutKey === appliedLayoutKey && compileDependency === appliedDependency) return;
    appliedSource = source;
    appliedContext = compileContext;
    appliedLayoutKey = layoutKey;
    appliedDependency = compileDependency;
    requestCompile();
  });

  onDestroy(() => {
    if (compileTimer) clearTimeout(compileTimer);
    if (svgUrl) URL.revokeObjectURL(svgUrl);
  });

  function requestCompile(delay = TYPST_IDLE_DEBOUNCE_MS) {
    if (compileTimer) clearTimeout(compileTimer);
    compileTimer = setTimeout(() => {
      preview = requestTypstCompile(preview);
      onCompile({
        source,
        sharedStyle: compileContext,
        pageText: { background, geometry, snapBlocksToGrid },
        widthPt: layout.width,
        generation: preview.requestedGeneration,
      });
    }, delay);
  }
</script>

<section
  class="page-text"
  style:left={`${layout.x}px`}
  style:top={`${layout.y}px`}
  style:width={`${layout.width}px`}
  style:height={`${layout.height}px`}
  aria-label={$t("workspace.pageTextDescription", { position: readingOrder + 1, description: layout.description })}
>
  {#if svgUrl}
    <img
      src={svgUrl}
      alt={$t("workspace.renderedPageText")}
      draggable="false"
      style:width={`${svgWidth}px`}
      style:margin={`${-preview.padPt}px`}
    />
  {/if}
  {#if overflow}
    <span class="overflow" role="status">{$t("workspace.pageTextContinuesPastTheWritingArea")}</span>
  {/if}
</section>

<style>
  .page-text {
    position: absolute;
    z-index: 0;
    color: #16212b;
    overflow: visible;
    pointer-events: none;
  }
  img { display: block; max-width: none; pointer-events: none; user-select: none; }
  .overflow {
    position: absolute;
    bottom: 0;
    right: 0;
    padding: 3px 6px;
    border-radius: var(--radius);
    background: rgb(116 35 31 / 88%);
    color: #fff;
    font-size: var(--text-xs);
  }
</style>
