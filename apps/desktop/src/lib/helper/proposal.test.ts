import { it, expect, vi } from "vitest";
import { render } from "svelte/server";
import ProposalCard from "./ProposalCard.svelte";
import { acceptProposal, applyBlocked, compileDiagnostics, lineDiff, validateProposalPlacement } from "./proposal";
import { sendProposalRevision } from "./proposal";
import type { Proposal } from "../ipc/helper";
vi.mock("@tauri-apps/api/core",()=>({invoke:vi.fn()}));
import { invoke } from "@tauri-apps/api/core";
import { helperDismissProposal, helperRequestChange, helperEditProposal, helperInsertProposal } from "../ipc/helper";
const proposal:Proposal={anchor:{x:0,y:0,width:100,height:20},id:"p",sessionId:"s",root:"book",pageId:"page",version:2,status:"pending",text:null,changed:[],summary:"Fix the equation",assumptions:["Mass is positive"],edit:true,items:[{object:{type:"typst",id:"block",sourcePath:"blocks/a.typ",x:0,y:0,rotation:0,scale:1,zIndex:1,readingOrder:1,groupId:null,createdAt:"now",modifiedAt:"now",layoutWidthPt:100,measuredWidthPt:100,measuredHeightPt:20},source:"$ F = m a $",beforeSource:"broken",svg:"<svg/>",beforeSvg:null,hash:"hash",widthPt:100,heightPt:20,placement:"below_selection"}]};
it("failed revision rejects without finalizing the usable proposal; success sends exactly once", async () => {
  const original = structuredClone(proposal);
  const send = vi.fn(async () => false);
  await expect(sendProposalRevision(proposal, "Keep the equation", send)).rejects.toThrow("revision_failed");
  expect(proposal).toEqual(original);
  send.mockResolvedValue(true);
  await sendProposalRevision(proposal, "Keep the equation", send);
  expect(send).toHaveBeenLastCalledWith("Revise proposal p as requested: Keep the equation");
  expect(send).toHaveBeenCalledTimes(2);
});
it("new page review exposes the model location, editable source and explicit creation without run", () => {
 const value:Proposal={...proposal,edit:false,kind:"page_create",items:[],newPage:{pageType:"python",title:"Worked solution",position:{kind:"after",pageId:"p2"},source:"",cells:[{cellType:"markdown",source:"## Explanation"},{cellType:"code",source:"print(42)"}],svg:null}};
 const props={proposal:value,pages:[{id:"p1",number:1,title:"Lecture"},{id:"p2",number:2,title:"Exercises"}],onInsert:async()=>{},onClose(){},onUpdate(){}};
 const html=render(ProposalCard,{props}).body;
 expect(html).toContain('aria-label="Page placement"');expect(html).toContain("Exercises");expect(html).toContain("Create page");expect(html).toContain("print(42)");expect(html).toContain("will not run");expect(html).not.toContain("Insert and run");
 const appended=render(ProposalCard,{props:{...props,proposal:{...value,kind:"cells_append",newPage:{...value.newPage!,targetPageId:"p2"}}}}).body;
 expect(appended).toContain("Add cells to Page 2: Exercises");expect(appended).not.toContain('aria-label="Page placement"');expect(appended).not.toContain("Create page");
});
it("new page application can start while viewing another page but refuses a notebook change during flush",async()=>{
 const value={...proposal,newPage:{pageType:"blank" as const,title:"New",position:{kind:"last" as const},source:"",cells:[],svg:null}};
 const submit=vi.fn(async()=>{});let root="book";
 await acceptProposal(value,{current:()=>({root,pageId:"other"}),persist:async()=>true,submit});expect(submit).toHaveBeenCalledTimes(1);
 await expect(acceptProposal(value,{current:()=>({root,pageId:"other"}),persist:async()=>{root="foreign";return true},submit})).rejects.toThrow("wrong_page");expect(submit).toHaveBeenCalledTimes(1);
});
it("review leads with render, exposes actions and does not autofocus source",()=>{
 const html=render(ProposalCard,{props:{proposal,onInsert:async()=>{},onClose(){},onUpdate(){}}}).body;
 expect(html.indexOf("<img")).toBeLessThan(html.indexOf("Fix the equation"));
 for(const label of ["Apply change","Change","Dismiss","Mass is positive","Before","After"])expect(html).toContain(label);
 expect(html).not.toContain("autofocus");
});
it("stale offers regenerate and dismiss with changed dependencies",()=>{
 const html=render(ProposalCard,{props:{proposal:{...proposal,status:"stale",changed:[{dependency:"object",id:"block",change:"modified"}]},onInsert:async()=>{},onClose(){},onUpdate(){}}}).body;
 expect(html).toContain("Regenerate");expect(html).toContain("Dismiss");expect(html).toContain("block");
});
it("review actions carry exact identity, version and source",async()=>{
 await helperDismissProposal("p");expect(invoke).toHaveBeenLastCalledWith("helper_dismiss_proposal",{id:"p"});
 await helperRequestChange("p","regenerate");expect(invoke).toHaveBeenLastCalledWith("helper_request_change",{id:"p",text:"regenerate"});
 await helperEditProposal("p",2,["exact"]);expect(invoke).toHaveBeenLastCalledWith("helper_edit_proposal",{id:"p",proposalVersion:2,sources:["exact"]});
});
it("acceptance flushes then submits exactly one candidate",async()=>{
 const submit=vi.fn(async()=>{}),persist=vi.fn(async()=>true);
 await acceptProposal(proposal,{current:()=>({root:"book",pageId:"page"}),persist,submit});
 expect(persist).toHaveBeenCalledTimes(1);expect(submit).toHaveBeenCalledExactlyOnceWith(proposal);
 expect(persist.mock.invocationCallOrder[0]).toBeLessThan(submit.mock.invocationCallOrder[0]);
});
it("page switched during flush refuses before submission",async()=>{
 let pageId="page";const submit=vi.fn();
 await expect(acceptProposal(proposal,{current:()=>({root:"book",pageId}),persist:async()=>{pageId="other";return true;},submit})).rejects.toThrow("wrong_page");
 expect(submit).not.toHaveBeenCalled();
});
it("insert names only the reviewed proposal version; Rust builds the candidate",async()=>{
 await helperInsertProposal("book","page","p",2);
 expect(invoke).toHaveBeenLastCalledWith("insert_helper_proposal",{root:"book",pageId:"page",proposal:{proposalId:"p",proposalVersion:2}});
});
it("stale refusal propagates without a retry",async()=>{
 const submit=vi.fn(async()=>{throw {code:"proposal_stale"};});
 await expect(acceptProposal(proposal,{current:()=>({root:"book",pageId:"page"}),persist:async()=>true,submit})).rejects.toEqual({code:"proposal_stale"});
 expect(submit).toHaveBeenCalledTimes(1);
});

it("one accepted proposal crosses IPC as exactly one insert carrying its version",async()=>{
 vi.mocked(invoke).mockClear();
 await acceptProposal(proposal,{current:()=>({root:"book",pageId:"page"}),persist:async()=>true,submit:async p=>{await helperInsertProposal("book",p.pageId,p.id,p.version);}});
 expect(invoke).toHaveBeenCalledExactlyOnceWith("insert_helper_proposal",{root:"book",pageId:"page",proposal:{proposalId:"p",proposalVersion:2}});
});

it("new placements pass the existing page clamp and refuse escaped positions",()=>{
 const value={...proposal,edit:false,version:1,items:[{...proposal.items[0],object:{...proposal.items[0].object,x:12,y:26},placement:"below_selection" as const}]};
 expect(()=>validateProposalPlacement(value,{widthPt:595,heightPt:842})).not.toThrow();
 value.items[0].object.x=1000;
 expect(()=>validateProposalPlacement(value,{widthPt:595,heightPt:842})).toThrow("invalid_placement");
});

import ProposalPreview from "./ProposalPreview.svelte";
import ProposalTray from "./ProposalTray.svelte";
import { proposalPreview } from "./state";
import { retainSessionProposals, reviewShortcut } from "./proposal";
import traySource from "./ProposalTray.svelte?raw";
import workspaceSource from "./HelperWorkspace.svelte?raw";

it("places opaque previews with the page coordinate transform at multiple zooms", () => {
  proposalPreview.set({...proposal, edit:false, items:proposal.items.map(item => ({...item, object:{...item.object,x:25,y:40}}))});
  for (const zoom of [0.5, 1, 2]) {
    const html = render(ProposalPreview, { props:{root:"book",pageId:"page",width:600,height:800,zoom} }).body;
    expect(html).toContain(`x="${25 * zoom}"`);
    expect(html).toContain(`y="${40 * zoom}"`);
    expect(html).toContain(`width="${100 * zoom}"`);
    expect(html).toContain(`height="${20 * zoom}"`);
    expect(html).toContain('fill="white"');
    expect(html).toContain('pointer-events: none');
  }
  expect(render(ProposalPreview,{props:{root:"other",pageId:"page",width:600,height:800,zoom:1}}).body).not.toContain("<svg");
  proposalPreview.set(null);
});
it("tray exposes pending and stale proposals as buttons and wires opening the card", () => {
  const html = render(ProposalTray,{props:{proposals:[proposal,{...proposal,id:"done",summary:"Hidden",status:"inserted"}],discarded:false,onOpen(){},onClearNotice(){}}}).body;
  expect(html).toContain("<button");
  expect(html).toContain(proposal.summary);
  expect(html).not.toContain("Hidden");
  // SSR omits event handlers: verify the small component's event-to-card wiring too.
  const tray = traySource;
  const workspace = workspaceSource;
  expect(tray).toContain("onclick={() => onOpen(proposal.id)}");
  expect(workspace).toContain("onOpen={id => selected = id}");
});
it("session end removes only its proposals and displays the translated discard notice", () => {
  const result = retainSessionProposals([proposal,{...proposal,id:"other",sessionId:"live"}],[{id:"live"}]);
  expect(result.proposals.map(p => p.id)).toEqual(["other"]);
  expect(result.discarded).toBe(true);
  const html = render(ProposalTray,{props:{...result,onOpen(){},onClearNotice(){}}}).body;
  expect(html).toContain('role="status"');
  expect(html).toContain("The assistant session ended. Its proposals were discarded.");
  expect(retainSessionProposals([{...proposal,status:"inserted"}],[]).discarded).toBe(false);
});
it("review shortcuts insert or close and leave normal typing alone", () => {
  const insert = vi.fn(async()=>{}), close = vi.fn();
  const event = {key:"Enter",ctrlKey:true,preventDefault:vi.fn(),stopPropagation:vi.fn()};
  reviewShortcut(event,insert,close);
  expect(insert).toHaveBeenCalledTimes(1);
  expect(event.preventDefault).toHaveBeenCalledTimes(1);
  reviewShortcut({...event,key:"Escape"},insert,close);
  expect(close).toHaveBeenCalledTimes(1);
  event.preventDefault.mockClear();
  reviewShortcut({...event,ctrlKey:false},insert,close);
  expect(event.preventDefault).not.toHaveBeenCalled();
});

const pageText:Proposal={...proposal,kind:"page_text_edit",pageNumber:3,managedHeader:"#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n",version:1,items:[{...proposal.items[0],object:{...proposal.items[0].object,type:"page_typst",id:"page-text"} as unknown as Proposal["items"][0]["object"],source:"= New\nBody",beforeSource:"= Old\nBody",svg:"<svg>AFTER_PAGE</svg>",beforeSvg:"<svg>BEFORE_PAGE</svg>",widthPt:595,heightPt:842}]};
const card=(value:Proposal,away=false)=>render(ProposalCard,{props:{proposal:value,away,onInsert:async()=>{},onClose(){},onUpdate(){}}}).body;
it("review dispatches by kind: Page text names its target, blocks keep their review",()=>{
 const html=card(pageText);
 for(const label of ["Page text · Page 3","Before","After","Apply change","Show whole page","Editable Page text body","managed preset header is preserved","#show: preset.with(rhythm: goodtype_rhythm)","Assumptions","Mass is positive","Fix the equation"])expect(html).toContain(label);
 expect(html).toContain("After: Page text on Page 3");
 expect(html).not.toContain("autofocus");
 expect(html.match(/class="primary[ "]/g)).toHaveLength(1);
 const block=card(proposal);
 expect(block).not.toContain("Page text ·");
 expect(block).not.toContain("Show whole page");
});
it("Page text source disclosure shows only the editable body with a line diff",()=>{
 const html=card(pageText);
 expect(html).toMatch(/class="del[^"]*"[^>]*>.*= Old/s);
 expect(html).toMatch(/class="add[^"]*"[^>]*>.*= New/s);
 expect(html).toMatch(/class="same[^"]*"[^>]*>.*Body/s);
 expect(html).not.toMatch(/<textarea[^>]*>[^<]*#import/);
});
it("overflow, another page and staleness block Apply with a named reason",()=>{
 const overflow=card({...pageText,overflow:true});
 expect(overflow).toContain("Content exceeds the page");
 expect(overflow).toMatch(/class="primary[^"]*"[^>]*disabled/);
 const away=card(pageText,true);
 expect(away).toContain("Return to Page 3");
 expect(away).toMatch(/class="primary[^"]*"[^>]*disabled/);
 const stale=card({...pageText,status:"stale",changed:[{dependency:"file",id:"/lib/defs.typ",change:"modified"},{dependency:"preset",change:"modified"}]});
 for(const label of ["Regenerate","Dismiss","Imported file /lib/defs.typ","Preset"])expect(stale).toContain(label);
 expect(stale).toMatch(/class="primary[^"]*"[^>]*disabled/);
 expect(card(pageText)).not.toMatch(/class="primary[^"]*"[^>]*disabled/);
});
it("Apply waits for the edited Page text to compile; blocks keep their existing gating",()=>{
 const idle={busy:false,dirty:false,stale:false,away:false};
 expect(applyBlocked(pageText,idle)).toBe(false);
 expect(applyBlocked(pageText,{...idle,dirty:true})).toBe(true);
 expect(applyBlocked(pageText,{...idle,busy:true})).toBe(true);
 expect(applyBlocked({...pageText,overflow:true},idle)).toBe(true);
 expect(applyBlocked(pageText,{...idle,away:true})).toBe(true);
 expect(applyBlocked({...pageText,status:"stale"},idle)).toBe(true);
 // Block review still compiles from its primary button when dirty.
 expect(applyBlocked(proposal,{...idle,dirty:true,away:true})).toBe(false);
});
it("compile diagnostics parse into body lines; other failures stay text",()=>{
 expect(compileDiagnostics('[{"severity":"error","message":"unknown variable: x","line":2}]')).toEqual([{severity:"error",message:"unknown variable: x",line:2}]);
 expect(compileDiagnostics("proposal changed")).toBeNull();
 expect(compileDiagnostics("[]")).toBeNull();
});
it("line diff keeps order and marks only changed lines",()=>{
 expect(lineDiff("a\nb\nc","a\nB\nc")).toEqual([{kind:"same",text:"a"},{kind:"del",text:"b"},{kind:"add",text:"B"},{kind:"same",text:"c"}]);
 expect(lineDiff("","x").filter(l=>l.kind!=="same")).toEqual([{kind:"del",text:""},{kind:"add",text:"x"}]);
});

import { presetError } from "./proposal";
it("preset review offers Install without page comparison and retains final success", () => {
 const p: Proposal = {...proposal, kind:"preset_install", items:[], edit:false, preset:{source:"body",name:"Lab",description:"Notes",sourceUrl:"<script>untrusted</script>",candidateHash:"h",dependencyHashes:{},check:{form:"function",sampleRevision:"1",rendered:true,widthPt:100,heightPt:50,diagnostics:[{severity:"warning",message:"unknown font family"}]},svg:"",png:"aGVsbG8=",filename:"x.typ",epoch:1,requestId:"r"}};
 const props={proposal:p,onInsert:async()=>{throw Error("page insert forbidden")},onClose(){},onUpdate(){}};
 const html=render(ProposalCard,{props}).body;
 expect(html).toContain(">Install</button>");expect(html).toContain("unknown font family");expect(html).toContain("&lt;script>");expect(html).not.toContain(">Before</button>");expect(html).not.toContain(">Apply change</button>");
 expect(render(ProposalCard,{props:{...props,proposal:{...p,status:"already_installed"}}}).body).toContain("no file was written");
 for(const state of [{busy:true,dirty:false,stale:false,away:false},{busy:false,dirty:true,stale:false,away:false},{busy:false,dirty:false,stale:true,away:false},{busy:false,dirty:false,stale:false,away:true}])expect(applyBlocked(p,state)).toBe(true);
 expect(applyBlocked(p,{busy:false,dirty:false,stale:false,away:false})).toBe(false);
});
it("maps preset String failures to owned recovery messages",()=>{
 expect(presetError("collision: file already exists")).toBe("proposal.presetCollision");
 expect(presetError("changed: cached package dependency")).toBe("proposal.presetDependency");
 expect(presetError("changed: preset review or authorization")).toBe("proposal.presetChanged");
 expect(presetError("changed: successful matching preview required")).toBe("proposal.presetPreviewRequired");
 expect(presetError("limit: 20 pending proposals")).toBe("proposal.presetLimit");
 expect(presetError("busy")).toBe("helper.detail_busy");
 expect(presetError("unexpected filesystem detail")).toBe("proposal.presetFailed");
});
