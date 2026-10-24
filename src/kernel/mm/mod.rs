//! Virtual memory management: page tables, the mapping API, MAIR configuration, identity mapping

pub mod frame_alloc;
pub mod identity;
pub mod mair;
pub mod map;
pub mod pgtable;
pub mod pgtable_hwdef;

pub use identity::{check_kimage_mapping, setup_identity_mapping};
pub use mair::setup_mair_ranges;
