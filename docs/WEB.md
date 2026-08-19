# Browser Lab

Firmverse can execute firmware and multi-node Worlds directly in a browser. The browser frontend is deliberately **not** a second emulator implementation.

```text
                    same Rust core
                         │
            ┌────────────┴────────────┐
            │                         │
       native CLI                 wasm32 cdylib
            │                         │
       terminal/host              raw JSON ABI
                                      │
                                 Web Worker
                                      │
                          ┌───────────┴───────────┐
                          │                       │
                  <firmverse-board>       <firmverse-world>
```

PHY6252 instructions still execute through the existing `jjkt/zmu` Cortex-M backend. SoC MMIO, ROM shims, Board metadata and World RF behavior are the same code used by native Firmverse and CI.

## Ideas borrowed from the other Pom4H projects

The browser structure follows two patterns that already work well elsewhere in the account:

- **Bolid:** keep the application/runtime platform-neutral and make the browser layer a thin adapter for browser-only capabilities such as storage, clocks and transports. Firmverse applies the same rule to firmware loading and execution: the browser reads a `File` into memory, then calls the same Rust `Chip` runtime instead of emulating a filesystem or creating a JavaScript MCU.
- **elements:** describe a device through registry metadata, then let generic UI consume that metadata. Firmverse exports Board profiles, SoC profiles, connector rows, indicators and package pin/ADC metadata from Rust. The custom elements render that registry instead of maintaining a second pinout in JavaScript.

Those are architectural ideas, not source-code copies.

## Build

The browser artifact is a static directory; no Node package install or bundler is required.

```sh
bash tools/build_web.sh
```

This produces:

```text
web/dist/
  index.html
  styles.css
  app.js
  elements.js
  engine-worker.js
  firmverse.wasm
```

Serve the directory over HTTP because browsers do not allow all Worker/WASM features from `file://` URLs:

```sh
python3 -m http.server 8080 -d web/dist
```

Then open `http://localhost:8080`.

## Firmware stays local

The browser uses `File.text()` / drag-and-drop to read Intel HEX into memory. `HexImage::parse()` feeds those bytes into the same PHY6252 loader used by `Chip::load()`.

There is no upload API in the Browser Lab. A static deployment can therefore run firmware locally without sending the image to a Firmverse server.

## Worker boundary

CPU execution happens in `engine-worker.js`, not the main UI thread. The Worker instantiates `firmverse.wasm` and speaks to a small raw ABI:

```text
firmverse_input_reserve(len)
firmverse_call(len)
firmverse_result_ptr()
firmverse_result_len()
```

Requests and responses are UTF-8 JSON stored in WebAssembly linear memory. The JS layer does not depend on Rust struct layout or wasm-bindgen-generated glue.

Current operations are:

- `registry`
- `new`
- `reset`
- `addNode`
- `removeNode`
- `moveNode`
- `setWorld`
- `pin`
- `adc`
- `tick`
- `snapshot`

This gives the ABI room to remain stable while Rust internals continue to move under `soc/`.

## Registry is the UI manifest

The `registry` response is generated from core definitions at runtime:

- `board::PROFILES`
- `soc::PROFILES`
- `soc::phy6252::pins::PINS`
- `World::list()`

A browser board element therefore does not contain a PB-03F pin table. For example, connector rows and LED semantics come from `BoardProfile`, while `P15 → GPIO9 → ADC1` remains a PHY6252 package fact.

The same rule should be kept for future boards: **add metadata to the Board/SoC model, not a special case to the browser renderer.**

## `<firmverse-board>`

`<firmverse-board>` is a functional board visualization generated from the registry. It currently renders:

- board/SoC identity;
- physical connector rows;
- package pin labels;
- board indicators;
- live LED state from emulated GPIO;
- firmware/node identity.

The World uses a compact `detail="symbol"` view; the inspector uses the full connector view.

## `<firmverse-world>`

`<firmverse-world>` is the first visual World editor. It is not a decorative topology diagram.

Each node card is backed by a real `BrowserLab` node. Dragging the card changes `Chip.x` / `Chip.y` through `moveNode`. The next World tick runs the existing RF model against those new coordinates.

As a result:

```text
move board
   ↓
real World x/y changes
   ↓
distance changes
   ↓
World::radio recalculates RSSI
   ↓
firmware receives Scan/Gone
   ↓
visual link/RSSI changes
```

Links are drawn from the live `heard` snapshot, not inferred by the canvas itself. If the RF model says two nodes cannot hear one another, the line disappears.

## Inspector

The browser inspector is also metadata-driven. It currently exposes:

- external digital input switches generated from the SoC pin registry;
- ADC sliders generated from pins that declare an ADC channel;
- full Board visualization with live indicators;
- UART output;
- node MAC, position, firmware and stop state.

The intended direction is the same as the metadata editor in `elements`: new controllable Board/SoC properties should become inspectable through registry metadata instead of one-off UI code.

## CI proof

The integration smoke job builds `wasm32-unknown-unknown`, checks JavaScript syntax, then instantiates the final `.wasm` in Node without browser shims.

The smoke goes beyond `registry`:

1. creates a Browser Lab;
2. loads a tiny Cortex-M0 Intel HEX from memory;
3. executes instructions through `zmu` inside WebAssembly;
4. creates a second node in the same mesh World;
5. proves both nodes hear each other;
6. moves one node far away;
7. proves the RF link disappears.

That test prevents a future refactor from leaving behind a WASM file that compiles but cannot actually execute firmware or a multi-node World.

## Current scope

Implemented in the first browser slice:

- PHY6252/zmu execution in WebAssembly;
- multiple firmware nodes;
- `mesh`, `still` and `crowd` World selection;
- drag-to-position editor;
- live RSSI links;
- PB-03F metadata-driven visualization;
- GPIO/ADC interaction;
- UART/indicator inspection;
- static build artifact with no backend requirement.

Not implemented yet:

- CH592F execution (the native SoC backend does not exist yet either);
- arbitrary custom walls/attenuation/noise objects;
- editable virtual advertisers from the canvas;
- browser-side persistent NOR/localStorage adapter;
- import/export of complete World scenario files.

Those additions belong to the World/platform adapter layers; they should not leak into the CPU or PHY6252 implementation.
