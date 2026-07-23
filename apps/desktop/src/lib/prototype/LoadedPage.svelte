<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import type { InkLayer, NotebookManifest, Page, PageObject, Stroke } from "../model";
  import type { TypstCompileResult } from "../editor/typst";
  import type { InkTool } from "../ink/pipeline";
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
  type HistoryResult = {
    snapshot: Snapshot;
    canUndo: boolean;
    canRedo: boolean;
  };

  let {
    snapshot,
    root,
    zoom,
    tool,
    color,
    widthPt,
    onCommitted,
    onStatus,
  }: {
    snapshot: Snapshot;
    root: string;
    zoom: number;
    tool: InkTool;
    color: string;
    widthPt: number;
    onCommitted?: (snapshot: Snapshot) => void;
    onStatus?: (status: string) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let current = $state(snapshot);
  // svelte-ignore state_referenced_locally
  let strokes = $state<Stroke[]>(snapshot.inkLayers.flatMap((layer) => layer.strokes));
  let selectedStrokeIds = $state<string[]>([]);
  let results = $state<Record<string, TypstCompileResult>>({});
  const imageUrls = new Map<string, string>();
  let commitQueue: Promise<void> = Promise.resolve();

  $effect(() => {
    if (snapshot.page.revision === current.page.revision) return;
    current = snapshot;
    strokes = snapshot.inkLayers.flatMap((layer) => layer.strokes);
    selectedStrokeIds = [];
  });

  onDestroy(() => {
    for (const url of imageUrls.values()) URL.revokeObjectURL(url);
  });

  function sourceFor(object: Extract<PageObject, { type: "typst" }>) {
    const file = current.blocks.find((candidate) => candidate.path === object.sourcePath);
    return file ? new TextDecoder().decode(new Uint8Array(file.bytes)) : "";
  }

  function imageUrl(path: string) {
    const existingUrl = imageUrls.get(path);
    if (existingUrl) return existingUrl;
    const file = current.assets.find((candidate) => candidate.path === path);
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

  function commitInk(next: Stroke[], label: string) {
    strokes = next;
    const committedStrokes = next;
    commitQueue = commitQueue.then(async () => {
      const inkLayers = current.inkLayers.map((layer, index) =>
        index === 0 ? { ...layer, strokes: committedStrokes } : layer,
      );
      try {
        const result = await invoke<HistoryResult>("commit_notebook", {
          root,
          snapshot: { ...current, inkLayers },
        });
        current = result.snapshot;
        strokes = current.inkLayers.flatMap((layer) => layer.strokes);
        onCommitted?.(current);
        onStatus?.(`${label} on page ${pageNumber()}`);
      } catch (error) {
        strokes = current.inkLayers.flatMap((layer) => layer.strokes);
        onStatus?.(`Could not save page ${pageNumber()}: ${String(error)}`);
      }
    });
  }

  function pageNumber() {
    const index = current.manifest.pages.findIndex((page) => page.id === current.page.id);
    return Math.max(0, index) + 1;
  }
</script>

<div class="objects">
  {#each current.page.objects as object (object.id)}
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
    {strokes}
    {selectedStrokeIds}
    pageWidthPt={current.page.geometry.widthPt}
    pageHeightPt={current.page.geometry.heightPt}
    {zoom}
    {tool}
    {color}
    {widthPt}
    onStrokeFinalized={(stroke) => commitInk([...strokes, stroke], "Added ink")}
    onStrokesChange={(next) => commitInk(next, "Updated ink")}
    onSelectionChange={(next) => (selectedStrokeIds = next)}
  />
</div>

<style>
  .objects,
  .ink {
    position: absolute;
    inset: 0;
  }

  .objects {
    z-index: 1;
    pointer-events: none;
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
