<script lang="ts">
  import { onDestroy } from "svelte";
  import PageSurface from "../notebook/PageSurface.svelte";
  import { AssetUrlCache, blockViewsFromSnapshot, imageViewsFromSnapshot, pageTypstViewFromSnapshot, shapeViewsFromSnapshot } from "../notebook/pageView";
  import { createNotebookPreviews } from "../editor/notebookPreviews";
  import type { NotebookSnapshot } from "../ipc/types";
  import type { TypstCompileRequest } from "../ipc/typst";
  import type { TypstCompileResult } from "../editor/typst";
  let { snapshot, root, selection }: { snapshot: NotebookSnapshot; root: string; selection?: import("../ipc/helper").SharePayload } = $props();
  const view = $derived.by(() => {
    if (!selection) return snapshot;
    const ids = new Set(selection.objectIds);
    for (let changed = true; changed;) {
      changed = false;
      for (const object of snapshot.page.objects) if (ids.has(object.id) && object.type === "group") for (const id of object.childIds) if (!ids.has(id)) { ids.add(id); changed = true; }
    }
    const region = !ids.size && !selection.strokeIds.length;
    return { ...snapshot, page: { ...snapshot.page, objects: snapshot.page.objects.filter(o => ids.has(o.id) || region && o.type === "pdf_material") }, inkLayers: snapshot.inkLayers.map(layer => ({ ...layer, strokes: layer.strokes.filter(s => selection!.strokeIds.includes(s.id)) })) };
  });
  const previews = createNotebookPreviews("__TAURI_INTERNALS__" in window);
  let results = $state<Record<string, TypstCompileResult>>({});
  let alive = true;
  const urls = new AssetUrlCache();
  async function compile(id: string, request: TypstCompileRequest) {
    const result = await previews.compile({ root, pageId: snapshot.page.id, blockId: id, current: () => alive }, request, true);
    if (result && alive) results = { ...results, [id]: result };
  }
  onDestroy(() => { alive = false; previews.dispose(); urls.dispose(); });
</script>
<svg viewBox={selection ? `${selection.cropPt.x} ${selection.cropPt.y} ${selection.cropPt.width} ${selection.cropPt.height}` : `0 0 ${snapshot.page.geometry.widthPt} ${snapshot.page.geometry.heightPt}`} aria-hidden="true">
  <foreignObject width={snapshot.page.geometry.widthPt} height={snapshot.page.geometry.heightPt}>
    <div class="paper" style:width={`${snapshot.page.geometry.widthPt}px`} style:height={`${snapshot.page.geometry.heightPt}px`}><PageSurface materials={view.page.objects} {root} background={view.page.background} pageWidthPt={view.page.geometry.widthPt} pageHeightPt={view.page.geometry.heightPt}
      blocks={blockViewsFromSnapshot(view)} pageTypst={pageTypstViewFromSnapshot(view)} images={imageViewsFromSnapshot(view, urls)}
      shapes={shapeViewsFromSnapshot(view)} strokes={view.inkLayers.flatMap(layer => layer.strokes)} {results} onCompile={compile} showSelectionBounds={false} /></div>
  </foreignObject>
</svg>
<style>
  svg { display: block; width: 100%; max-height: 55dvh; background: var(--paper); }
  .paper { position: relative; isolation: isolate; pointer-events: none; }
</style>
