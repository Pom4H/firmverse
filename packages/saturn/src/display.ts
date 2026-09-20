/**
 * Deterministic 320x240 Saturn display emulator.
 *
 * This module deliberately does not depend on DOM, React, Canvas or wall clock.
 * A caller supplies model time and signal values. The emulator resolves bindings,
 * advances animation phases and returns a bounded vector command frame. Browser,
 * test and future C23/satgui targets can project the same frame differently.
 */

export type DisplayColor = number; // RGB565, matching the physical Saturn display.

export interface DisplayBinding {
  signal: string;
  scale?: number;
  offset?: number;
  min?: number;
  max?: number;
  fallback?: number;
}

export interface DisplayPoint { x:number; y:number }

interface DisplayNodeBase {
  id?: string;
  visible?: DisplayBinding;
  visibleAbove?: number;
}

export interface DisplayRectNode extends DisplayNodeBase {
  kind:'rect';
  x:number;y:number;width:number;height:number;
  fill:DisplayColor;
  stroke?:DisplayColor;
  strokeWidth?:number;
  radius?:number;
  opacity?:number;
}
export interface DisplayCircleNode extends DisplayNodeBase {
  kind:'circle';
  cx:number;cy:number;r:number;
  fill:DisplayColor;
  stroke?:DisplayColor;
  strokeWidth?:number;
  opacity?:number;
}
export interface DisplayLineNode extends DisplayNodeBase {
  kind:'line';
  x1:number;y1:number;x2:number;y2:number;
  color:DisplayColor;
  width?:number;
  opacity?:number;
}
export interface DisplayTextValue {
  value:DisplayBinding;
  prefix?:string;
  suffix?:string;
  digits?:number;
  on?:string;
  off?:string;
}
export interface DisplayTextNode extends DisplayNodeBase {
  kind:'text';
  x:number;y:number;
  text:string|DisplayTextValue;
  color:DisplayColor;
  size?:number;
  weight?:number;
  align?:'start'|'middle'|'end';
  mono?:boolean;
}
export interface DisplayTankNode extends DisplayNodeBase {
  kind:'tank';
  x:number;y:number;width:number;height:number;
  level:DisplayBinding;
  shell:DisplayColor;
  background:DisplayColor;
  water:DisplayColor;
  waterLine?:DisplayColor;
  strokeWidth?:number;
  waveAmplitude?:number;
  waveLength?:number;
  waveSpeed?:number;
  radius?:number;
}
export interface DisplayPumpNode extends DisplayNodeBase {
  kind:'pump';
  cx:number;cy:number;r:number;
  rpm:DisplayBinding;
  shell:DisplayColor;
  body:DisplayColor;
  bladeA:DisplayColor;
  bladeB?:DisplayColor;
  hub:DisplayColor;
  bladeCount?:number;
}
export interface DisplayFlowNode extends DisplayNodeBase {
  kind:'flow';
  points:DisplayPoint[];
  value:DisplayBinding;
  background:DisplayColor;
  color:DisplayColor;
  width?:number;
  backgroundWidth?:number;
  packetRadius?:number;
  packetSpacing?:number;
  /** Pixels per second at a resolved value of 1. */
  speed?:number;
  threshold?:number;
}
export interface DisplayLampNode extends DisplayNodeBase {
  kind:'lamp';
  cx:number;cy:number;r:number;
  value:DisplayBinding;
  off:DisplayColor;
  on:DisplayColor;
  highlight?:DisplayColor;
  halo?:DisplayColor;
  stroke?:DisplayColor;
  pulseHz?:number;
}

export type DisplayNode =
 | DisplayRectNode
 | DisplayCircleNode
 | DisplayLineNode
 | DisplayTextNode
 | DisplayTankNode
 | DisplayPumpNode
 | DisplayFlowNode
 | DisplayLampNode;

export interface SaturnDisplayScene {
  width?:320;
  height?:240;
  background:DisplayColor;
  nodes:readonly DisplayNode[];
}

interface DrawBase { opacity?:number }
export type DisplayDrawCommand =
 | (DrawBase&{type:'rect';x:number;y:number;width:number;height:number;fill:DisplayColor;stroke?:DisplayColor;strokeWidth?:number;radius?:number})
 | (DrawBase&{type:'circle';cx:number;cy:number;r:number;fill:DisplayColor;stroke?:DisplayColor;strokeWidth?:number})
 | (DrawBase&{type:'line';x1:number;y1:number;x2:number;y2:number;color:DisplayColor;width:number})
 | (DrawBase&{type:'polyline';points:DisplayPoint[];color:DisplayColor;width:number})
 | (DrawBase&{type:'polygon';points:DisplayPoint[];fill:DisplayColor})
 | (DrawBase&{type:'text';x:number;y:number;text:string;color:DisplayColor;size:number;weight:number;align:'start'|'middle'|'end';mono:boolean});

export interface SaturnDisplayFrame {
  width:320;
  height:240;
  timeMs:number;
  commands:DisplayDrawCommand[];
}

export type DisplaySignals = Readonly<Record<string,number|null|undefined>>;

const finite=(value:number,fallback=0)=>Number.isFinite(value)?value:fallback;
const clamp=(value:number,min:number,max:number)=>Math.max(min,Math.min(max,value));
const opacity=(value:number|undefined)=>value===undefined?1:clamp(finite(value,1),0,1);
const TAU=Math.PI*2;

function resolve(binding:DisplayBinding,signals:DisplaySignals):number {
  const raw=signals[binding.signal];
  let value=typeof raw==='number'&&Number.isFinite(raw)?raw:(binding.fallback??0);
  value=value*(binding.scale??1)+(binding.offset??0);
  if(binding.min!==undefined)value=Math.max(binding.min,value);
  if(binding.max!==undefined)value=Math.min(binding.max,value);
  return value;
}
function visible(node:DisplayNode,signals:DisplaySignals):boolean {
  if(!node.visible)return true;
  return resolve(node.visible,signals)>(node.visibleAbove??0);
}
function rotate(point:DisplayPoint,cx:number,cy:number,angle:number):DisplayPoint {
  const dx=point.x-cx,dy=point.y-cy,c=Math.cos(angle),s=Math.sin(angle);
  return {x:cx+dx*c-dy*s,y:cy+dx*s+dy*c};
}
function polylineLength(points:readonly DisplayPoint[]):number {
  let total=0;for(let i=1;i<points.length;i++)total+=Math.hypot(points[i]!.x-points[i-1]!.x,points[i]!.y-points[i-1]!.y);return total;
}
function pointAt(points:readonly DisplayPoint[],distance:number):DisplayPoint {
  if(points.length===0)return{x:0,y:0};
  if(points.length===1)return{...points[0]!};
  const total=polylineLength(points);if(total<=0)return{...points[0]!};
  let target=((distance%total)+total)%total;
  for(let i=1;i<points.length;i++){
    const a=points[i-1]!,b=points[i]!,len=Math.hypot(b.x-a.x,b.y-a.y);
    if(target<=len||i===points.length-1){const t=len>0?clamp(target/len,0,1):0;return{x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t};}
    target-=len;
  }
  return{...points[points.length-1]!};
}
function formatText(value:DisplayTextValue,signals:DisplaySignals):string {
  const n=resolve(value.value,signals);
  if(value.on!==undefined&&value.off!==undefined)return(value.prefix??'')+(n!==0?value.on:value.off)+(value.suffix??'');
  const digits=value.digits??0;
  return(value.prefix??'')+n.toFixed(digits)+(value.suffix??'');
}
interface PhaseState {timeMs:number;phase:number}

/**
 * Stateful only for animation phase continuity. PLC state remains owned by
 * SaturnRuntime. Calling render twice at the same model time is observation-only.
 */
export class SaturnDisplayEmulator {
  private phases=new Map<string,PhaseState>();

  reset():void {this.phases.clear();}

  private phase(id:string,timeMs:number,radiansPerMs:number):number {
    let state=this.phases.get(id);
    if(!state){state={timeMs,phase:0};this.phases.set(id,state);return 0;}
    const dt=Math.max(0,timeMs-state.timeMs);
    if(dt>0){state.phase=(state.phase+radiansPerMs*dt)%TAU;state.timeMs=timeMs;}
    return state.phase;
  }

  render(scene:SaturnDisplayScene,signals:DisplaySignals,timeMs:number):SaturnDisplayFrame {
    if(!Number.isFinite(timeMs)||timeMs<0)throw new Error('Saturn display time must be a finite non-negative number');
    if(scene.width!==undefined&&scene.width!==320)throw new Error('Saturn display width must be 320');
    if(scene.height!==undefined&&scene.height!==240)throw new Error('Saturn display height must be 240');
    if(scene.nodes.length>256)throw new Error('Saturn display scene exceeds 256 nodes');
    const commands:DisplayDrawCommand[]=[{type:'rect',x:0,y:0,width:320,height:240,fill:scene.background}];

    const push=(command:DisplayDrawCommand)=>{if(commands.length>=1024)throw new Error('Saturn display frame exceeds 1024 draw commands');commands.push(command);};

    for(let index=0;index<scene.nodes.length;index++){
      const node=scene.nodes[index]!;if(!visible(node,signals))continue;
      const id=node.id??node.kind+':'+index;
      switch(node.kind){
        case'rect':push({type:'rect',x:node.x,y:node.y,width:node.width,height:node.height,fill:node.fill,...(node.stroke===undefined?{}:{stroke:node.stroke}),...(node.strokeWidth===undefined?{}:{strokeWidth:node.strokeWidth}),...(node.radius===undefined?{}:{radius:node.radius}),opacity:opacity(node.opacity)});break;
        case'circle':push({type:'circle',cx:node.cx,cy:node.cy,r:node.r,fill:node.fill,...(node.stroke===undefined?{}:{stroke:node.stroke}),...(node.strokeWidth===undefined?{}:{strokeWidth:node.strokeWidth}),opacity:opacity(node.opacity)});break;
        case'line':push({type:'line',x1:node.x1,y1:node.y1,x2:node.x2,y2:node.y2,color:node.color,width:node.width??1,opacity:opacity(node.opacity)});break;
        case'text':push({type:'text',x:node.x,y:node.y,text:typeof node.text==='string'?node.text:formatText(node.text,signals),color:node.color,size:node.size??10,weight:node.weight??600,align:node.align??'start',mono:node.mono??false});break;
        case'tank':{
          const level=clamp(resolve(node.level,signals),0,1),sw=node.strokeWidth??2,r=node.radius??8;
          push({type:'rect',x:node.x,y:node.y,width:node.width,height:node.height,fill:node.background,stroke:node.shell,strokeWidth:sw,radius:r});
          const pad=Math.max(4,sw+3),innerH=Math.max(0,node.height-pad*2),waterH=innerH*level,waterY=node.y+node.height-pad-waterH;
          if(waterH>0)push({type:'rect',x:node.x+pad,y:waterY,width:Math.max(0,node.width-pad*2),height:waterH,fill:node.water,radius:Math.max(2,r/2)});
          if(level>0&&node.waterLine!==undefined){
            const amp=node.waveAmplitude??2.5,wave=node.waveLength??22,speed=node.waveSpeed??.006,points:DisplayPoint[]=[];
            const phase=timeMs*speed;
            for(let x=node.x+pad;x<=node.x+node.width-pad;x+=4)points.push({x,y:waterY+Math.sin((x-node.x)/wave*TAU+phase)*amp});
            push({type:'polyline',points,color:node.waterLine,width:2});
          }
          break;
        }
        case'pump':{
          const rpm=Math.max(0,resolve(node.rpm,signals)),angle=this.phase(id,timeMs,rpm/60000*TAU),count=clamp(Math.round(node.bladeCount??6),3,12);
          push({type:'circle',cx:node.cx,cy:node.cy,r:node.r+5,fill:node.shell});
          push({type:'circle',cx:node.cx,cy:node.cy,r:node.r,fill:node.body,stroke:node.bladeA,strokeWidth:1});
          for(let i=0;i<count;i++){
            const a=angle+i/count*TAU;
            const p0={x:node.cx+node.r*.14,y:node.cy-node.r*.04},p1={x:node.cx+node.r*.80,y:node.cy-node.r*.28},p2={x:node.cx+node.r*.74,y:node.cy+node.r*.12},p3={x:node.cx+node.r*.22,y:node.cy+node.r*.18};
            push({type:'polygon',points:[p0,p1,p2,p3].map(p=>rotate(p,node.cx,node.cy,a)),fill:i%2===0?node.bladeA:(node.bladeB??node.bladeA)});
          }
          push({type:'circle',cx:node.cx,cy:node.cy,r:node.r*.16,fill:node.hub});
          break;
        }
        case'flow':{
          if(node.points.length<2)break;
          const value=resolve(node.value,signals),active=Math.abs(value)>(node.threshold??.001),abs=Math.abs(value);
          push({type:'polyline',points:node.points.map(p=>({...p})),color:node.background,width:node.backgroundWidth??Math.max(6,(node.width??3)+4),opacity:1});
          if(active){
            push({type:'polyline',points:node.points.map(p=>({...p})),color:node.color,width:node.width??3,opacity:.92});
            const length=polylineLength(node.points),spacing=Math.max(10,node.packetSpacing??22),speed=(node.speed??48)*Math.max(.2,abs);
            const phase=this.phase(id,timeMs,speed/Math.max(1,length)*TAU),offset=phase/TAU*length,direction=value<0?-1:1;
            const packetCount=Math.min(32,Math.max(1,Math.floor(length/spacing)));
            for(let i=0;i<packetCount;i++){
              const p=pointAt(node.points,direction*(offset+i*spacing));
              push({type:'circle',cx:p.x,cy:p.y,r:node.packetRadius??2.6,fill:node.color,opacity:.96});
            }
          }
          break;
        }
        case'lamp':{
          const value=clamp(resolve(node.value,signals),0,1),on=value>.001,pulseHz=node.pulseHz??1.25;
          const pulse=on?.72+.18*Math.sin(timeMs/1000*pulseHz*TAU):0;
          if(on&&node.halo!==undefined)push({type:'circle',cx:node.cx,cy:node.cy,r:node.r*1.65,fill:node.halo,opacity:pulse*value});
          push({type:'circle',cx:node.cx,cy:node.cy,r:node.r,fill:on?node.on:node.off,...(node.stroke===undefined?{}:{stroke:node.stroke}),strokeWidth:node.stroke===undefined?undefined:2});
          if(on&&node.highlight!==undefined)push({type:'circle',cx:node.cx-node.r*.28,cy:node.cy-node.r*.30,r:node.r*.18,fill:node.highlight,opacity:.78});
          break;
        }
      }
    }
    return{width:320,height:240,timeMs,commands};
  }
}
