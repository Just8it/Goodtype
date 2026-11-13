<script lang="ts">
import PdfBackground from "./PdfBackground.svelte";
import type {PageObject} from "../model";
let {objects,root,zoom=1}:{objects:PageObject[];root:string;zoom?:number}=$props();
</script>
{#each objects as object (object.id)}{#if object.type === "pdf_material"}
<div class="material" data-object-id={object.id} style:left={`${object.x}px`} style:top={`${object.y}px`} style:width={`${object.sourceWidthPt}px`} style:height={`${object.sourceHeightPt}px`} style:z-index={object.zIndex} style:transform={`rotate(${object.rotation}deg) scale(${object.scale})`}>
<PdfBackground {root} sourcePath={object.sourcePath} page={object.page + 1} widthPt={object.sourceWidthPt} {zoom}/>
</div>
{/if}{/each}
<style>.material{position:absolute;transform-origin:top left;pointer-events:none}</style>
