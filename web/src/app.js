import './elements.js';

const $ = (selector) => document.querySelector(selector);
const worker = new Worker('./engine-worker.js', { type: 'module' });

const state = {
  registry: null,
  snapshot: { world: { name: 'mesh', nowMs: 0 }, nodes: [] },
  firmware: null,
  firmwareName: null,
  selected: null,
  running: false,
  nextNode: 0,
  adc: new Map(),
};

const worldElement = $('#world');
const boardPreview = $('#board-preview');
const runtimeStatus = $('#runtime-status');
const firmwareInput = $('#firmware-input');
const firmwareDrop = $('#firmware-drop');
const firmwareName = $('#firmware-name');
const worldSelect = $('#world-select');
const boardSelect = $('#board-select');
const addNode = $('#add-node');
const runButton = $('#run');
const stepButton = $('#step');
const resetButton = $('#reset');

function selectedNode() {
  return state.snapshot.nodes.find((node) => node.id === state.selected) ?? null;
}

function setStatus(text, kind = '') {
  runtimeStatus.textContent = text;
  runtimeStatus.dataset.kind = kind;
}

function refresh() {
  worldElement.registry = state.registry;
  worldElement.snapshot = state.snapshot;
  worldElement.selected = state.selected;
  $('#metric-time').textContent = `${state.snapshot.world?.nowMs ?? 0} ms`;
  $('#metric-nodes').textContent = String(state.snapshot.nodes.length);
  runButton.textContent = state.running ? 'Stop' : 'Run';
  runButton.disabled = !state.registry || state.snapshot.nodes.length === 0;
  stepButton.disabled = !state.registry || state.snapshot.nodes.length === 0 || state.running;
  addNode.disabled = !state.registry || !state.firmware;
  refreshInspector();
}

function refreshInspector() {
  const node = selectedNode();
  $('#empty-inspector').hidden = Boolean(node);
  $('#node-inspector').hidden = !node;
  if (!node || !state.registry) return;

  $('#node-name').textContent = `${node.id} · ${node.board}`;
  $('#node-mac').textContent = node.mac;
  boardPreview.registry = state.registry;
  boardPreview.node = node;

  const pins = state.registry.pins?.[node.soc] ?? [];
  const pinControls = $('#pin-controls');
  pinControls.replaceChildren();
  for (const pin of pins) {
    const label = document.createElement('label');
    label.className = 'pin-switch';
    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.addEventListener('change', () => {
      worker.postMessage({ type: 'pin', id: node.id, pin: pin.label, high: checkbox.checked });
    });
    const text = document.createElement('span');
    text.textContent = pin.label;
    const note = state.registry.boards
      .find((board) => board.id === node.board)
      ?.pinNotes?.find((entry) => entry.pin === pin.label)?.note;
    if (note) text.title = note;
    label.append(checkbox, text);
    pinControls.append(label);
  }

  const adcControls = $('#adc-controls');
  adcControls.replaceChildren();
  const adcPins = pins.filter((pin) => pin.adcChannel !== null && pin.adcChannel !== undefined);
  const values = state.adc.get(node.id) ?? [0, 0, 0, 0];
  state.adc.set(node.id, values);
  for (const pin of adcPins.sort((a, b) => a.adcChannel - b.adcChannel)) {
    const label = document.createElement('label');
    label.innerHTML = `<span>${pin.label} / ADC${pin.adcChannel}</span>`;
    const input = document.createElement('input');
    input.type = 'range';
    input.min = '0';
    input.max = '3300';
    input.step = '10';
    input.value = String(values[pin.adcChannel]);
    const output = document.createElement('output');
    output.textContent = `${input.value} mV`;
    input.addEventListener('input', () => {
      values[pin.adcChannel] = Number(input.value);
      output.textContent = `${input.value} mV`;
    });
    input.addEventListener('change', () => {
      worker.postMessage({ type: 'adc', id: node.id, values: [...values] });
    });
    label.append(input, output);
    adcControls.append(label);
  }

  $('#uart-log').textContent = (node.uart ?? []).join('\n') || '—';
}

function populateRegistry() {
  worldSelect.replaceChildren();
  for (const world of state.registry.worlds) {
    const option = new Option(world.id, world.id);
    option.title = world.description;
    worldSelect.add(option);
  }
  worldSelect.value = state.snapshot.world?.name ?? 'mesh';

  boardSelect.replaceChildren();
  for (const board of state.registry.boards) {
    const option = new Option(board.name, board.id);
    option.disabled = !board.implemented;
    option.title = board.description;
    boardSelect.add(option);
  }
  boardSelect.value = state.registry.boards.find((board) => board.implemented)?.id ?? '';
}

async function acceptFirmware(file) {
  if (!file) return;
  state.firmware = await file.text();
  state.firmwareName = file.name;
  firmwareName.textContent = `${file.name} · ${(file.size / 1024).toFixed(1)} KiB`;
  firmwareDrop.dataset.ready = 'true';
  refresh();
}

firmwareInput.addEventListener('change', () => acceptFirmware(firmwareInput.files?.[0]));
for (const type of ['dragenter', 'dragover']) {
  firmwareDrop.addEventListener(type, (event) => {
    event.preventDefault();
    firmwareDrop.dataset.drag = 'true';
  });
}
for (const type of ['dragleave', 'drop']) {
  firmwareDrop.addEventListener(type, (event) => {
    event.preventDefault();
    delete firmwareDrop.dataset.drag;
  });
}
firmwareDrop.addEventListener('drop', (event) => acceptFirmware(event.dataTransfer?.files?.[0]));

addNode.addEventListener('click', () => {
  if (!state.firmware) return;
  let id;
  do id = `n${state.nextNode++}`; while (state.snapshot.nodes.some((node) => node.id === id));
  const index = state.snapshot.nodes.length;
  const x = (index % 4) * 3 - 4.5;
  const y = Math.floor(index / 4) * 2.5;
  state.selected = id;
  worker.postMessage({
    type: 'addNode',
    id,
    board: boardSelect.value,
    label: state.firmwareName ?? 'firmware.hex',
    firmware: state.firmware,
    x,
    y,
  });
});

runButton.addEventListener('click', () => worker.postMessage({ type: state.running ? 'stop' : 'run' }));
stepButton.addEventListener('click', () => worker.postMessage({ type: 'step', ticks: 1, burst: 4000 }));
resetButton.addEventListener('click', () => {
  state.selected = null;
  state.nextNode = 0;
  state.adc.clear();
  worker.postMessage({ type: 'reset', world: worldSelect.value || 'mesh' });
});
worldSelect.addEventListener('change', () => {
  worker.postMessage({ type: 'setWorld', world: worldSelect.value, looping: true });
});

let moveTimer = null;
let pendingMove = null;
worldElement.addEventListener('select-node', (event) => {
  state.selected = event.detail.id;
  refresh();
});
worldElement.addEventListener('move-node', (event) => {
  pendingMove = event.detail;
  if (moveTimer !== null) return;
  moveTimer = setTimeout(() => {
    worker.postMessage({ type: 'moveNode', ...pendingMove });
    pendingMove = null;
    moveTimer = null;
  }, 32);
});

worker.onmessage = (event) => {
  const message = event.data ?? {};
  switch (message.type) {
    case 'ready':
      state.registry = message.registry;
      state.snapshot = message.snapshot;
      setStatus('WASM / zmu ready', 'ready');
      populateRegistry();
      refresh();
      break;
    case 'snapshot':
      state.snapshot = message.snapshot;
      if (state.selected && !selectedNode()) state.selected = null;
      refresh();
      break;
    case 'running':
      state.running = Boolean(message.running);
      refresh();
      break;
    case 'error':
      state.running = false;
      setStatus(message.error, 'error');
      console.error(message.error);
      refresh();
      break;
  }
};

worker.onerror = (event) => {
  setStatus(event.message || 'Worker failed', 'error');
};

worker.postMessage({ type: 'init', wasm: './firmverse.wasm', world: 'mesh', strict: true });
