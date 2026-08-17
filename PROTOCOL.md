# Line protocol

`phy6252 --raw` speaks UTF-8 lines. The REPL (`phy6252` with no flags) accepts the same words plus `connect`, `write hi`, `p34 on`, `adc 3.3 …`.

```text
phy6252 [--raw] [--once] [--strict-mmio] [--max-insns N] [firmware.hex]
```

Default image: `firmware/kit-demo.hex`, or `PHY6252_HEX`. Default is live. `--once` runs until halt or the insn cap. `--strict-mmio` faults on the first MMIO register that is neither modeled nor an explicit stub; without it, unknown registers use a sparse full-address backing store and are reported on stderr.

## Stdin (host → chip)

| Line | Meaning |
|---|---|
| `IN <hex32>` | `AP_GPIO` external bits (`gpio_pin_e`, not silk numbers). |
| `WRITE <hex\|text>` | ATT payload into the mailbox RX buffer. |
| `CONNECT` / `DISCONNECT` | Link flag. |
| `CCCD <n>` | Notify enable (`n != 0`). |
| `TICK <ms>` | Mailbox `tick_ms`. |
| `ADC <p20> <p15> <p24> <p23>` | Millivolts (or volts with a `.`). |

P34 is bit 22 (`IN 00400000` or `p34 on`).

## Stdout (`--raw`)

| Line | Meaning |
|---|---|
| `READY` | Live. |
| `ADV name=… service=…` | Demo GAP name / service UUID (ATT is the mailbox). |
| `GPIO <dr> <ddr>` | `swporta_dr` / `ddr`. |
| `PWM <c0> … <c5>` | PWM duty. |
| `UART <text>` | UART0/1 line. |
| `FRAME <HEXBYTES>` | Mailbox TX. |
| `STOP <reason>` | Exit. |

Mailbox at `0x20000000`:

```text
magic u32     = 0x50485932 ("PHY2")
status u32    bit0 connected, bit1 notify
rx_seq, rx_len, rx[256]
tx_seq, tx_len, tx[256]
tick_ms u32
```
