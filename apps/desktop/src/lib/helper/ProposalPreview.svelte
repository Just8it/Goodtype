<script lang="ts">
  import { pageToViewport } from "../geometry/coordinates";
  import { proposalPreview } from "./state";

  let { root, pageId, width, height, zoom }: {
    root: string; pageId: string; width: number; height: number; zoom: number;
  } = $props();
  const view = $derived({ pageOriginX: 0, pageOriginY: 0, zoom });
  const size = $derived(pageToViewport({ x: width, y: height }, view));
</script>

{#if $proposalPreview?.root === root && $proposalPreview.pageId === pageId && $proposalPreview.kind !== "preset_install" && !$proposalPreview.edit}
  <svg class="preview" viewBox={`0 0 ${size.x} ${size.y}`} aria-hidden="true" style="pointer-events: none">
    {#each $proposalPreview.items as item (item.object.id)}
      {@const position = pageToViewport(item.object, view)}
      {@const bounds = pageToViewport({ x: item.widthPt, y: item.heightPt }, view)}
      <rect x={position.x} y={position.y} width={bounds.x} height={bounds.y} fill="white" />
      <image x={position.x} y={position.y} width={bounds.x} height={bounds.y}
        href={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(item.svg)}`} />
    {/each}
  </svg>
{/if}

<style>
  .preview {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 24;
  }
</style>
