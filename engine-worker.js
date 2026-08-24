let wasm = null;
let running = false;
let runTimer = null;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

async function instantiate(url) {
  const imports = {};
  if (WebAssembly.instantiateStreaming) {
    try {
      const response = await fetch(url);
      const module = await WebAssembly.instantiateStreaming(response, imports);
      return module.instance;
    } catch (error) {
      console.warn('instantiateStreaming failed, falling back to ArrayBuffer', error);
    }
  }
  const response = await fetch(url);
  if (!response.ok) throw new Error(`WASM fetch failed: ${response.status}`);
  const bytes = await response.arrayBuffer();
  const module = await WebAssembly.instantiate(bytes, imports);
  return module.instance;
}

function call(request) {
  if (!wasm) throw new Error('Firmverse WASM is not initialized');
  const bytes = encoder.encode(JSON.stringify(request));
  const ptr = Number(wasm.exports.firmverse_input_reserve(bytes.length));
  new Uint8Array(wasm.exports.memory.buffer, ptr, bytes.length).set(bytes);
  wasm.exports.firmverse_call(bytes.length);
  const outPtr = Number(wasm.exports.firmverse_result_ptr());
  const outLen = Number(wasm.exports.firmverse_result_len());
  const raw = decoder.decode(new Uint8Array(wasm.exports.memory.buffer, outPtr, outLen));
  const result = JSON.parse(raw);
  if (!result.ok) throw new Error(result.error || 'Firmverse operation failed');
  return result;
}

function publishSnapshot(result) {
  if (result?.snapshot) postMessage({ type: 'snapshot', snapshot: result.snapshot });
}

function stopRunLoop() {
  running = false;
  if (runTimer !== null) clearTimeout(runTimer);
  runTimer = null;
}

function runLoop() {
  if (!running) return;
  try {
    const result = call({ op: 'tick', ticks: 8, burst: 2000 });
    publishSnapshot(result);
    const stopped = result.snapshot?.nodes?.find((node) => node.stopped);
    if (stopped) {
      stopRunLoop();
      const reason = typeof stopped.stopped === 'string' ? stopped.stopped : 'unknown reason';
      postMessage({ type: 'error', error: `${stopped.id ?? 'node'} stopped: ${reason}` });
      return;
    }
  } catch (error) {
    stopRunLoop();
    postMessage({ type: 'error', error: String(error?.message ?? error) });
    return;
  }
  runTimer = setTimeout(runLoop, 0);
}

async function initialize(message) {
  stopRunLoop();
  const url = new URL(message.wasm ?? './firmverse.wasm', self.location.href);
  wasm = await instantiate(url);
  const registry = call({ op: 'registry' }).registry;
  const initial = call({
    op: 'new',
    world: message.world ?? 'mesh',
    looping: message.looping ?? true,
    strict: message.strict ?? true,
    maxInsns: message.maxInsns ?? 50_000_000,
  });
  postMessage({ type: 'ready', registry, snapshot: initial.snapshot });
}

self.onmessage = async (event) => {
  const message = event.data ?? {};
  try {
    switch (message.type) {
      case 'init':
        await initialize(message);
        return;
      case 'run':
        if (!running) {
          running = true;
          postMessage({ type: 'running', running: true });
          runLoop();
        }
        return;
      case 'stop':
        stopRunLoop();
        postMessage({ type: 'running', running: false });
        return;
      case 'step':
        publishSnapshot(call({ op: 'tick', ticks: message.ticks ?? 1, burst: message.burst ?? 2000 }));
        return;
      case 'reset': {
        stopRunLoop();
        call({ op: 'reset' });
        const result = call({
          op: 'new',
          world: message.world ?? 'mesh',
          looping: true,
          strict: true,
          maxInsns: 50_000_000,
        });
        postMessage({ type: 'running', running: false });
        publishSnapshot(result);
        return;
      }
      case 'addNode':
        publishSnapshot(call({
          op: 'addNode',
          id: message.id,
          board: message.board,
          label: message.label,
          firmware: message.firmware,
          x: message.x,
          y: message.y,
        }));
        return;
      case 'removeNode':
        publishSnapshot(call({ op: 'removeNode', id: message.id }));
        return;
      case 'moveNode':
        publishSnapshot(call({ op: 'moveNode', id: message.id, x: message.x, y: message.y }));
        return;
      case 'setWorld':
        publishSnapshot(call({ op: 'setWorld', world: message.world, looping: message.looping ?? true }));
        return;
      case 'pin':
        publishSnapshot(call({ op: 'pin', id: message.id, pin: message.pin, high: Boolean(message.high) }));
        return;
      case 'adc':
        publishSnapshot(call({ op: 'adc', id: message.id, values: message.values }));
        return;
      case 'snapshot':
        publishSnapshot(call({ op: 'snapshot' }));
        return;
      default:
        throw new Error(`Unknown worker message: ${message.type}`);
    }
  } catch (error) {
    postMessage({ type: 'error', error: String(error?.message ?? error) });
  }
};
