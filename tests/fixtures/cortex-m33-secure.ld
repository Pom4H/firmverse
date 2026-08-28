ENTRY(reset)

MEMORY {
  FLASH (rx)  : ORIGIN = 0x10000000, LENGTH = 1M
  RAM   (rwx) : ORIGIN = 0x38000000, LENGTH = 256K
}

SECTIONS {
  .vector_table ORIGIN(FLASH) : {
    KEEP(*(.vector_table))
  } > FLASH

  .text : {
    *(.text*)
  } > FLASH

  .rodata : {
    *(.rodata*)
  } > FLASH

  .data : {
    *(.data*)
  } > RAM AT> FLASH

  .bss (NOLOAD) : {
    *(.bss*)
    *(COMMON)
  } > RAM
}
