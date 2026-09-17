//! Virtual memory management: page tables, MAIR configuration, identity mapping

pub mod identity;
pub mod mair;
pub mod pgtable;
pub mod pgtable_hwdef;

pub use identity::setup_identity_mapping;
pub use mair::setup_mair_ranges;
