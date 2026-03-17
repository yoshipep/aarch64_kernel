# aarch64_kernel

Minimal AArch64 bare-metal kernel written in Rust, for learning purposes.

This project is part of a blog series I’m writing at [jcomes.org](https://jcomes.org/), where I document the process of writing a simple OS for the AArch64 architecture — using Rust (a language I’m learning along the way). The goal is to go from boot to a minimal working kernel, exploring memory management, exception handling, and more.

This is not intended to be a complete OS — it’s a learning project, built from scratch.

---

## 🧱 Features (so far)

- Runs on `qemu-system-aarch64` using the `virt` machine (Cortex-A57, GICv3)
- Freestanding Rust code (no `std`, no runtime)
- Custom linker script and boot assembly
- Boots from the [bootloader](https://github.com/yoshipep/aarch64_bootloader)
- **Device Tree Blob (DTB) parsing** — discovers hardware at boot by walking the flattened device tree. Devices register a `compatible` string and a setup function in a static match table, similar to Linux's `platform_driver` model
- **GICv3 interrupt controller** — full driver for the Distributor (SPIs) and Redistributor (PPIs/SGIs), with support for priority, group, trigger mode (level/edge), and affinity routing
- **PL011 UART driver** — polling TX, interrupt-driven RX with an IRQ-safe circular buffer. Base address and clock frequency discovered from the DTB. Includes an early console fallback (hardcoded base address) so `print!` works before DTB-based driver initialization
- **ARM Generic Timer** — non-secure physical timer (EL1) with millisecond-granularity arming. Interrupt configured as a PPI through the GIC redistributor
- **Exception handling** — full vector table with handlers for synchronous exceptions (SVC), IRQs, FIQs, and SErrors. Unimplemented exception classes are decoded and reported
- **IRQ-safe mutex** — spinlock that masks interrupts while held, preventing deadlocks between main code and interrupt handlers
- **Platform features** — compile-time platform selection via Cargo features (`qemu-virt` default). Platform-specific constants (e.g., early console address) are gated behind feature flags, preparing for future hardware targets like Raspberry Pi
- **Identity mapping and MMU** — sets up MAIR_EL1 (device nGnRnE, normal write-back, normal non-cacheable), builds a two-level page table (L0 table → L1 1 GiB block descriptors) for identity mapping, configures TCR_EL1 (48-bit VA, 40-bit PA, 4K granule, inner-shareable cacheable), and enables the MMU via SCTLR_EL1

---

## 🔧 Requirements

You'll need:

- A Linux environment (Ubuntu 22.04+ recommended)
- Rust (via [rustup.rs](https://rustup.rs))
- QEMU (`qemu-system-aarch64`)
- AArch64 cross-compilation tools (`gcc-aarch64-linux-gnu` or `binutils` + `gcc` built manually)

Install required tools via apt:

```bash
sudo apt update
sudo apt install qemu-system-aarch64 gcc-14-aarch64-linux-gnu binutils-aarch64-linux-gnu
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-unknown-none
```

## 🚀 Build & Run

To compile and run:

```bash
make run
```
