<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import type { InkLayer, NotebookManifest, Page, PageObject } from "../model";
  import type { TypstCompileResult } from "../editor/typst";
  import InkSurface from "./InkSurface.svelte";
  import TypstBlock from "./TypstBlock.svelte";

  type StoredFile = { path: string; bytes: number[] };
  type Snapshot = {
    manifest: NotebookManifest;
    page: Page;
    blocks: StoredFile[];
    assets: StoredFile[];
    inkLayers: InkLayer[];
  };

  let {
    snapshot,
    root,
    zoom,
  }: {
    snapshot: Snapshot;
    root: string;
    zoom: number;
  } = $props();

  let results = $state<Record<string, TypstCompileResult>>({});
  const imageUrls = new Map<string, string>();

  onDestroy(() => {
    for (const url of imageUrls.values()) URL.revokeObjectURL(url);
  });

  function sourceFor(object: Extract<PageObject, { type: "typst" }>) {
    const file = snapshot.blocks.find((candidate) => candidate.path === object.sourcePath);
    return file ? new TextDecoder().decode(new Uint8Array(file.bytes)) : "";
  }

  function imageUrl(path: string) {
    const current = imageUrls.get(path);
    if (current) return current;
    const file = snapshot.assets.find((candidate) => candidate.path === path);
    if (!file) return "";
    const url = URL.createObjectURL(new Blob([new Uint8Array(file.bytes)]));
    imageUrls.set(path, url);
    return url;
  }

  async function compile(
    id: string,
    request: { source: string; widthPt: number; generation: number },
  ) {
    try {
      results[id] = await invoke<TypstCompileResult>("compile_typst", { root, request });
    } catch {
      // The page remains readable through ink and images if a background preview fails.
    }
  }
</script>

<div class="objects">
  {#each snapshot.page.objects as object (object.id)}
    {#if object.type === "typst"}
      <TypstBlock
        id={object.id}
        source={sourceFor(object)}
        initialX={object.x}
        initialY={object.y}
        initialLayoutWidthPt={object.layoutWidthPt}
        initialScale={object.scale}
        compileResult={results[object.id] ?? null}
        toPageDelta={(x, y) => ({ x: x / zoom, y: y / zoom })}
        onCompile={(request) => compile(object.id, request)}
        onSourceChange={() => {}}
        onTransform={() => {}}
      />
    {:else if object.type === "image"}
      <img
        class="image"
        src={imageUrl(object.sourcePath)}
        alt={object.altText}
        style:left={`${object.x}px`}
        style:top={`${object.y}px`}
        style:width={`${object.widthPt}px`}
        style:height={`${object.heightPt}px`}
        style:transform={`scale(${object.scale})`}
      />
    {/if}
  {/each}
</div>
<div class="ink">
  <InkSurface
    strokes={snapshot.inkLayers.flatMap((layer) => layer.strokes)}
    pageWidthPt={snapshot.page.geometry.widthPt}
    pageHeightPt={snapshot.page.geometry.heightPt}
    {zoom}
  />
</div>

<style>
  .objects,
  .ink {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .objects {
    z-index: 1;
  }

  .ink {
    z-index: 2;
  }

  .image {
    position: absolute;
    object-fit: fill;
    transform-origin: top left;
  }
</style>
