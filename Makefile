#==============================================================================
# AArch64 Kernel Build System
#==============================================================================
# This Makefile builds:
#   1. Kernel (src/) - Rust + Assembly
#      - Uses kernel-specific assembly includes from src/include/asm/
#      - Assembly objects output to src/build/
#   2. Bootloader (bootloader/) - Optional submodule
#      - If present, built using bootloader/Makefile
#      - Completely independent build system
#   3. Device Tree Blob for QEMU virt machine
#
# Main targets:
#   make all        - Build kernel (+ bootloader if present) and DTB
#   make run        - Build and run (with bootloader if present, else kernel directly)
#   make run-kernel - Build and run kernel directly (bypass bootloader)
#   make clean      - Clean all build artifacts
#   make doc        - Generate documentation
#
# Note: The bootloader is optional. If the submodule is not initialized,
#       the kernel will build and run independently.
#==============================================================================

#==============================================================================
# TOOLCHAIN CONFIGURATION
#==============================================================================
TARGET = aarch64-unknown-none
CC = aarch64-linux-gnu-gcc-14
CFLAGS = -c -I$(INCLUDE_DIR) -x assembler-with-cpp
AS = aarch64-linux-gnu-as
ASFLAGS = -I$(INCLUDE_DIR)
CPP = aarch64-linux-gnu-cpp-14
CPPFLAGS = -I$(INCLUDE_DIR)
OBJCOPY = aarch64-linux-gnu-objcopy
QEMU = qemu-system-aarch64
VERSION := debug
LD = aarch64-linux-gnu-ld

#==============================================================================
# PATHS AND SOURCES
#==============================================================================
SRC_DIR := src
INCLUDE_DIR := include
BUILD_DIR := build
ASM_DIR := $(SRC_DIR)/asm

RUST_SRC := $(shell find $(SRC_DIR) -name '*.rs')
ASM_SRC_S := $(shell find $(SRC_DIR) -name '*.s')
ASM_SRC_S_CAP := $(shell find $(SRC_DIR) -name '*.S')

# Kernel output files (objects go into build directory)
ASM_OBJS := $(patsubst %,$(BUILD_DIR)/%.o,$(ASM_SRC_S)) \
            $(patsubst %,$(BUILD_DIR)/%.o,$(ASM_SRC_S_CAP))
CRATE_NAME := $(shell cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
RUST_OBJ := target/$(TARGET)/debug/lib$(CRATE_NAME).a
OBJS := $(ASM_OBJS) $(RUST_OBJ)
KERNEL_ELF = kernel.elf
LINKER_SCRIPT = linker.lds

#==============================================================================
# BOOTLOADER CONFIGURATION
#==============================================================================
BOOTLOADER_DIR = bootloader
BOOTLOADER_BIN = $(BOOTLOADER_DIR)/bootloader.bin

# Check if bootloader submodule exists
BOOTLOADER_EXISTS := $(shell test -d $(BOOTLOADER_DIR) && test -f $(BOOTLOADER_DIR)/Makefile && echo yes || echo no)

#==============================================================================
# COMMON
#==============================================================================
DOC_DIR := doc
DTB_FILE := virt.dtb
COMBINED_BLOB := combined.bin

#==============================================================================
# QEMU CONFIGURATION
#==============================================================================
ifeq ($(BOOTLOADER_EXISTS),yes)
	# Boot with bootloader if present
	QEMU_FLAGS = -machine virt,gic-version=3,virtualization=on -cpu cortex-a57 -serial stdio \
				-kernel $(COMBINED_BLOB) -dtb $(DTB_FILE) -m 1G
else
	# Boot kernel directly if no bootloader
	QEMU_FLAGS = -machine virt,gic-version=3,virtualization=on -cpu cortex-a57 -serial stdio \
				-kernel $(KERNEL_ELF) -dtb $(DTB_FILE) -m 1G
endif

#==============================================================================
# BUILD TARGETS
#==============================================================================

ifeq ($(BOOTLOADER_EXISTS),yes)
all: $(KERNEL_ELF) $(BOOTLOADER_BIN) $(COMBINED_BLOB) $(DTB_FILE)
	@echo "Build complete: kernel + bootloader"
else
all: $(KERNEL_ELF) $(DTB_FILE)
	@echo "Build complete: kernel only (no bootloader found)"
endif

$(BUILD_DIR)/%.s.o: $(SRC_DIR)/%.s
	@mkdir -p $(dir $@)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/%.S.o: $(SRC_DIR)/%.S
	@mkdir -p $(dir $@)
	$(AS) $(ASFLAGS) $< -o $@

$(RUST_OBJ): $(RUST_SRC)
	@echo "Building Rust kernel..."
	cargo build --target $(TARGET)

# Link the kernel
$(KERNEL_ELF): $(ASM_OBJS) $(RUST_OBJ) $(LINKER_SCRIPT).tmp
	@echo "Linking kernel: $@"
	$(LD) -T $(LINKER_SCRIPT).tmp -o $@ $(ASM_OBJS) $(RUST_OBJ)

#------------------------------------------------------------------------------
# BOOTLOADER BUILD RULES
#------------------------------------------------------------------------------

# Build bootloader using its own Makefile (if present)
ifeq ($(BOOTLOADER_EXISTS),yes)
$(BOOTLOADER_BIN):
	@echo "Building bootloader..."
	$(MAKE) -C $(BOOTLOADER_DIR)
endif

#------------------------------------------------------------------------------
# BLOB BUILD RULES (Bootloader + Kernel combined)
#------------------------------------------------------------------------------

ifeq ($(BOOTLOADER_EXISTS),yes)
# Create combined blob: bootloader.bin + kernel.elf (with alignment for safe struct access)
# Align to 4096 bytes (page size) - reasonable tradeoff between space and alignment
KERNEL_ALIGN := 4096
$(COMBINED_BLOB): $(BOOTLOADER_BIN) $(KERNEL_ELF)
	@echo "Creating combined blob: bootloader + kernel (aligned for struct safety)..."
	@echo "  Bootloader: $(BOOTLOADER_BIN) (loaded at 0x40080000 by QEMU)"
	@cp $(BOOTLOADER_BIN) $(COMBINED_BLOB)
	@truncate -s %$(KERNEL_ALIGN) $(COMBINED_BLOB)
	@KERNEL_OFFSET=$$(stat -c%s $(COMBINED_BLOB)); \
	 KERNEL_ADDR=$$(printf "0x%x" $$((0x40080000 + KERNEL_OFFSET))); \
	 echo "  Bootloader padded to: 0x$$(printf %x $$KERNEL_OFFSET) (aligned to $(KERNEL_ALIGN) bytes)"; \
	 echo "  Kernel ELF: $(KERNEL_ELF) at offset 0x$$(printf %x $$KERNEL_OFFSET) (runtime addr: $$KERNEL_ADDR)"
	@cat $(KERNEL_ELF) >> $(COMBINED_BLOB)
	@echo -n "  Combined blob size: "
	@ls -lh $(COMBINED_BLOB) | awk '{print $$5}'
	@echo "Blob created successfully!"

# Build blob (depends on bootloader and kernel)
blob: $(COMBINED_BLOB)
	@echo "Blob build complete"

# Run the combined blob
run-blob: $(COMBINED_BLOB) $(DTB_FILE)
	@echo "Running combined blob (bootloader will load kernel)..."
	$(QEMU) -machine virt,gic-version=3,virtualization=on -cpu cortex-a57 -serial stdio \
			-kernel $(COMBINED_BLOB) -dtb $(DTB_FILE) -m 1G
endif

#------------------------------------------------------------------------------
# COMMON BUILD RULES
#------------------------------------------------------------------------------
$(DTB_FILE):
	$(QEMU) -machine virt,gic-version=3,dumpdtb=$@ -cpu cortex-a57

# Run with bootloader
run: all
	$(QEMU) $(QEMU_FLAGS)

$(LINKER_SCRIPT).tmp: $(LINKER_SCRIPT) $(INCLUDE_DIR)
	$(CPP) $(CPPFLAGS) -P -C $< -o $@

$(BUILD_DIR)/%.s.o: %.s
	@mkdir -p $(dir $@)
	$(AS) $(ASFLAGS) $< -o $@

$(BUILD_DIR)/%.S.o: %.S
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $< -o $@

# Run kernel directly (for testing without bootloader)
run-kernel: $(KERNEL_ELF) $(DTB_FILE)
	$(QEMU) -machine virt,gic-version=3 -cpu cortex-a57 -serial stdio \
			-kernel $(KERNEL_ELF) -dtb $(DTB_FILE)

doc:
	cargo doc --target $(TARGET) --no-deps --target-dir $(DOC_DIR)

doc-open:
	cargo doc --target $(TARGET) --no-deps --target-dir $(DOC_DIR) --open

clean: clean-kernel clean-bootloader clean-common

clean-kernel:
	@echo "Cleaning kernel artifacts..."
	cargo clean
	rm -rf $(DOC_DIR)
	rm -rf $(BUILD_DIR)
	rm -f $(KERNEL_ELF)

clean-bootloader:
ifeq ($(BOOTLOADER_EXISTS),yes)
	@echo "Cleaning bootloader artifacts..."
	$(MAKE) -C $(BOOTLOADER_DIR) clean
else
	@echo "No bootloader to clean"
endif

clean-common:
	@echo "Cleaning common artifacts..."
	rm -f $(DTB_FILE)
	rm -f $(COMBINED_BLOB)
	rm -f $(LINKER_SCRIPT).tmp


.PHONY: all run run-kernel doc doc-open clean clean-kernel clean-bootloader clean-common
