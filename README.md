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
phy6252 --raw                  # GPIO / UART / FRAME lines
phy6252 --once path.hex        # no REPL, stop at halt / insn cap
```

Install: `cargo install --path .` → `phy6252` on your PATH.

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
