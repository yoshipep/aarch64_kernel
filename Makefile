# Target architecture and toolchain
TARGET = aarch64-unknown-none
AS = aarch64-linux-gnu-as
CC = aarch64-linux-gnu-gcc-14
CFLAGS = -Wall -ggdb -ffreestanding -nostdlib -I./include
LD = rust-lld
QEMU = qemu-system-aarch64
VERSION := debug

# File paths
SRC_DIR = src
ASM_DIR = $(SRC_DIR)/asm
BOOT_ASM = $(ASM_DIR)/boot.s
KERNEL_RS = $(SRC_DIR)/lib.rs
LINKER_SCRIPT = linker.ld

# Output filenames
BOOT_OBJ = $(ASM_DIR)/boot.o
CRATE_NAME := $(shell cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
#KERNEL_OBJ = $(shell find `pwd`/target/$(TARGET)/$(VERSION) -name "*lib*.a")
KERNEL_OBJ = target/$(TARGET)/$(VERSION)/lib$(CRATE_NAME).a
KERNEL_ELF = kernel.elf

# QEMU options
QEMU_FLAGS = -machine virt -cpu cortex-a57 -nographic -kernel $(KERNEL_ELF)

# Build the kernel
all: $(KERNEL_ELF)

# Assemble the boot.s to boot.o
$(BOOT_OBJ): $(BOOT_ASM)
	$(AS) $< -o $@

# Compile the Rust kernel to an object file
$(KERNEL_OBJ): $(KERNEL_RS)
	cargo build --target $(TARGET)

# Link the kernel object and boot object into an ELF
$(KERNEL_ELF): $(BOOT_OBJ) $(KERNEL_OBJ) $(LINKER_SCRIPT)
	$(LD) -flavor gnu -o $(KERNEL_ELF) -T $(LINKER_SCRIPT) -o $@ $(BOOT_OBJ) $(KERNEL_OBJ)

# Run the kernel with QEMU
run: $(KERNEL_ELF)
	$(QEMU) $(QEMU_FLAGS)

# Clean up build artifacts
clean:
	cargo clean
	rm -rf target
	rm -f $(BOOT_OBJ) $(KERNEL_ELF)

.PHONY: all run clean
