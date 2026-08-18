CROSS   ?= arm-none-eabi-
override CC      := $(CROSS)gcc
override OBJCOPY := $(CROSS)objcopy
OUT     := build
KIT_ELF := $(OUT)/kit-demo.elf
KIT_HEX := $(OUT)/kit-demo.hex
CAP_ELF := $(OUT)/capability-demo.elf
CAP_HEX := $(OUT)/capability-demo.hex

CFLAGS  := -mcpu=cortex-m0 -mthumb -ffreestanding -nostdlib -Os -Wall -Wextra -Werror
LDFLAGS := -T linker.ld -nostdlib -Wl,--no-warn-rwx-segments

.PHONY: all clean

all: $(KIT_HEX) $(CAP_HEX)

$(OUT):
	mkdir -p $(OUT)

$(KIT_ELF): main.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -Wl,-Map=$(OUT)/kit-demo.map -o $@ main.c

$(CAP_ELF): capability.c controller_probe.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -Wl,-Map=$(OUT)/capability-demo.map -o $@ capability.c controller_probe.c

$(KIT_HEX): $(KIT_ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/kit-demo.bin

$(CAP_HEX): $(CAP_ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/capability-demo.bin

clean:
	rm -rf $(OUT)
