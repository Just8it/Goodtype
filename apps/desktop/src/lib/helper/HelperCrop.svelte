<script lang="ts">
  import { viewportToPage } from "../geometry/coordinates";
  import { resizeCrop } from "./crop";
  import { t } from "../i18n";
  import type { CropPt } from "../ipc/helper";
  let {crop, page, zoom, editable = true, onCommit}: {crop:CropPt;page:{widthPt:number;heightPt:number};zoom:number;editable?:boolean;onCommit:(crop:CropPt)=>void} = $props();
  let preview = $state<CropPt | null>(null);
  let drag: {id:number;x:number;y:number;corner:number;base:CropPt} | null = null;
  const shown = $derived(preview ?? crop);
  function begin(e:PointerEvent,corner:number) {
    if(e.button!==0) return;
    e.preventDefault();e.stopPropagation();(e.currentTarget as SVGElement).setPointerCapture(e.pointerId);
    drag={id:e.pointerId,x:e.clientX,y:e.clientY,corner,base:{...crop}};
  }
  function move(e:PointerEvent) {
    if(!drag || e.pointerId!==drag.id) return;
    e.stopPropagation();
    const d=viewportToPage({x:e.clientX-drag.x,y:e.clientY-drag.y},{pageOriginX:0,pageOriginY:0,zoom});
    preview=resizeCrop(drag.base,drag.corner,d,page);
  }
  function end(e:PointerEvent) {
    if(!drag || e.pointerId!==drag.id)return;
    e.stopPropagation();
    if(e.type==="pointerup"){move(e);if(preview)onCommit(preview);}
    drag=null;preview=null;
  }
</script>
<svg class="helper-crop" style:--zoom={zoom} aria-label={$t("codex.resizeCrop")}>
  <rect x={shown.x} y={shown.y} width={shown.width} height={shown.height}/>
  {#if editable}{#each [0,1,2,3] as corner}
    <circle class="hit" role="button" tabindex="0" aria-label={$t((["codex.cropTopLeft", "codex.cropTopRight", "codex.cropBottomLeft", "codex.cropBottomRight"] as const)[corner])} cx={shown.x+(corner%2?shown.width:0)} cy={shown.y+(corner<2?0:shown.height)} r={22/zoom}
      onkeydown={e=>{if(e.key.startsWith("Arrow")){e.preventDefault();const step=e.shiftKey?10:1;const next=resizeCrop(crop,corner,{x:e.key==="ArrowLeft"?-step:e.key==="ArrowRight"?step:0,y:e.key==="ArrowUp"?-step:e.key==="ArrowDown"?step:0},page);if(next)onCommit(next);}}}
      onpointerdown={e=>begin(e,corner)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end}/>
    <circle class="visible" cx={shown.x+(corner%2?shown.width:0)} cy={shown.y+(corner<2?0:shown.height)} r={5/zoom}/>
  {/each}{/if}
</svg>
<style>
.helper-crop{position:absolute;inset:0;width:100%;height:100%;overflow:visible;z-index:2147483647;pointer-events:none}
rect{fill:none;stroke:var(--paper-select);stroke-width:calc(1.5px / var(--zoom));stroke-dasharray:calc(4px / var(--zoom)) calc(3px / var(--zoom))}
circle.hit{fill:transparent;pointer-events:all;touch-action:none;cursor:nwse-resize}
.visible{fill:var(--paper);stroke:var(--paper-select);stroke-width:calc(1.5px / var(--zoom));pointer-events:none}
.hit:focus-visible{outline:2px solid var(--paper-select);outline-offset:2px}
.hit:hover + .visible,.hit:focus-visible + .visible{fill:var(--paper-select)}
.hit:nth-of-type(3),.hit:nth-of-type(5){cursor:nesw-resize}
</style>
