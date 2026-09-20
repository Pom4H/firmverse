import test from 'node:test';
import assert from 'node:assert/strict';
import { compileControlIR, inspectProgram, SaturnRuntime, SaturnDisplayEmulator, type SaturnDisplayScene, type ElementSpec, type ControlIR } from './src/index.ts';
const ir=(elements:ElementSpec[]):ControlIR=>({schema:'firmverse/saturn-control-ir@1',project:{name:'Portable PLC',version:'1',buildTime:'reproducible'},elements});
const program=()=>compileControlIR(ir([
 {id:'di',type:'INP_PIN',params:[1]}, {id:'time',type:'CONST',params:[500]},
 {id:'timer',type:'TON',inputs:['di','time']}, {id:'out',type:'OUT_PIN',inputs:['timer'],params:[1]},
])).fbdbin;
const load=(p=program())=>{const vm=SaturnRuntime.createSync();assert.equal(vm.load(p).ok,true);return vm;};
test('compiler and inspector share the versioned native Rust implementation',()=>{assert.equal(inspectProgram(program()).elements,4);assert.deepEqual(program(),program());});
test('timer and independent WASM instances resume exactly, including hidden state',()=>{
 const a=load();a.setInput(1,0);a.step(100);a.setInput(1,1);a.step(100);a.step(100);
 const snapshot=a.snapshot(),b=load();b.restore(snapshot);
 for(let i=0;i<10;i++){a.renderScreen();assert.deepEqual(a.snapshot(),snapshot);}
 const values=[];for(let i=0;i<4;i++){a.step(100);b.step(100);assert.deepEqual(a.snapshot(),b.snapshot());values.push(a.getOutput(1));}
 assert.deepEqual(values,[0,0,1,1]);
});
test('corrupted, wrong-program and wrong-ABI snapshots preserve the running state',()=>{
 const a=load();a.step(10);const saved=a.snapshot();
 for(const bad of [{...saved,abi:'wrong'},{...saved,program:'00'},{...saved,data:saved.data.map((x,i)=>i===saved.data.length-1?x^1:x)}]){assert.throws(()=>a.restore(bad));assert.deepEqual(a.snapshot(),saved);}
});
test('rejected load does not replace the current runtime',()=>{const a=load();a.step(100);const before=a.snapshot(),bad=program();bad[10]^=1;assert.equal(a.load(bad).ok,false);assert.deepEqual(a.snapshot(),before);});
test('environment-dependent operations are explicitly unsupported for portable execution',()=>{
 const p=compileControlIR(ir([{id:'random',type:'MFUN',params:[1,0,0,0]}])).fbdbin;
 assert.equal(SaturnRuntime.createSync().load(p).ok,false);
});
test('cold reset does not retain state merely because checkpoint can retain it',()=>{
 const a=load();a.setInput(1,0);a.step(100);a.setInput(1,1);for(let i=0;i<6;i++)a.step(100);assert.equal(a.getOutput(1),1);a.reset();assert.equal(a.getOutput(1),0);
});
test('unsupported graph depths are rejected before the C runtime',()=>{
 const e:ElementSpec[]=[{id:'a0',type:'CONST',params:[1]}];for(let i=1;i<140;i++)e.push({id:'a'+i,type:'NOT',inputs:['a'+(i-1)]});
 const a=SaturnRuntime.createSync();assert.equal(a.load(compileControlIR(ir(e)).fbdbin).ok,false);
});
test('I/O and elapsed time validate integer bounds',()=>{const a=load();assert.throws(()=>a.setInput(999,1));assert.throws(()=>a.step(-1));assert.throws(()=>a.step(.5));assert.throws(()=>a.setInput(1,NaN));});

test('display emulator renders live tank, flow, rotor and lamp animation without DOM or CSS',()=>{
 const display=new SaturnDisplayEmulator();
 const scene:SaturnDisplayScene={background:0x0010,nodes:[
  {id:'tank',kind:'tank',x:8,y:20,width:70,height:100,level:{signal:'level'},shell:0xffff,background:0x0841,water:0x05ff,waterLine:0x07ff},
  {id:'flow',kind:'flow',points:[{x:78,y:70},{x:132,y:70},{x:160,y:96}],value:{signal:'flow'},background:0x2104,color:0x07ff,speed:60},
  {id:'pump',kind:'pump',cx:192,cy:96,r:30,rpm:{signal:'rpm'},shell:0x0841,body:0x2104,bladeA:0x07ff,bladeB:0x04b2,hub:0xffff},
  {id:'lamp',kind:'lamp',cx:278,cy:96,r:18,value:{signal:'lamp'},off:0x2104,on:0x07e0,halo:0x07e0,highlight:0xffff},
 ]};
 const signals={level:.65,flow:.8,rpm:300,lamp:1};
 const a=display.render(scene,signals,0),same=display.render(scene,signals,0),b=display.render(scene,signals,100);
 assert.deepEqual(same,a,'reading a display twice at the same model time must be observation-only');
 const water=a.commands.find(c=>c.type==='rect'&&c.fill===0x05ff);assert.ok(water&&water.type==='rect'&&water.height>50);
 const bladeA=a.commands.find(c=>c.type==='polygon'&&c.fill===0x07ff),bladeB=b.commands.find(c=>c.type==='polygon'&&c.fill===0x07ff);
 assert.ok(bladeA&&bladeB&&bladeA.type==='polygon'&&bladeB.type==='polygon');assert.notDeepEqual(bladeA.points,bladeB.points);
 const packetsA=a.commands.filter(c=>c.type==='circle'&&c.fill===0x07ff&&c.r<5),packetsB=b.commands.filter(c=>c.type==='circle'&&c.fill===0x07ff&&c.r<5);
 assert.ok(packetsA.length>0&&packetsB.length>0);assert.notDeepEqual(packetsA,packetsB);
 const haloA=a.commands.find(c=>c.type==='circle'&&c.fill===0x07e0&&c.r>20),haloB=b.commands.find(c=>c.type==='circle'&&c.fill===0x07e0&&c.r>20);
 assert.ok(haloA&&haloB);assert.notEqual(haloA.opacity,haloB.opacity);
});
test('display animation is driven by supplied signals and freezes with zero motion',()=>{
 const display=new SaturnDisplayEmulator();
 const scene:SaturnDisplayScene={background:0,nodes:[
  {id:'pump',kind:'pump',cx:100,cy:100,r:30,rpm:{signal:'rpm'},shell:1,body:2,bladeA:3,hub:4},
  {id:'flow',kind:'flow',points:[{x:130,y:100},{x:240,y:100}],value:{signal:'flow'},background:5,color:6},
 ]};
 const stopped0=display.render(scene,{rpm:0,flow:0},0),stopped1=display.render(scene,{rpm:0,flow:0},500);
 assert.deepEqual(stopped0.commands,stopped1.commands);
 assert.equal(stopped1.commands.filter(c=>c.type==='circle'&&c.fill===6).length,0);
 display.reset();
 const running0=display.render(scene,{rpm:240,flow:1},0),running1=display.render(scene,{rpm:240,flow:1},250);
 assert.notDeepEqual(running0.commands,running1.commands);
});
test('display validates physical size and command budget',()=>{
 const display=new SaturnDisplayEmulator();
 assert.throws(()=>display.render({width:320,height:240,background:0,nodes:[]},{},-1),/time/);
 assert.throws(()=>display.render({width:320,height:240,background:0,nodes:Array.from({length:257},(_,i)=>({kind:'rect' as const,x:i,y:0,width:1,height:1,fill:0}))},{},0),/256/);
});
