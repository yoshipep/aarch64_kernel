# Target architecture and toolchain
TARGET = aarch64-unknown-none
AS = aarch64-linux-gnu-as
ASFLAGS = -I./include/asm
CC = aarch64-linux-gnu-gcc-14
CFLAGS = -Wall -ggdb -ffreestanding -nostdlib -I./include
LD = rust-lld
QEMU = qemu-system-aarch64
VERSION := debug

# File paths
SRC_DIR = src
ASM_DIR = $(SRC_DIR)/asm
RUST_SRC := $(shell find $(SRC_DIR) -name '*.rs')
ASM_SRC := $(shell find $(SRC_DIR) -name '*.s')
LINKER_SCRIPT = linker.ld

# Output filenames
ASM_OBJS := $(ASM_SRC:.s=.o)
CRATE_NAME := $(shell cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
KERNEL_OBJ = target/$(TARGET)/$(VERSION)/lib$(CRATE_NAME).a
KERNEL_ELF = kernel.elf

# QEMU options
QEMU_FLAGS = -machine virt,gic-version=3 -cpu cortex-a57 -nographic -kernel $(KERNEL_ELF)

# Build the kernel
all: $(KERNEL_ELF)

# Assemble the boot.s to boot.o
$(ASM_DIR)/%.o: $(ASM_DIR)/%.s
	$(AS) $(ASFLAGS) $< -o $@

# Compile the Rust kernel to an object file
$(KERNEL_OBJ): $(RUST_SRC)
	cargo build --target $(TARGET)

# Link the kernel object and boot object into an ELF
$(KERNEL_ELF): $(ASM_OBJS) $(KERNEL_OBJ) $(LINKER_SCRIPT)
	$(LD) -flavor gnu -T $(LINKER_SCRIPT) -o $@ $(ASM_OBJS) $(KERNEL_OBJ)

# Run the kernel with QEMU
run: $(KERNEL_ELF)
	$(QEMU) $(QEMU_FLAGS)

# Clean up build artifacts
clean:
	cargo clean
	rm -rf target
	rm -f $(ASM_OBJS) $(KERNEL_ELF)

.PHONY: all run clean
