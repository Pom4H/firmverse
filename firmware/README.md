# PB-03F-Kit demo image

Bare-metal Cortex-M0 firmware for the AI-Thinker **PB-03F-Kit** (PHY6252). No vendor SDK.

```sh
make -C firmware
```

Needs `arm-none-eabi-gcc`. A prebuilt `firmware/kit-demo.hex` is in the tree for `cargo run -- --live`.

The image lives in SRAM at `0x1FFF0000` and walks every kit header the emulator models: RGB + P0, P14/P16/P17/P31–P33, P34 input, ADC pads P20/P15/P24/P23, UART0/1, PWM, timer, WDT, I2C0, SPI0, PCR/AON, and BLE ATT through the mailbox at `0x20000000` (laptop radio via `scripts/air.sh`).
