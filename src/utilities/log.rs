//! Kernel log helpers

/// Prints a message prefixed with its subsystem, dmesg-style: `pr_info!("mm", "x = {}", 1)` prints `mm: x = 1`.
///
/// The prefix is spliced into the format string with `concat!`, so it costs nothing at runtime. Takes the same
/// arguments as `println!`, preceded by the subsystem name, which must be a string literal.
#[macro_export]
macro_rules! pr_info {
    ($sub:literal, $fmt:literal) => {
        $crate::println!(concat!($sub, ": ", $fmt))
    };
    ($sub:literal, $fmt:literal, $($arg:tt)*) => {
        $crate::println!(concat!($sub, ": ", $fmt), $($arg)*)
    };
}
