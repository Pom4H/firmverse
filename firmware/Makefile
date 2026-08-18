CROSS   ?= arm-none-eabi-
override CC      := $(CROSS)gcc
override OBJCOPY := $(CROSS)objcopy
OUT     := build
KIT_ELF := $(OUT)/kit-demo.elf
KIT_HEX := $(OUT)/kit-demo.hex
RANK_ELF := $(OUT)/rssi-rank.elf
RANK_HEX := $(OUT)/rssi-rank.hex
CAP_ELF := $(OUT)/capability-demo.elf
CAP_HEX := $(OUT)/capability-demo.hex

CFLAGS  := -mcpu=cortex-m0 -mthumb -ffreestanding -fno-builtin -fno-tree-loop-distribute-patterns -nostdlib -Os -Wall -Wextra -Werror
LDFLAGS := -T linker.ld -nostdlib -Wl,--no-warn-rwx-segments

.PHONY: all clean silicon

all: $(KIT_HEX) $(RANK_HEX) $(CAP_HEX)

silicon:
	$(MAKE) -C silicon

$(OUT):
	mkdir -p $(OUT)

$(KIT_ELF): main.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -Wl,-Map=$(OUT)/kit-demo.map -o $@ main.c

$(RANK_ELF): rssi_rank.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -Wl,-Map=$(OUT)/rssi-rank.map -o $@ rssi_rank.c

$(CAP_ELF): capability.c controller_probe.c runtime.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -Wl,-Map=$(OUT)/capability-demo.map -o $@ capability.c controller_probe.c runtime.c

$(KIT_HEX): $(KIT_ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/kit-demo.bin

$(RANK_HEX): $(RANK_ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/rssi-rank.bin

$(CAP_HEX): $(CAP_ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/capability-demo.bin

clean:
	rm -rf $(OUT)
