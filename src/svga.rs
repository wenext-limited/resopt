//! SVGA animation optimization (placeholder until the verified path lands).
use crate::{Resource, ResourceAnalysis, analyze_image::Context};

pub(crate) fn analyze(
    _context: &Context<'_>,
    resource: &Resource,
    _index: usize,
    _original: &[u8],
    digest: &str,
) -> ResourceAnalysis {
    let mut row = ResourceAnalysis::new(resource, "unsupported");
    row.sha256 = Some(digest.to_string());
    row
}
