# PHY6252 silicon image: real BLE observer + advertiser for rssi-rank.
# Requires PHY62XX SDK 3.1.2 (GCC libs). Override the path if needed:
#   make -C firmware/silicon PHY62XX_SDK=/path/to/PHY62XX_SDK_3.1.2

SHELL := /bin/bash
CROSS ?= arm-none-eabi-
CC := $(CROSS)gcc
OBJCOPY := $(CROSS)objcopy
SIZE := $(CROSS)size

PROJ_ROOT := $(abspath .)
FW := $(abspath ..)
SDK_CANDIDATES := \
	$(PHY62XX_SDK) \
	$(abspath $(FW)/../../Bolid/Firmware/sdk/PHY62XX_SDK_3.1.2) \
	$(abspath $(FW)/../Bolid/Firmware/sdk/PHY62XX_SDK_3.1.2)
ROOT := $(firstword $(foreach p,$(SDK_CANDIDATES),$(if $(wildcard $(p)/lib/libphy6222_rf.a),$(p),)))

ifeq ($(ROOT),)
$(error PHY62XX SDK not found. Set PHY62XX_SDK to PHY62XX_SDK_3.1.2)
endif

BUILD := $(PROJ_ROOT)/build
HEX := $(FW)/build/rssi-rank-ble.hex
ELF := $(BUILD)/rssi-rank-ble.elf
MAP := $(BUILD)/rssi-rank-ble.map
ROM_SYM := $(ROOT)/misc/bb_rom_sym_m0.txt
ROM_LD := $(BUILD)/rom_data.ld
ROM_ASM := $(BUILD)/rom_veneers.S
ROM_OBJ := $(BUILD)/rom_veneers.o
FLASH_LD := $(PROJ_ROOT)/rssi-rank.ld
OBJDUMP := $(CROSS)objdump
SBP_SRC := $(ROOT)/example/ble_peripheral/simpleBlePeripheral

VPATH = $(PROJ_ROOT)
VPATH += $(PROJ_ROOT)/source
VPATH += $(SBP_SRC)
VPATH += $(SBP_SRC)/source

include $(ROOT)/components/gcc/components.cflags
include $(ROOT)/components/gcc/components.mk

CSRCS := $(filter-out ota_app_service.c devinfoservice.c pwm.c led_light.c phy6222_cstart.c,$(CSRCS))

vpath %.c $(VPATH)
vpath %.s $(ROOT)/components/gcc/CMSIS/device/phyplus

DEF := $(filter-out -DHOST_CONFIG=4 -DMI_MESH_ENABLED=1,$(DEF))
DEF += -DHOST_CONFIG=0x0C
DEF += -DMAX_NUM_LL_CONN=1
DEF += -DGATT_MAX_NUM_CONN=MAX_NUM_LL_CONN+1

INC += -I$(PROJ_ROOT)/source
INC += -I$(PROJ_ROOT)/include

SRC_RAW = main.c
SRC_RAW += OSAL_SimpleBLEPeripheral.c
SRC_RAW += SimpleBLEPeripheral_Main.c
SRC_RAW += simpleBLEPeripheral.c
SRC_RAW += cstart_flash.c
SRC_RAW += wrap_main.c
SRC_RAW += libc_stub.c
SRC_RAW += $(CSRCS)

STARTUP_RAW = phy6222_start.s

override CFLAGS := -Os -mcpu=cortex-m0 -mthumb -ffreestanding -fno-builtin \
	-ffunction-sections -fdata-sections -Wall -nostartfiles -nostdlib \
	-Wno-implicit-function-declaration

LIBS := $(filter-out -lc -lm,$(LIBS))
LIBS += -lgcc

LFLAGS += -Wl,--gc-sections
LFLAGS += -Wl,--wrap=main
LFLAGS += -Wl,--wrap=ll_patch_slave
LFLAGS += -Wl,--wrap=ll_patch_advscan
LFLAGS += -Wl,--wrap=dbg_printf_init
LFLAGS += -Wl,--wrap=dbg_printf
LFLAGS += -Wl,--wrap=watchdog_config
LFLAGS += -Wl,-Map=$(MAP)
LFLAGS += -Wl,--no-warn-mismatch,--allow-multiple-definition
LFLAGS += $(LIBS_PATH)
LFLAGS += $(LIBS)

OBJ := $(addprefix $(BUILD)/,$(notdir $(SRC_RAW:%.c=%.o)))
STARTUP_OBJ := $(BUILD)/$(notdir $(STARTUP_RAW:%.s=%.o))
BOOT_OBJ := $(BUILD)/boot.o

.PHONY: all clean

all: $(HEX)

$(BUILD):
	mkdir -p $(BUILD)
	mkdir -p $(FW)/build

$(ROM_LD) $(ROM_ASM): $(ROM_SYM) gen_rom_sym.py | $(BUILD)
	python3 gen_rom_sym.py $(ROM_SYM) $(ROM_ASM) $(ROM_LD)

$(ROM_OBJ): $(ROM_ASM) | $(BUILD)
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD)/simpleBLEPeripheral.o: simpleBLEPeripheral.c | $(BUILD)
	$(CC) -c $(CFLAGS) $(DEF) $(INC) $< -o $@

$(BUILD)/main.o: source/main.c | $(BUILD)
	$(CC) -c $(CFLAGS) -w $(DEF) $(INC) $< -o $@

$(BUILD)/%.o: %.c | $(BUILD)
	$(CC) -c $(CFLAGS) -w $(DEF) $(INC) $< -o $@

$(BUILD)/%.o: %.s | $(BUILD)
	$(CC) -c $(CFLAGS) $< -o $@

$(BOOT_OBJ): boot.S | $(BUILD)
	$(CC) -c $(CFLAGS) $< -o $@

$(ELF): $(OBJ) $(STARTUP_OBJ) $(BOOT_OBJ) $(ROM_OBJ) $(ROM_LD) | $(BUILD)
	$(CC) -o $@ $(CFLAGS) $(OBJ) $(STARTUP_OBJ) $(BOOT_OBJ) $(ROM_OBJ) $(LFLAGS) -T$(FLASH_LD) -T$(ROM_LD)
	$(SIZE) $@

$(HEX): $(ELF)
	$(OBJCOPY) -O ihex $< $(BUILD)/flash.hex
	vma=$$($(OBJDUMP) -h $< | awk '/\.data /{print $$4}'); \
	$(OBJCOPY) -j .data --change-section-lma .data=0x$$vma -O ihex $< $(BUILD)/data-sram.hex
	$(OBJCOPY) -j .jump_table --change-section-lma .jump_table=0x1fff0000 -O ihex $< $(BUILD)/jt.hex
	$(OBJCOPY) -j .global_config --change-section-lma .global_config=0x1fff0400 -O ihex $< $(BUILD)/gc.hex
	python3 merge_hex.py $(BUILD)/flash.hex $(BUILD)/jt.hex $(BUILD)/gc.hex $(BUILD)/data-sram.hex $@ 0x1fff1838
	$(OBJCOPY) -O binary $< $(FW)/build/rssi-rank-ble.bin
	@echo "silicon image $@"

clean:
	rm -rf $(BUILD)
	rm -f $(HEX) $(FW)/build/rssi-rank-ble.bin
