import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const wasmPath = process.argv[2] ?? new URL('./dist/firmverse.wasm', import.meta.url);
const bytes = await readFile(wasmPath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const api = instance.exports;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

for (const name of [
  'memory',
  'firmverse_input_reserve',
  'firmverse_call',
  'firmverse_result_ptr',
  'firmverse_result_len',
]) {
  assert.ok(api[name], `missing WASM export ${name}`);
}

function call(request) {
  const input = encoder.encode(JSON.stringify(request));
  const inputPtr = Number(api.firmverse_input_reserve(input.length));
  new Uint8Array(api.memory.buffer, inputPtr, input.length).set(input);
  api.firmverse_call(input.length);
  const ptr = Number(api.firmverse_result_ptr());
  const len = Number(api.firmverse_result_len());
  const response = JSON.parse(decoder.decode(new Uint8Array(api.memory.buffer, ptr, len)));
  assert.equal(response.ok, true, response.error ?? JSON.stringify(response));
  return response;
}

const registry = call({ op: 'registry' }).registry;
assert.ok(registry.boards.some((board) => board.id === 'pb03f-kit' && board.soc === 'phy6252'));
assert.ok(registry.socs.some((soc) => soc.id === 'phy6252' && soc.implemented));
assert.ok(registry.worlds.some((world) => world.id === 'mesh'));
assert.ok(registry.pins.phy6252.some((pin) => pin.label === 'P15' && pin.adcChannel === 1));
assert.equal(registry.compilerSchemas['saturn-plc'], 'firmverse/saturn-control-ir@1');

const saturnArtifact = call({
  op: 'compileSaturnControlIr',
  controlIr: {
    schema: 'firmverse/saturn-control-ir@1',
    project: {
      name: 'WASM compiler smoke',
      version: '1',
      buildTime: '2026-08-29',
    },
    elements: [
      { id: 'di', type: 'INP_PIN', params: [1] },
      { id: 'do', type: 'OUT_PIN', inputs: ['di'], params: [1] },
    ],
  },
});
assert.equal(saturnArtifact.artifact.format, 'fbdbin');
assert.equal(saturnArtifact.artifact.encoding, 'hex');
assert.equal(saturnArtifact.artifact.elements, 2);
assert.equal(saturnArtifact.listing.length, 2);
assert.match(saturnArtifact.artifact.data, /^[0-9A-F]+$/);
assert.equal(saturnArtifact.artifact.data.length, saturnArtifact.artifact.bytes * 2);

call({ op: 'new', world: 'mesh', looping: true, strict: true, maxInsns: 10000 });

// Tiny Cortex-M0 image at PHY6252 SRAM: valid vector pair followed by `b .`.
// It proves the browser artifact executes the real zmu-backed CPU path rather
// than only exposing registry metadata.
const firmware = [
  ':020000041FFFDC',
  ':0A0000000010FF1F0900FF1FFEE7BC',
  ':00000001FF',
].join('\n');

let result = call({
  op: 'addNode',
  id: 'web0',
  board: 'pb03f-kit',
  label: 'smoke.hex',
  firmware,
  x: 0,
  y: 0,
});
assert.equal(result.snapshot.nodes.length, 1);
assert.equal(result.snapshot.nodes[0].board, 'pb03f-kit');

result = call({ op: 'tick', ticks: 2, burst: 16 });
assert.ok(result.snapshot.nodes[0].insns >= 32, 'zmu did not execute instructions in WASM');
assert.equal(result.snapshot.nodes[0].stopped, null);

call({
  op: 'addNode',
  id: 'web1',
  board: 'pb03f-kit',
  label: 'smoke.hex',
  firmware,
  x: 3,
  y: 0,
});
result = call({ op: 'tick', ticks: 1, burst: 4 });
const a = result.snapshot.nodes.find((node) => node.id === 'web0');
const b = result.snapshot.nodes.find((node) => node.id === 'web1');
assert.ok(a.heard.some((heard) => heard.nodeId === 'web1'), 'web0 did not hear web1');
assert.ok(b.heard.some((heard) => heard.nodeId === 'web0'), 'web1 did not hear web0');

call({ op: 'moveNode', id: 'web1', x: 80, y: 0 });
result = call({ op: 'tick', ticks: 1, burst: 4 });
assert.ok(!result.snapshot.nodes.find((node) => node.id === 'web0').heard.some((heard) => heard.nodeId === 'web1'));

console.log(
  `Firmverse WASM smoke OK: ${bytes.length} bytes, Saturn compiler + zmu + two-node World executed`,
);
