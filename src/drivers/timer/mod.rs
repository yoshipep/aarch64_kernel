//! Timer driver module

pub mod arch_timer;

// Re-export commonly used functions for convenience
pub use arch_timer::setup;
