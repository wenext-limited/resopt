//! Resource discovery and recoverable, pixel-verified optimization plans.
//!
//! The first backend recompresses static catalog PNGs without changing their
//! filenames, decoded samples, or non-IDAT chunks. Plans measure source bytes,
//! not compiled catalog or App Store download sizes.

#![doc = include_str!("../README.md")]

mod analysis;
mod catalog;
mod filesystem;
mod image_backend;
mod optimizer;
mod plan;
mod references;
mod report;
mod review;
mod server;
pub use server::serve;
mod resources;
mod scan_options;
pub use analysis::{
    AnalysisOptions, AnalysisReport, ImageCandidate, ResourceAnalysis, analyze,
    analyze_with_progress,
};
pub use image_backend::{ImageDifference, ImageInfo, image_backend_available};
pub use report::refresh_report;
pub use resources::{Resource, ResourceInventory, inventory, inventory_with_options};
pub use scan_options::ScanOptions;

pub use catalog::{Asset, Inventory, scan, scan_with_options};
pub use optimizer::Policy;
pub use plan::{ApplyReport, Candidate, Plan, apply, create_plan, read_plan, restore};
