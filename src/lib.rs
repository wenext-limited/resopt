//! Resource discovery and recoverable, pixel-verified optimization plans.
//!
//! The first backend recompresses static catalog PNGs without changing their
//! filenames, decoded samples, or non-IDAT chunks. Plans measure source bytes,
//! not compiled catalog or App Store download sizes.

#![cfg_attr(feature = "native", doc = include_str!("../README.md"))]

#[cfg(feature = "native")]
mod analysis;
#[cfg(feature = "native")]
mod catalog;
#[cfg(feature = "native")]
mod filesystem;
mod image_backend;
mod optimizer;
#[cfg(feature = "native")]
mod plan;
mod png_pixels;
mod quality;
#[cfg(feature = "native")]
mod references;
#[cfg(feature = "native")]
mod report;
#[cfg(feature = "native")]
mod review;
#[cfg(feature = "native")]
mod server;
#[cfg(feature = "native")]
pub use server::serve;
#[cfg(feature = "native")]
mod resources;
#[cfg(feature = "native")]
mod scan_options;
#[cfg(feature = "native")]
pub use analysis::{
    AnalysisOptions, AnalysisReport, ImageCandidate, ResourceAnalysis, analyze,
    analyze_with_progress,
};
pub use image_backend::{
    DEFAULT_MAX_PIXELS, ImageDifference, ImageInfo, MAX_PIXELS_LIMIT, image_backend_available,
};
#[cfg(feature = "native")]
pub use report::refresh_report;
#[cfg(feature = "native")]
pub use resources::{Resource, ResourceInventory, inventory, inventory_with_options};
#[cfg(feature = "native")]
pub use scan_options::ScanOptions;

#[cfg(feature = "native")]
pub use catalog::{Asset, Inventory, scan, scan_with_options};
pub use optimizer::Policy;
#[cfg(feature = "native")]
pub use plan::{ApplyReport, Candidate, Plan, apply, create_plan, read_plan, restore};

/// Platform-independent, in-memory PNG optimization and image scoring.
pub mod portable;
