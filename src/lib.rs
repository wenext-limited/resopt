//! Resource discovery and recoverable, pixel-verified optimization plans.
//!
//! The first backend recompresses static catalog PNGs without changing their
//! filenames, decoded samples, or non-IDAT chunks. Plans measure source bytes,
//! not compiled catalog or App Store download sizes.

#![doc = include_str!("../README.md")]

mod catalog;
mod filesystem;
mod optimizer;
mod plan;

pub use catalog::{Asset, Inventory, scan};
pub use optimizer::Policy;
pub use plan::{ApplyReport, Candidate, Plan, apply, create_plan, read_plan, restore};
