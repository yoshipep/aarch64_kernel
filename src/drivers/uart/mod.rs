//! PL011 UART driver module

pub mod pl011;

// Re-export commonly used functions for convenience
pub use pl011::{getchar, putchar};
