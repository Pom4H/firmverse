import test from 'node:test';
import assert from 'node:assert/strict';
import { compileControlIR, inspectProgram, SaturnRuntime, type ElementSpec, type ControlIR } from './src/index.ts';
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
