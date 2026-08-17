CROSS   ?= arm-none-eabi-
override CC      := $(CROSS)gcc
override OBJCOPY := $(CROSS)objcopy
OUT     := build
ELF     := $(OUT)/kit-demo.elf
HEX     := $(OUT)/kit-demo.hex

CFLAGS  := -mcpu=cortex-m0 -mthumb -ffreestanding -nostdlib -Os -Wall -Wextra -Werror
LDFLAGS := -T linker.ld -nostdlib -Wl,-Map=$(OUT)/kit-demo.map -Wl,--no-warn-rwx-segments

.PHONY: all clean

all: $(HEX)

$(OUT):
	mkdir -p $(OUT)

$(ELF): main.c board.h linker.ld | $(OUT)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ main.c

$(HEX): $(ELF)
	$(OBJCOPY) -O ihex $< $@
	$(OBJCOPY) -O binary $< $(OUT)/kit-demo.bin

clean:
	rm -rf $(OUT)
