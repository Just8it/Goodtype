import { describe, it, expect, vi } from 'vitest';
import { cellGrant, cellsAbove, validateCellContext, registerCellApply, applyAndRun } from './cellIntent';
import { cellProblemKeys } from '../ipc/helper';
import { codexProblem } from '../helper/codex';
import type { Proposal, ApplyCellResult } from '../ipc/helper';
describe('cell context', () => {
 it('starts with only the active cell, outputs on, and adds above in document order', () => {
 const grant=cellGrant('calc','part',['a','b','c'],'c');
 expect(grant.cellIds).toEqual(['c']); expect(grant.includeOutputs).toBe(true);
 expect(cellsAbove(grant,['a','b','c']).cellIds).toEqual(['a','b','c']);
 expect(validateCellContext(grant)).toEqual(grant);
 });
 it('refuses missing targets, empty, duplicate and oversized grants', () => {
 expect(()=>cellGrant('c','p',['a'],'b')).toThrow();
 for(const cellIds of [[],['a','a'],Array.from({length:101},(_,i)=>String(i))]) expect(()=>validateCellContext({calculationId:'c',partId:'p',cellIds,includeOutputs:true})).toThrow();
 expect(()=>cellsAbove(cellGrant('c','p',['100'],'100'),Array.from({length:101},(_,i)=>String(i)))).toThrow();
 });
 it('adopts before running and never runs on failed apply', async () => {
 const order:string[]=[]; const run=vi.fn();
 const unregister=registerCellApply(async(_p,_result,shouldRun)=>{order.push('refresh');if(shouldRun){order.push('run');run();}});
 const p={} as Proposal,r={} as ApplyCellResult;
 await applyAndRun(p,true,async()=>{order.push('apply');return r});
 expect(order).toEqual(['apply','refresh','run']);
 await expect(applyAndRun(p,true,async()=>{throw Error('stale')})).rejects.toThrow('stale');expect(run).toHaveBeenCalledTimes(1);
 await applyAndRun(p,false,async()=>r);expect(run).toHaveBeenCalledTimes(1);unregister();
 });
 it('maps every structured cell problem',()=>{for(const code of Object.keys(cellProblemKeys))expect(codexProblem({code,params:{}})).toMatchObject({key:`helper.${code}`});});
});
