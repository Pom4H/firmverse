/** Portable Saturn toolchain: one Rust compiler, one upstream C execution engine.
 * Each runtime gets its own WebAssembly instance; no shared process-global PLC.
 */
import { compilerBase64, runtimeBase64, compilerHash, runtimeHash } from './binaries.ts';
export { compilerHash, runtimeHash };
export const STATE_ABI = `firmverse/saturn-state@1:${runtimeHash}`;
export interface ElementSpec { id: string; type: string | number; inputs?: readonly string[]; params?: readonly number[]; invert?: boolean; caption?: string; comment?: string }
export interface ControlIR { schema: 'firmverse/saturn-control-ir@1'; project: {name:string;version:string;buildTime:string}; elements: readonly ElementSpec[]; screens?: readonly (readonly number[])[]; hints?: readonly {type:'input'|'output'|'event'; index:number;text:string}[] }
export interface CompiledProgram { fbdbin: Uint8Array; elementCount:number; screenCount:number; requiredRtlVersion:number; listing: {index:number;id:string;type:string;inputs:string[];params:number[];comment:string}[] }
export type HmiDrawCommand =
 | {type:'rect'|'line'|'ellipse'; x1:number;y1:number;x2:number;y2:number;color:number}
 | {type:'text';x:number;y:number;font:number;color:number;bkcolor:number;transparent:boolean;text:string}
 | {type:'image';x:number;y:number;image:number};
type Wasm = WebAssembly.Exports & {memory:WebAssembly.Memory};
const bytes=(s:string)=>Uint8Array.from(atob(s),c=>c.charCodeAt(0));
let compiler: Wasm | undefined, runtimeModule: WebAssembly.Module | undefined;
function call(e:Wasm,name:string,...args:number[]):number { const fn=e[name];if(typeof fn!=='function')throw new Error(`Missing Saturn ABI: ${name}`);return Number(fn(...args)); }
const heap=(e:Wasm)=>new Uint8Array(e.memory.buffer);
function invokeCompiler(input:Uint8Array,operation:string):any {
 if(input.length>1_048_576)throw new Error('Saturn compiler input exceeds 1 MiB');
 compiler??=new WebAssembly.Instance(new WebAssembly.Module(bytes(compilerBase64)),{}).exports as Wasm;
 const ptr=call(compiler,'fv_input_reserve',input.length);if(!ptr)throw new Error('Compiler allocation failed');
 heap(compiler).set(input,ptr);call(compiler,operation);
 const result=JSON.parse(new TextDecoder().decode(heap(compiler).slice(call(compiler,'fv_result_ptr'),call(compiler,'fv_result_ptr')+call(compiler,'fv_result_len'))));
 if(!result.ok)throw new Error(result.error);return result;
}
export function compileControlIR(ir:ControlIR):CompiledProgram {
 const result=invokeCompiler(new TextEncoder().encode(JSON.stringify(ir)),'fv_compile');
 return {fbdbin:Uint8Array.from(result.bytes),elementCount:result.elementCount,screenCount:result.screenCount,requiredRtlVersion:result.requiredRtl,listing:result.listing};
}
export function inspectProgram(program:Uint8Array):{elements:number;rtl:number;screens:number} {return invokeCompiler(program,'fv_inspect');}
export interface RuntimeSnapshot { abi:string; program:string; data:number[] }
function integer(value:number,min=-2147483648,max=2147483647):number { if(!Number.isInteger(value)||value<min||value>max)throw new Error('Saturn value outside integer range');return value; }
function cp1251(e:Wasm,ptr:number):string {
 if(!ptr)return '';const h=heap(e);let end=ptr;while(end<h.length&&h[end]&&end-ptr<4096)end++;
 return new TextDecoder('windows-1251').decode(h.subarray(ptr,end));
}
/** Bounded deterministic local-block profile. Transport/RTC/random MFUN are not
 * silently emulated. They require separately implemented environment capabilities.
 */
function checkExecutionCapabilities(program:Uint8Array):void {
 const stop=program.indexOf(0x94);if(stop<1||stop>256)throw new Error('Saturn execution requires 1..256 blocks');
 const unsupported=new Set([14,16,33,34,36,37]);
 for(let i=0;i<stop;i++)if(unsupported.has(program[i]&63))throw new Error('Saturn block requires an unavailable environment capability');
 const arities=[1,0,1,2,2,2,2,2,2,2,2,2,2,2,1,0,0,4,3,3,5,1,1,0,2,2,2,3,2,2,2,2,2,0,1,2,0,1,5,1,5];
 let at=stop+1;const graph:number[][]=[];
 for(let i=0;i<stop;i++){const refs:number[]=[];for(let n=0;n<arities[program[i]&63];n++){refs.push(program[at]|program[at+1]<<8);at+=2;}graph.push(refs);}
 // Mirror the upstream evaluation stack bound. Cycles are marked before descent.
 for(let root=0;root<stop;root++){
  const visited=new Set<number>();const walk=(index:number,depth:number):void=>{
   if(visited.has(index))return;if(depth>120)throw new Error('Saturn graph exceeds the runtime stack budget');visited.add(index);
   for(const ref of graph[index])walk(ref,depth+1);
  };walk(root,0);
 }

}
export class SaturnRuntime {
 private e!:Wasm; private program=new Uint8Array(); private identity=''; private loaded=false;
 static createSync():SaturnRuntime {return new SaturnRuntime();}
 static async create():Promise<SaturnRuntime> {return new SaturnRuntime();}
 private instantiate():Wasm {
  runtimeModule??=new WebAssembly.Module(bytes(runtimeBase64));
  // No filesystem, network, clocks or randomness are granted to this package.
  let e:Wasm,initializing=true;
  // WASI libc initializes its stack canary once; this is not a PLC random source.
  const wasi={fd_close:()=>76,fd_seek:()=>76,fd_write:()=>76,random_get:(ptr:number,size:number)=>{
   if(!initializing)throw new Error('Randomness is not a granted PLC capability');
   heap(e).fill(0xa5,ptr,ptr+size);return 0;}};
  e=new WebAssembly.Instance(runtimeModule,{wasi_snapshot_preview1:wasi}).exports as Wasm;call(e,'_initialize');initializing=false;return e;
 }
 load(program:Uint8Array,needReset=true):{ok:true;memorySize:number}|{ok:false;code:number;message:string} {
  try {
   inspectProgram(program);checkExecutionCapabilities(program);
   // Load on a candidate instance: a rejected program never destroys the live one.
   const candidate=this.instantiate(),ptr=call(candidate,'malloc',program.length);if(!ptr)throw new Error('Runtime allocation failed');
   heap(candidate).set(program,ptr);const size=call(candidate,'fv_fbd_load',ptr,program.length,1);call(candidate,'free',ptr);
   if(size<=0)throw new Error(`FBD load rejected (${size})`);
   if(!needReset)throw new Error('Warm program replacement requires an explicit retained-state migration');
   this.e=candidate;this.program=program.slice();this.identity=Array.from(program,b=>b.toString(16).padStart(2,'0')).join('');this.loaded=true;
   return {ok:true,memorySize:size};
  } catch(error) {return {ok:false,code:-100,message:error instanceof Error?error.message:String(error)};}
 }
 private use():Wasm {if(!this.loaded)throw new Error('No Saturn program loaded');return this.e;}
 setInput(pin:number,value:number|boolean):void {call(this.use(),'fv_fbd_set_input',integer(pin,0,127),integer(Number(value)));}
 getOutput(pin:number):number {return call(this.use(),'fv_fbd_get_output',integer(pin,0,127));}
 step(periodMs:number):void {call(this.use(),'fv_fbd_step',integer(periodMs,0,1000));}
 /** Rendering reads values only: never call fbdDoStepEx to obtain a picture. */
 renderScreen(screen=0):HmiDrawCommand[] {
  const e=this.use(),count=call(e,'fv_fbd_render',integer(screen,0,31));if(count<0)throw new Error('HMI draw budget exceeded');
  const out:HmiDrawCommand[]=[];
  for(let i=0;i<count;i++){
   const f=(n:number)=>call(e,'fv_fbd_draw_field',i,n),kind=f(0);
   if(kind===1)out.push({type:'text',x:f(1),y:f(2),font:f(7),color:f(5),bkcolor:f(6),transparent:!!f(8),text:cp1251(e,call(e,'fv_fbd_draw_text',i))});
   else if(kind===4)out.push({type:'image',x:f(1),y:f(2),image:f(9)});
   else out.push({type:kind===0?'rect':kind===2?'line':'ellipse',x1:f(1),y1:f(2),x2:f(3),y2:f(4),color:f(5)});
  }return out;
 }
 stepAndRenderScreen(periodMs:number,screen=0):HmiDrawCommand[]{this.step(periodMs);return this.renderScreen(screen);}
 snapshot():RuntimeSnapshot {
  const e=this.use(),size=call(e,'fv_fbd_snapshot_size'),ptr=call(e,'malloc',size);
  try {if(!ptr||size<16||size>65536||call(e,'fv_fbd_snapshot',ptr,size)!==size)throw new Error('Snapshot failed');
   return {abi:STATE_ABI,program:this.identity,data:Array.from(heap(e).slice(ptr,ptr+size))};
  }finally {if(ptr)call(e,'free',ptr);}
 }
 restore(state:RuntimeSnapshot):void {
  const e=this.use();if(state?.abi!==STATE_ABI||state.program!==this.identity||!Array.isArray(state.data)||state.data.length!==call(e,'fv_fbd_snapshot_size')||state.data.length>65536||state.data.some(v=>!Number.isInteger(v)||v<0||v>255))throw new Error('Incompatible Saturn snapshot');
  const ptr=call(e,'malloc',state.data.length);if(!ptr)throw new Error('Snapshot allocation failed');
  try {heap(e).set(state.data,ptr);if(call(e,'fv_fbd_restore',ptr,state.data.length)!==1)throw new Error('Corrupted Saturn snapshot');}finally{call(e,'free',ptr);}
 }
 reset():void {const result=this.load(this.program);if(!result.ok)throw new Error(result.message);}
 get setpointCount():number{return call(this.use(),'fv_fbd_sp_count');}
 getSetpoint(index:number){const e=this.use();integer(index,0,this.setpointCount-1);return {index,caption:cp1251(e,call(e,'fv_fbd_sp_caption',index)),value:call(e,'fv_fbd_sp_value',index),lowLimit:call(e,'fv_fbd_sp_low',index),upperLimit:call(e,'fv_fbd_sp_high',index),defValue:call(e,'fv_fbd_sp_default',index),divider:call(e,'fv_fbd_sp_divider',index),step:call(e,'fv_fbd_sp_step',index)};}
 setSetpoint(index:number,value:number):void {const p=this.getSetpoint(index);call(this.use(),'fv_fbd_sp_set',index,integer(value,p.lowLimit,p.upperLimit));}
 get watchpointCount():number{return call(this.use(),'fv_fbd_wp_count');}
 getWatchpoint(index:number){const e=this.use();integer(index,0,this.watchpointCount-1);return {index,caption:cp1251(e,call(e,'fv_fbd_wp_caption',index)),value:call(e,'fv_fbd_wp_value',index),divider:call(e,'fv_fbd_wp_divider',index)};}
 getProjectInfo(){const e=this.use();return {name:cp1251(e,call(e,'fv_fbd_project_field',0)),version:cp1251(e,call(e,'fv_fbd_project_field',1)),buildTime:cp1251(e,call(e,'fv_fbd_project_field',2))};}
 getIoHint(type:0|1,pin:number):string{return cp1251(this.use(),call(this.e,'fv_fbd_io_hint',type,integer(pin,0,127)));}
}
