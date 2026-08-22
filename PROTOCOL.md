# Line protocol

`phy6252 --raw` speaks UTF-8 lines. The REPL (`phy6252` with no flags) accepts the same words plus `connect`, `write hi`, `uart 0 9600 UXTDWU`, `p34 on`, `adc 3.3 …`.

```text
phy6252 [--raw] [--once] [--strict] [--max-insns N] [firmware.hex]
phy6252 sim [--raw] [--once] [--ticks N] [--world NAME] [--node id[@x,y]=firmware.hex]…
phy6252 worlds
```

Default image: `firmware/kit-demo.hex`, or `PHY6252_HEX`. Default is live. `--once` runs until halt or the insn cap.

`phy6252 sim` shares one millisecond clock across every `--node`. Two or more chips default to world `mesh`; one chip defaults to `crowd`. Live runs wrap the world timeline; `--once --ticks N` is a finite scripted run (add `--loop` to wrap walkers). With more than one chip, stdout lines are tagged `[id]`. Stdin `a scan …` or `[b] gone …` targets one node; unprefixed commands go to every chip.

`--strict` turns the emulator into a silicon-discovery runner: the first unmodeled MMIO register or vendor-ROM entry faults instead of being silently accepted. `--strict-mmio` remains an alias for compatibility. Outside strict mode, unknown MMIO registers use a sparse full-address backing store and are reported on stderr.

## Stdin (host → chip)

| Line | Meaning |
|---|---|
| `IN <hex32>` | `AP_GPIO` external bits (`gpio_pin_e`, not silk numbers). |
| `WRITE <hex\|text>` | ATT payload into the mailbox RX buffer. |
| `UART <port> <baud> <hex\|text>` | Host bytes into a modeled UART receive path. Currently UART0 is accepted while the PHY6252 ROM programmer is active. |
| `SCAN <mac> <rssi>` | BLE advertiser seen (mailbox scan report). MAC is `aa:bb:cc:dd:ee:ff` or 12 hex digits; RSSI is signed decimal. |
| `GONE <mac>` | Advertiser left range / disconnected. |
| `CONNECT` / `DISCONNECT` | Link flag. |
| `CCCD <n>` | Notify enable (`n != 0`). |
| `TICK <ms>` | Mailbox `tick_ms`. |
| `ADC <p20> <p15> <p24> <p23>` | Millivolts (or volts with a `.`). |

Kit pads use `gpio_pin_e` bits, not silk numbers:

| Silk | Bit | Mask | Role |
|---|---|---|---|
| P7 | 4 | `00000010` | red LED |
| P11 | 7 | `00000080` | green LED |
| P18 | 12 | `00001000` | blue LED |
| P0 | 0 | `00000001` | yellow / warm LED |
| P34 | 22 | `00400000` | white / cool LED |
| P15 | 9 | `00000200` | Restore (`p15 on`) |

P13 is on the DIP header but has no `gpio_pin_e` bit. P14 is a header pin, not an LED.

## Stdout (`--raw`)

| Line | Meaning |
|---|---|
| `READY` | Live. |
| `ADV name=… service=…` | Demo GAP name / service UUID (ATT is the mailbox). |
| `GPIO <dr> <ddr>` | `swporta_dr` / `ddr`. |
| `PWM <c0> … <c5>` | PWM duty. |
| `UART <text>` | UART0/1 line. ROM programmer state/replies are emitted as `UART ROM0@<baud> …`. |
| `FRAME <HEXBYTES>` | Mailbox TX. |
| `STOP <reason>` | Exit. |
| `WORLD <name> loop=<0|1> nodes=<n>` | Sim start (`phy6252 sim --raw`). |
| `NODE <id> mac=… x=… y=… hex=…` | One sim chip. |
| `[id] GPIO …` / `[id] UART …` | Same as GPIO/UART, tagged when several chips run. |

### PHY6252 ROM UART sequence

When guest firmware performs a CMSIS `NVIC_SystemReset()` (`SCB.AIRCR.SYSRESETREQ` with `VECTKEY`), the PHY6252 model enters the ROM programmer window instead of immediately executing the application reset vector:

```text
UART ROM0@9600 await UXTDWU
```

The host can then drive the real entry handshake through the line protocol:

```text
UART 0 9600 UXTDWU
```

and receives:

```text
UART ROM0@9600 cmd>>:
```

The modeled command monitor then expects 115200 baud. `UART 0 115200 reset` exits the ROM monitor and restarts the application. Other ROM programmer commands are deliberately not synthesized yet.

Strict discovery diagnostics are written to stderr, for example:

```text
MMIO unknown read32 addr=0x4000f03c aligned=0x4000f03c -- strict fault
ROM shim drv_irq_init entry=0x0000a9c8 behavior=noop-return
ROM unknown read16 addr=0x0000aeac -- vendor ROM image/ABI not modeled; strict fault
```

Mailbox at `0x20000000`:

```text
magic u32     = 0x50485932 ("PHY2")
status u32    bit0 connected, bit1 notify
rx_seq, rx_len, rx[256]
tx_seq, tx_len, tx[256]
tick_ms u32
```
