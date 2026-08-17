# Line protocol

UTF-8, one command or event per line, `\n` terminated. This is the only public API.

## Invoke

```text
phy6252-emu [--live] [--max-insns N] [firmware.hex]
```

| | |
|---|---|
| `firmware.hex` | Intel HEX. Overrides `PHY6252_HEX`. Default: `firmware/build/kit-demo.hex`. |
| `--live` | Stream GPIO / UART / PWM / FRAME; read stdin. Alias: `--gpio`. |

Without `--live` the process steps until `--max-insns` (default 2e6) and prints a stop report.

## Stdin (host → chip)

Unknown lines are ignored.

| Line | Meaning |
|---|---|
| `IN <hex32>` | External input bits on `AP_GPIO` (`ext`), masked to 23 `gpio_pin_e` bits. |
| `WRITE <hexbytes>` | Copy payload into the radio mailbox RX buffer and bump `rx_seq`. |
| `CONNECT` | Mailbox `status` bit 0 (connected). |
| `DISCONNECT` | Clear connected + notify. |
| `CCCD <n>` | Notify enable (`n != 0` sets mailbox `status` bit 1). |
| `TICK <ms>` | Add milliseconds to mailbox `tick_ms`. |
| `ADC <p20> <p15> <p24> <p23>` | Millivolts for kit analog pads. Written to `ADC_CH_BASE` channels 7, 6, 4, 3. |

`gpio_pin_e` bit is **not** the silkscreen number. P34 is bit 22 (`IN 00400000`). P7 is bit 4, P11 bit 7, P18 bit 12.

## Stdout (chip → host)

| Line | Meaning |
|---|---|
| `hex <path>` | Image loaded (stderr in `--live`). |
| `READY` | Live pump is running. |
| `ADV name=… service=…` | BLE advertisement the host radio should use. |
| `GPIO <dr> <ddr>` | `AP_GPIO->swporta_dr` and `swporta_ddr`, 8 hex digits. |
| `PWM <c0> … <c5>` | Duty of six PWM slots, 4 hex digits each. |
| `UART <text>` | One line written to UART0 or UART1 THR. |
| `FRAME <HEXBYTES>` | Mailbox TX payload (uppercase, no spaces). |
| `STOP <reason>` | Live process ending. |

The radio mailbox lives in Cortex-M SRAM at `0x20000000` (the window zmu uses for RAM). Layout:

```text
magic u32     = 0x50485932 ("PHY2")
status u32    bit0 connected, bit1 notify
rx_seq, rx_len, rx[256]
tx_seq, tx_len, tx[256]
tick_ms u32
```

The demo firmware echoes `WRITE` payloads on `FRAME` and, while notify is on, sends a 12-byte snapshot about every 128 ms of `tick_ms`. On connect it logs `ble up` and holds the green LED.

## BLE air

`bash scripts/air.sh` (macOS) runs CoreBluetooth as the 2.4 GHz PHY:

| BLE host | emu stdin / stdout |
|---|---|
| advertise `PB03FKIT` | `ADV …` |
| central connects + CCCD | `CONNECT` then `CCCD 1` |
| GATT write RX | `WRITE <hex>` |
| `FRAME <hex>` | GATT notify TX |
| disconnect | `DISCONNECT` |

UUIDs: service `6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001`, RX `…0002…`, TX `…0003…`.
