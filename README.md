# PHY6252 emulator

Cortex-M0 emulator for **PHY6252** (AI-Thinker **PB-03F-Kit**). One Rust CLI: `phy6252`.

```sh
git clone --recurse-submodules https://github.com/Pom4H/phy6252-emu.git
cd phy6252-emu
cargo run --release
```

```text
connect
write hi
adc 3.3 1.65 2.5 3.3
p34 on
help
```

`p34 on` drives silkscreen P34. The demo lights every RGB LED while that pad is high. `connect` is the BLE link (ATT mailbox in the hex — there is no vendor BLE ROM here).

```sh
phy6252 --help
phy6252 firmware/kit-demo.hex
phy6252 --tui                  # realtime pinout + logs dashboard
phy6252 --raw                  # GPIO / UART / FRAME lines
phy6252 --once path.hex        # no REPL, stop at halt / insn cap
phy6252 --strict --once path.hex # stop on the first missing silicon behavior
```

`--strict-mmio` remains an alias for `--strict`.

Install: `cargo install --path .` → `phy6252` on your PATH.

## Realtime terminal dashboard

`phy6252 --tui [firmware.hex]` runs the exact same emulator through its stable `--raw` protocol and renders a live terminal dashboard instead of creating a second execution path.

The dashboard shows:

- PB-03F silkscreen pin → `gpio_pin_e` mapping for P0, P2, P3, P7, P11, P14, P15, P16, P17, P18, P20, P23, P24, P31, P32, P33 and P34;
- GPIO direction and current level (`OUT` reads DR, `IN` reads the externally driven level);
- live ADC voltages for P20 / P15 / P24 / P23;
- RGB/W LED levels and all six PWM channels;
- BLE mailbox link + notify state;
- a rolling log combining UART, ATT frames and emulator stderr, including strict discovery, ROM shims, power state and secure-boot diagnostics;
- command history with Up/Down and the same commands as the normal REPL.

Example commands can be typed directly into the TUI:

```text
connect
notify on
adc 3.3 1.65 2.5 3.3
p34 on
write 01020304
```

Esc or Ctrl-C exits. `--tui` is intentionally separate from `--raw`: automation keeps the line protocol, while humans get the dashboard.

## Firmware-driven silicon discovery

The emulator can use a real firmware image as a probe for missing PHY6252 behavior instead of silently pretending unsupported silicon works.

### Relocated vectors

PHY6252 SDK images do not have to place the Cortex-M vector table at the first byte of SRAM. The emulator scans SRAM for a plausible vector table, keeps the image at its real addresses, and mirrors only the selected SP/reset pair to address zero for zmu reset.

This allows images with jump/config areas before `.vectors` to reach their actual reset handler without rewriting the firmware.

### MMIO discovery

By default, an MMIO access that is not modeled is kept in a sparse register store keyed by the full 32-bit address and reported once on stderr:

```text
MMIO unknown write32 addr=0x40012340 aligned=0x40012340 -- sparse stub
```

This replaces the old modulo-1024 fallback, where unrelated peripheral addresses could alias the same backing cell.

With `--strict`, an unknown MMIO access raises `DAccViol` instead of being silently accepted:

```sh
phy6252 --strict --once vendor-firmware.hex
```

Functional GPIO/UART/ADC/PWM/timer registers are delegated to the current PHY6252 model. Bootstrap registers that are intentionally inert are whitelisted by exact address, not by broad peripheral range, so the next missing register remains observable.

### Vendor ROM discovery

The repository does not contain the PHY6252 vendor ROM. In strict mode, execution therefore stops at the first unmodeled ROM entry rather than executing an all-zero placeholder until the end of the ROM address range:

```text
ROM unknown read16 addr=0x0000abcd -- vendor ROM image/ABI not modeled; strict fault
```

Known ROM ABI functions are replaced only when their semantics are identified. Current executable shims cover bootstrap IRQ init, sleep policy, eFuse reads, AES-128/secure identity checks, ARM EABI memory clearing, OSAL memory comparison and OSAL heap setup. The goal is to execute the SDK contract, not to hide unknown ROM behind blanket no-op returns.

The intended workflow is iterative:

1. boot the real HEX with `--strict`;
2. stop at the first unknown MMIO register or ROM entry;
3. identify it from the SDK/link map;
4. model the required behavior or add a narrowly documented shim;
5. run the same firmware again and repeat.

That turns the firmware itself into an executable checklist for the missing PHY6252 model.

## Demo firmware

Bare-metal C, no vendor SDK. `make -C firmware` if you change it; `firmware/kit-demo.hex` is in the tree.

| Block | Kit | Demo |
|---|---|---|
| RGB | P7 / P11 / P18 | chase; green while BLE connected |
| Warm LED | P0 | fourth RGB phase |
| Header GPIO | P14 P16 P17 P31 P32 P33 | walking one |
| Button | P34 | all LEDs on |
| ADC | P20 P15 P24 P23 | `adc …` |
| UART0 / UART1 | TX | status / heartbeat |
| PWM, timer, WDT, I2C0, SPI0, PCR, AON | MMIO | poked |
| ATT mailbox | SRAM `0x20000000` | `connect` / `write` / notifies |

Machine protocol: [PROTOCOL.md](PROTOCOL.md) (`phy6252 --raw`).
