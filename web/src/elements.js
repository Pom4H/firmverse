const esc = (value) => String(value ?? '')
  .replaceAll('&', '&amp;')
  .replaceAll('<', '&lt;')
  .replaceAll('>', '&gt;')
  .replaceAll('"', '&quot;');

export class FirmverseBoard extends HTMLElement {
  #registry = null;
  #node = null;

  constructor() {
    super();
    this.attachShadow({ mode: 'open' });
  }

  set registry(value) {
    this.#registry = value;
    this.render();
  }

  set node(value) {
    this.#node = value;
    this.render();
  }

  get node() { return this.#node; }

  connectedCallback() { this.render(); }

  render() {
    if (!this.shadowRoot) return;
    const node = this.#node;
    const boardId = node?.board ?? this.getAttribute('board') ?? 'pb03f-kit';
    const board = this.#registry?.boards?.find((entry) => entry.id === boardId);
    if (!board) {
      this.shadowRoot.innerHTML = `<style>:host{display:block}</style><div>Board ${esc(boardId)}</div>`;
      return;
    }

    const rows = board.connectorRows ?? [];
    const rowHeight = 24;
    const bodyX = 72;
    const bodyW = 196;
    const top = 34;
    const height = Math.max(128, rows.length * rowHeight + 48);
    const pins = rows.map((row, index) => {
      const y = top + index * rowHeight + 14;
      return `
        <g class="pin-row">
          <circle cx="48" cy="${y}" r="5" class="pad"/>
          <path d="M53 ${y} H72" class="trace"/>
          <text x="39" y="${y + 4}" text-anchor="end">${esc(row.left)}</text>
          <circle cx="292" cy="${y}" r="5" class="pad"/>
          <path d="M268 ${y} H287" class="trace"/>
          <text x="301" y="${y + 4}">${esc(row.right)}</text>
        </g>`;
    }).join('');

    const indicators = (board.indicators ?? []).map((indicator, index) => {
      const live = node?.indicators?.find((entry) => entry.name === indicator.name);
      const x = bodyX + 24 + index * 34;
      const active = Boolean(live?.active);
      return `
        <g class="indicator ${active ? 'active' : ''}" transform="translate(${x} 17)">
          <circle r="7" data-color="${esc(indicator.name)}"/>
          <title>${esc(indicator.name)} · ${esc(indicator.pin)}</title>
        </g>`;
    }).join('');

    const symbol = this.getAttribute('detail') === 'symbol';
    this.shadowRoot.innerHTML = `
      <style>
        :host { display:block; min-width:0; color:#dce6e9; font-family:ui-monospace,SFMono-Regular,Menlo,monospace; }
        svg { width:100%; height:auto; display:block; overflow:visible; }
        .pcb { fill:#183a32; stroke:#64a894; stroke-width:1.5; }
        .pcb-inner { fill:none; stroke:#2d5c50; stroke-width:1; stroke-dasharray:3 4; }
        .chip { fill:#11181a; stroke:#506368; stroke-width:1.2; }
        .chip-mark { fill:#9fb0b4; font-size:11px; font-weight:700; letter-spacing:.04em; }
        .label { fill:#e8f1f2; font-size:12px; font-weight:700; }
        .sub { fill:#789095; font-size:9px; }
        .pad { fill:#d5b95c; stroke:#fff0a0; stroke-width:.7; }
        .trace { stroke:#80b2a4; stroke-width:1; }
        .pin-row text { fill:#9fb0b4; font-size:9px; }
        .indicator circle { fill:#263438; stroke:#71868b; stroke-width:1; transition:fill 80ms ease,filter 80ms ease; }
        .indicator.active circle { fill:#f5f7f1; filter:drop-shadow(0 0 4px #fff); }
        .indicator.active circle[data-color="red"] { fill:#ff5d62; filter:drop-shadow(0 0 5px #ff5d62); }
        .indicator.active circle[data-color="green"] { fill:#62e59b; filter:drop-shadow(0 0 5px #62e59b); }
        .indicator.active circle[data-color="blue"] { fill:#61a8ff; filter:drop-shadow(0 0 5px #61a8ff); }
        .indicator.active circle[data-color="yellow"] { fill:#ffd75d; filter:drop-shadow(0 0 5px #ffd75d); }
        .indicator.active circle[data-color="white"] { fill:#fff; filter:drop-shadow(0 0 5px #fff); }
        ${symbol ? '.pin-row,.sub{display:none}.pcb-inner{display:none}' : ''}
      </style>
      <svg viewBox="0 0 340 ${height}" role="img" aria-label="${esc(board.name)}">
        <rect class="pcb" x="${bodyX}" y="4" width="${bodyW}" height="${height - 8}" rx="14"/>
        <rect class="pcb-inner" x="84" y="40" width="172" height="${height - 66}" rx="8"/>
        <rect class="chip" x="126" y="${Math.max(58, height / 2 - 30)}" width="88" height="60" rx="4"/>
        <text class="chip-mark" x="170" y="${Math.max(58, height / 2 - 30) + 28}" text-anchor="middle">${esc(board.soc.toUpperCase())}</text>
        <text class="label" x="84" y="26">${esc(node?.id ?? board.name)}</text>
        <text class="sub" x="84" y="${height - 15}">${esc(node?.mac ?? board.description)}</text>
        ${indicators}
        ${pins}
      </svg>`;
  }
}

export class FirmverseWorld extends HTMLElement {
  #registry = null;
  #snapshot = { world: { name: 'mesh', nowMs: 0 }, nodes: [] };
  #selected = null;
  #drag = null;
  #scale = 68;

  constructor() {
    super();
    this.attachShadow({ mode: 'open' });
  }

  set registry(value) {
    this.#registry = value;
    this.render();
  }

  set snapshot(value) {
    this.#snapshot = value ?? this.#snapshot;
    this.render();
  }

  set selected(value) {
    this.#selected = value;
    this.render();
  }

  connectedCallback() { this.render(); }

  #screen(node, rect) {
    return {
      x: rect.width / 2 + node.x * this.#scale,
      y: rect.height / 2 + node.y * this.#scale,
    };
  }

  #world(clientX, clientY, rect) {
    return {
      x: (clientX - rect.left - rect.width / 2) / this.#scale,
      y: (clientY - rect.top - rect.height / 2) / this.#scale,
    };
  }

  render() {
    if (!this.shadowRoot) return;
    const nodes = this.#snapshot?.nodes ?? [];
    this.shadowRoot.innerHTML = `
      <style>
        :host { display:block; width:100%; height:100%; min-height:620px; position:relative; user-select:none; }
        .canvas { position:absolute; inset:0; overflow:hidden; border-radius:18px; background:
          radial-gradient(circle at center,rgba(83,119,125,.14),transparent 48%),
          linear-gradient(rgba(115,141,146,.08) 1px,transparent 1px),
          linear-gradient(90deg,rgba(115,141,146,.08) 1px,transparent 1px),#0d1416;
          background-size:auto,34px 34px,34px 34px,auto; }
        svg.links { position:absolute; inset:0; width:100%; height:100%; pointer-events:none; overflow:visible; }
        .axis { stroke:#4c6267; stroke-width:1; opacity:.35; }
        .link { stroke:#73d7ba; stroke-width:2; fill:none; stroke-dasharray:5 7; animation:flow 1.1s linear infinite; }
        .link-label { fill:#95bdb3; font:10px ui-monospace,monospace; paint-order:stroke; stroke:#0d1416; stroke-width:4px; }
        @keyframes flow { to { stroke-dashoffset:-24; } }
        .node { position:absolute; width:158px; transform:translate(-50%,-50%); padding:8px; border:1px solid #2e4348; border-radius:14px; background:rgba(15,24,27,.92); box-shadow:0 12px 28px rgba(0,0,0,.25); cursor:grab; touch-action:none; }
        .node.selected { border-color:#8ae0c7; box-shadow:0 0 0 2px rgba(138,224,199,.15),0 14px 30px rgba(0,0,0,.28); }
        .node:active { cursor:grabbing; }
        .node-head { display:flex; align-items:center; justify-content:space-between; gap:8px; font:11px ui-monospace,monospace; color:#d9e5e7; margin:0 2px 4px; }
        .node-head span:last-child { color:#789096; }
        firmverse-board { pointer-events:none; }
        .legend { position:absolute; left:18px; bottom:16px; color:#6d858a; font:10px ui-monospace,monospace; }
        .world-name { position:absolute; right:18px; top:16px; color:#6f898e; font:11px ui-monospace,monospace; letter-spacing:.08em; text-transform:uppercase; }
      </style>
      <div class="canvas">
        <svg class="links"><g class="link-layer"></g></svg>
        <div class="nodes"></div>
        <div class="world-name">${esc(this.#snapshot?.world?.name)} · ${esc(this.#snapshot?.world?.nowMs)} ms</div>
        <div class="legend">1 grid = 0.5 m · drag = real World coordinates</div>
      </div>`;

    const canvas = this.shadowRoot.querySelector('.canvas');
    const nodeLayer = this.shadowRoot.querySelector('.nodes');
    const linkLayer = this.shadowRoot.querySelector('.link-layer');
    const rect = canvas.getBoundingClientRect();

    for (const node of nodes) {
      const point = this.#screen(node, rect);
      const card = document.createElement('div');
      card.className = `node${node.id === this.#selected ? ' selected' : ''}`;
      card.dataset.id = node.id;
      card.style.left = `${point.x}px`;
      card.style.top = `${point.y}px`;
      card.innerHTML = `<div class="node-head"><strong>${esc(node.id)}</strong><span>${Number(node.x).toFixed(1)}, ${Number(node.y).toFixed(1)} m</span></div>`;
      const board = document.createElement('firmverse-board');
      board.setAttribute('detail', 'symbol');
      board.registry = this.#registry;
      board.node = node;
      card.append(board);
      card.addEventListener('pointerdown', (event) => {
        card.setPointerCapture(event.pointerId);
        this.#selected = node.id;
        this.#drag = { id: node.id, pointerId: event.pointerId };
        this.dispatchEvent(new CustomEvent('select-node', { detail: { id: node.id }, bubbles: true }));
      });
      card.addEventListener('pointermove', (event) => {
        if (!this.#drag || this.#drag.pointerId !== event.pointerId) return;
        const world = this.#world(event.clientX, event.clientY, canvas.getBoundingClientRect());
        const snap = event.altKey ? 0 : 0.25;
        const x = snap ? Math.round(world.x / snap) * snap : world.x;
        const y = snap ? Math.round(world.y / snap) * snap : world.y;
        card.style.left = `${event.clientX - canvas.getBoundingClientRect().left}px`;
        card.style.top = `${event.clientY - canvas.getBoundingClientRect().top}px`;
        this.dispatchEvent(new CustomEvent('move-node', { detail: { id: node.id, x, y }, bubbles: true }));
      });
      card.addEventListener('pointerup', () => { this.#drag = null; });
      card.addEventListener('pointercancel', () => { this.#drag = null; });
      nodeLayer.append(card);
    }

    const byId = new Map(nodes.map((node) => [node.id, node]));
    const drawn = new Set();
    for (const node of nodes) {
      for (const heard of node.heard ?? []) {
        if (!heard.nodeId || !byId.has(heard.nodeId)) continue;
        const key = [node.id, heard.nodeId].sort().join('::');
        if (drawn.has(key)) continue;
        drawn.add(key);
        const other = byId.get(heard.nodeId);
        const a = this.#screen(node, rect);
        const b = this.#screen(other, rect);
        const strength = Math.max(.18, Math.min(1, (Number(heard.rssi) + 90) / 55));
        const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
        line.setAttribute('class', 'link');
        line.setAttribute('x1', a.x); line.setAttribute('y1', a.y);
        line.setAttribute('x2', b.x); line.setAttribute('y2', b.y);
        line.setAttribute('opacity', strength);
        linkLayer.append(line);
        const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
        text.setAttribute('class', 'link-label');
        text.setAttribute('x', (a.x + b.x) / 2 + 6);
        text.setAttribute('y', (a.y + b.y) / 2 - 6);
        text.textContent = `${heard.rssi} dBm`;
        linkLayer.append(text);
      }
    }
  }
}

customElements.define('firmverse-board', FirmverseBoard);
customElements.define('firmverse-world', FirmverseWorld);
