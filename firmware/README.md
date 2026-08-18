# Emulator regression firmware

Freestanding Cortex-M0 firmware for the AI-Thinker **PB-03F-Kit / PHY6252**. It does not depend on the vendor SDK.

```sh
make -C firmware clean all
```

Requires `arm-none-eabi-gcc`.

The build produces:

- `build/kit-demo.hex` - small interactive GPIO/ADC/UART/PWM/ATT demo;
- `build/capability-demo.hex` - strict emulator regression image.

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

CI builds both images from source and treats the capability self-test plus the persistent restart as release-blocking smoke tests.
