//! Resource discovery and recoverable, pixel-verified optimization plans.
//!
//! The first backend recompresses static catalog PNGs without changing their
//! filenames, decoded samples, or non-IDAT chunks. Plans measure source bytes,
//! not compiled catalog or App Store download sizes.

#![cfg_attr(feature = "native", doc = include_str!("../README.md"))]

#[cfg(feature = "native")]
mod aapt;
#[cfg(feature = "native")]
mod analysis;
#[cfg(feature = "native")]
mod archive;
#[cfg(feature = "native")]
pub use archive::{ArchiveEntry, ArchiveInfo};
#[cfg(feature = "native")]
mod localization;
#[cfg(feature = "native")]
mod localization_checks;
#[cfg(feature = "native")]
pub use localization::{LanguageCoverage, LocalizationFile, LocalizationInfo, LocalizationIssue};
#[cfg(feature = "native")]
mod media;
#[cfg(feature = "native")]
mod pag;
#[cfg(feature = "native")]
mod vap;
#[cfg(feature = "native")]
pub use media::MediaInfo;
#[cfg(feature = "native")]
pub use pag::PagInfo;
#[cfg(feature = "native")]
pub use vap::VapInfo;
#[cfg(feature = "native")]
mod package_diff;
#[cfg(feature = "native")]
pub use package_diff::{EntryChange, PackageDiff, package_diff};
#[cfg(feature = "native")]
mod analyze_image;
#[cfg(feature = "native")]
pub mod android;
#[cfg(feature = "native")]
mod android_project;
#[cfg(feature = "native")]
mod android_refs;
#[cfg(feature = "native")]
mod batch;
#[cfg(feature = "native")]
pub use batch::{
    BatchItem, BatchOutcome, BatchPlan, BatchPolicy, BatchStatus, apply_report, plan_report,
    restore_report,
};
#[cfg(feature = "native")]
mod cache;
#[cfg(feature = "native")]
pub use cache::default_directory as cache_directory;
#[cfg(feature = "native")]
mod capabilities;
#[cfg(feature = "native")]
mod catalog;
#[cfg(feature = "native")]
mod similarity;
#[cfg(feature = "native")]
pub use similarity::{Fingerprint, SimilarComparison, SimilarGroup};
#[cfg(feature = "native")]
mod svga;
#[cfg(feature = "native")]
mod svga_render;
#[cfg(feature = "native")]
mod timings;
#[cfg(feature = "native")]
mod tools;
#[cfg(feature = "native")]
pub use android_project::MinSdk;
#[cfg(feature = "native")]
pub use capabilities::{Capabilities, Capability, capabilities};
#[cfg(feature = "native")]
pub use tools::Tool;
#[cfg(feature = "native")]
mod filesystem;
mod image_backend;
mod optimizer;
#[cfg(feature = "native")]
mod plan;
mod png_pixels;
#[cfg(feature = "native")]
mod png_quantize;
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
    AnalysisControl, AnalysisOptions, AnalysisReport, AnimationInfo, ImageCandidate, Performance,
    ResourceAnalysis, analyze, analyze_with_progress,
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

#[cfg(feature = "native")]
mod web;
#[cfg(feature = "native")]
pub use web::{WebOptions, web};

#[cfg(feature = "native")]
mod webp_backend;
