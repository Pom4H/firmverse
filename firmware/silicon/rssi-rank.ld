MEMORY
{
  jumptbl (rwx) : ORIGIN = 0x1fff0000, LENGTH = 1K
  gcfgtbl  (rwx) : ORIGIN = 0x1fff0400, LENGTH = 1K
  flash (rx) : ORIGIN = 0x11020000, LENGTH = 180K
  sram (rwx) : ORIGIN = 0x1fff1838, LENGTH = 0xE7C8
}

OUTPUT_ARCH(arm)
EXTERN(_vectors)
EXTERN(Reset_Boot)
ENTRY(Reset_Boot)

SECTIONS
{
    .jump_table ORIGIN(jumptbl) :
    {
        KEEP(*(jump_table_mem_area))
    } > jumptbl

    .global_config ORIGIN(gcfgtbl) :
    {
        KEEP(*(global_config_area))
    } > gcfgtbl

    .textentry : {
        _stext = ABSOLUTE(.);
        *(*.isr_vector)
        *phy6222_start.o(.text)
        *cstart_flash.o(.text .text.*)
    } > flash

    .init_section : {
        _sinit = ABSOLUTE(.);
        *(.init_array .init_array.*)
        _einit = ABSOLUTE(.);
    } > flash

    .ARM.extab : {
        *(.ARM.extab*)
    } > flash

    __exidx_start = ABSOLUTE(.);
    .ARM.exidx : {
        *(.ARM.exidx*)
    } > flash
    __exidx_end = ABSOLUTE(.);

    ._sjtblsstore : {
       _sjtblss = ABSOLUTE(.);
    } > flash

   ._eronlystore : {
       _eronly = ABSOLUTE(.);
   } > flash

    .data : {
        _sdata = ABSOLUTE(.);
        KEEP(*(.boot_vectors))
        KEEP(*(.boot_text))
        _stextram = ABSOLUTE(.);
		*.o(_section_standby_code_)
		*.o(_section_sram_code_)
		*flash.o(.text .text.*)
		*patch.o(.text .text.*)
		*patch.o(_func_xip_code_.ll_patch_master)
		*patch.o(_func_xip_code_.ll_patch_slave)
		*patch.o(_func_xip_code_.ll_patch_advscan)
		*clock.o(.text.hal_xtal16m_cap_Set)
		*rf_phy_driver.o(.text .text.*)
		*(.text)
		*libphy6222_host.a:l2cap_util.o(.text.L2CAP_Fragment_SendDataPkt)
		*libphy6222_host.a:l2cap_util.o(.text.l2capSegmentBuffToLinkLayer)
		*libphy6222_host.a:l2cap_util.o(.text.l2capPocessFragmentTxData)
        _etextram = ABSOLUTE(.);
        *(.data .data.*)
        *(.gnu.linkonce.d.*)
        CONSTRUCTORS
        . = ALIGN(4);
        _edata = ABSOLUTE(.);
    } > sram AT > flash

    .bss : {
        _sbss = ABSOLUTE(.);
        *(.bss .bss.*)
        *(.gnu.linkonce.b.*)
        *(COMMON)
        . = ALIGN(4);
        __heap_start__ = .;
        end = __heap_start__;
        _end = end;
        __end = end;
        _ebss = ABSOLUTE(.);
    } > sram

    .int_stack : {
        . = ALIGN(4);
        *(.int_stack)
        . = ALIGN(4);
        _stack_top = ABSOLUTE(.);
    } > sram

   .common_text : {
       *(.text .text.*)
       *(.rodata .rodata.*)
       *(.fixup)
        *(.gnu.warning)
        *(.gnu.linkonce.t.*)
        *(.glue_7)
        *(.glue_7t)
        *(.got)
        *(.gcc_except_table)
        *(.gnu.linkonce.r.*)
        _etext = ABSOLUTE(.);
   } > flash

    .stab 0 : { *(.stab) }
    .stabstr 0 : { *(.stabstr) }
    .comment 0 : { *(.comment) }
    .debug_abbrev 0 : { *(.debug_abbrev) }
    .debug_info 0 : { *(.debug_info) }
    .debug_line 0 : { *(.debug_line) }
    .debug_pubnames 0 : { *(.debug_pubnames) }
    .debug_aranges 0 : { *(.debug_aranges) }
}
