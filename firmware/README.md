# Emulator regression firmware

Freestanding Cortex-M0 firmware for the AI-Thinker **PB-03F-Kit / PHY6252**. It does not depend on the vendor SDK.

```sh
make -C firmware clean all
```

Requires `arm-none-eabi-gcc`.

The build produces:

- `build/kit-demo.hex` - small interactive GPIO/ADC/UART/PWM/ATT demo;
- `build/rssi-rank.hex` - top-5 advertiser ranking on kit LEDs R/G/B/Y/W (P7/P11/P18/P0/P34), blink rate from RSSI;
- `build/capability-demo.hex` - strict emulator regression image.

`rssi-rank` consumes mailbox scan reports (`scan` / `gone` in the emulator, or neighbour chips under `phy6252 sim`). Colours stay with a device while it remains in the top 5; a drop-out or 4 s timeout frees that colour. Restore is P15. P14 is a header pin, not an LED.

`capability-demo` exercises OSAL memory and linked queues, SPI-NOR erase/program, persistent-flash-compatible access, AES, HCI/LL compatibility entrypoints, all modeled DMAC transfer directions used by the test, GPIO, ADC, PWM and UART.

Run it directly:

```sh
../target/debug/phy6252 --strict --once --max-insns 1000000 build/capability-demo.hex
```

Or keep NOR state across runs:

```sh
PHY6252_FLASH_STATE=/tmp/phy6252.flash \
  ../target/debug/phy6252 --strict --once --max-insns 1000000 build/capability-demo.hex
```

CI builds the freestanding images from source and treats the capability self-test plus the persistent restart as release-blocking smoke tests.

## Silicon BLE image

`make -C firmware/silicon` (or `make -C firmware silicon`) links against PHY62XX SDK 3.1.2 and writes `build/rssi-rank-ble.hex`. That image is for a real PB-03F-Kit: it advertises as `rssi-rank` and ranks over-the-air advertisers onto the five kit LEDs. It is not part of `make all` / CI.

```sh
make -C firmware/silicon PHY62XX_SDK=/path/to/PHY62XX_SDK_3.1.2
```
