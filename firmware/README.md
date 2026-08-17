# PB-03F-Kit demo image

Bare-metal Cortex-M0 firmware for the AI-Thinker **PB-03F-Kit** (PHY6252). No vendor SDK.

```sh
make -C firmware
```

Needs `arm-none-eabi-gcc`. A prebuilt `firmware/kit-demo.hex` is in the tree for `cargo run -- --live`.

The image lives in SRAM at `0x1FFF0000`. Drive it with `phy6252`: GPIO, ADC, UART, PWM, and ATT through the mailbox at `0x20000000`.
