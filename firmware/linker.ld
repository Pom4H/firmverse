MEMORY
{
    SRAM (rwx) : ORIGIN = 0x1FFF0000, LENGTH = 64K
}

ENTRY(Reset_Handler)

SECTIONS
{
    .vectors : {
        KEEP(*(.vectors))
    } > SRAM

    .text : {
        *(.text*)
        *(.rodata*)
    } > SRAM

    .bss (NOLOAD) : {
        _sbss = .;
        *(.bss*)
        *(COMMON)
        _ebss = .;
    } > SRAM

    .stack (NOLOAD) : {
        . = ALIGN(8);
        . += 4096;
        _estack = .;
    } > SRAM
}
