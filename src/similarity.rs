//! Duplicate and near-duplicate image detection.
//!
//! Each decoded image gets a scale-invariant fingerprint: 16×16 area-averaged
//! grids of its luminance (composited over mid-gray) and of its alpha channel,
//! plus its mean color. Area averaging makes the grids nearly independent of
//! pixel dimensions, so images whose grids agree are grouped even when their
//! sizes differ. (A DCT bit hash was tried first; hard-edged alpha masks made
//! it unstable across scales.) Intended variants of one
//! asset (`@2x`/`@3x`, renditions of one image set, Android density or locale
//! folders of one resource name) are never reported against each other.
//!
//! Groups are findings for a person to act on; nothing is merged automatically,
//! because removing a file means changing the code that names it.
use crate::{ResourceAnalysis, image_backend::Decoded};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const GRID: usize = 16;
/// Largest mean absolute grid difference (0–1) still called the same picture.
const MAX_LUMA_DISTANCE: f32 = 0.022;
const MAX_ALPHA_DISTANCE: f32 = 0.03;
/// No single luminance or alpha grid cell may differ by more than this.
const MAX_CELL_DISTANCE: f32 = 0.16;
/// Largest per-channel mean color difference on a 0–255 scale.
const MAX_COLOR_DISTANCE: i32 = 14;
const MAX_ASPECT_DIFFERENCE: f64 = 0.03;
/// Below this luminance variance an image is too flat to hash reliably.
const MIN_VARIANCE: f32 = 0.0004;
/// Distance at which differently sized images count as one picture resized.
const RESIZED_DISTANCE: f32 = 0.012;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    /// Hex-encoded 16×16 luminance grid, one byte per cell.
    pub luma: String,
    /// Hex-encoded 16×16 alpha grid; empty for opaque images.
    pub alpha: String,
    pub mean_rgb: [u8; 3],
    /// False for nearly uniform images, which only match exact duplicates.
    pub detailed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarGroup {
    /// `identical` (same bytes), `resized` (similar fingerprint, different
    /// dimensions) or `similar` (near-duplicate).
    pub kind: String,
    /// Report indexes, largest file first.
    pub members: Vec<usize>,
    /// Comparisons to members[0], in member order (including the reference).
    /// Empty in reports generated before similarity scores were introduced.
    #[serde(default)]
    pub comparisons: Vec<SimilarComparison>,
    /// Bytes beyond the largest member; not guaranteed removable savings.
    pub redundant_bytes: u64,
}

/// A heuristic fingerprint comparison, not replacement safety or SSIMULACRA2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarComparison {
    /// 0–100; only matching file hashes receive 100. Other matches cap at 99.9.
    pub score: f32,
    pub identical: bool,
    /// Mean absolute luminance / alpha and largest mean RGB channel difference.
    /// All differences are normalized to 0–1.
    pub brightness_difference: f32,
    pub opacity_difference: f32,
    pub color_difference: f32,
    /// Largest luminance or alpha cell difference; reveals localized changes.
    pub max_cell_difference: f32,
}

/// Area-average one channel expression onto a GRID×GRID grid.
fn downscale(image: &Decoded, sample: impl Fn(&[f32]) -> f32) -> Vec<f32> {
    let (width, height) = (image.info.width, image.info.height);
    let mut sums = vec![0.0_f32; GRID * GRID];
    let mut counts = vec![0_u32; GRID * GRID];
    for y in 0..height {
        let gy = y * GRID / height;
        let row = &image.pixels[y * width * 4..(y + 1) * width * 4];
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            let cell = gy * GRID + x * GRID / width;
            sums[cell] += sample(pixel);
            counts[cell] += 1;
        }
    }
    // Images smaller than the grid leave empty cells; reuse the nearest source.
    (0..GRID * GRID)
        .map(|cell| {
            if counts[cell] > 0 {
                return sums[cell] / counts[cell] as f32;
            }
            let (gx, gy) = (cell % GRID, cell / GRID);
            let pixel = ((gy * height / GRID) * width + gx * width / GRID) * 4;
            sample(&image.pixels[pixel..pixel + 4])
        })
        .collect()
}

fn encode(grid: &[f32]) -> String {
    grid.iter()
        .map(|v| format!("{:02x}", (v.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect()
}

fn decode(hex: &str) -> Option<Vec<f32>> {
    if hex.is_empty() {
        return Some(vec![1.0; GRID * GRID]);
    }
    if hex.len() != GRID * GRID * 2 || !hex.is_ascii() {
        return None;
    }
    (0..GRID * GRID)
        .map(|cell| {
            u8::from_str_radix(&hex[cell * 2..cell * 2 + 2], 16)
                .ok()
                .map(|v| f32::from(v) / 255.0)
        })
        .collect()
}

/// Mean and largest absolute difference between two grids.
fn distance(a: &[f32], b: &[f32]) -> (f32, f32) {
    let (mut total, mut worst) = (0.0_f32, 0.0_f32);
    for (x, y) in a.iter().zip(b) {
        let delta = (x - y).abs();
        total += delta;
        worst = worst.max(delta);
    }
    (total / a.len() as f32, worst)
}

pub(crate) fn fingerprint(image: &Decoded) -> Option<Fingerprint> {
    if image.info.width == 0 || image.info.height == 0 || image.pixels.is_empty() {
        return None;
    }
    // Pixels are premultiplied, so compositing over gray is `c + (1 - a) * 0.5`.
    let luma = downscale(image, |p| {
        0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2] + (1.0 - p[3]) * 0.5
    });
    let alpha = downscale(image, |p| p[3]);
    let mean = luma.iter().sum::<f32>() / luma.len() as f32;
    let variance = luma.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / luma.len() as f32;
    let opaque = alpha.iter().all(|a| *a >= 0.999);
    let mut color = [0.0_f64; 3];
    let mut coverage = 0.0_f64;
    for pixel in image.pixels.as_chunks::<4>().0 {
        for (total, value) in color.iter_mut().zip(pixel) {
            *total += f64::from(*value);
        }
        coverage += f64::from(pixel[3]);
    }
    let mean_rgb = color.map(|total| {
        if coverage > 0.0 {
            (total / coverage * 255.0).round().clamp(0.0, 255.0) as u8
        } else {
            0
        }
    });
    Some(Fingerprint {
        luma: encode(&luma),
        alpha: if opaque {
            String::new()
        } else {
            encode(&alpha)
        },
        mean_rgb,
        detailed: variance >= MIN_VARIANCE,
    })
}

/// Identity of the asset a file is a variant of. Files sharing it are intended
/// to look alike and are never reported against each other.
fn variant_key(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new(""));
    if parent
        .extension()
        .is_some_and(|e| e == "imageset" || e == "appiconset")
    {
        return parent.to_path_buf();
    }
    let stem = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = stem.split('.').next().unwrap_or_default();
    let stem = stem
        .strip_suffix("@2x")
        .or_else(|| stem.strip_suffix("@3x"))
        .unwrap_or(stem)
        .to_ascii_lowercase();
    match crate::android::classify(path) {
        // One resource name across density, locale and RTL folders of a module.
        Some(android) if android.area == "res" => android
            .module
            .join(android.source_set)
            .join(android.res_type.unwrap_or_default())
            .join(stem),
        _ => parent.join(stem),
    }
}

struct Entry {
    index: usize,
    bytes: u64,
    sha256: Option<String>,
    print: Fingerprint,
    luma: Vec<f32>,
    alpha: Vec<f32>,
    aspect: f64,
    dimensions: (usize, usize),
    frames: usize,
    variant: PathBuf,
}

fn compare(a: &Entry, b: &Entry) -> SimilarComparison {
    let identical = a.sha256.is_some() && a.sha256 == b.sha256;
    let (brightness_difference, luma_peak) = distance(&a.luma, &b.luma);
    let (opacity_difference, alpha_peak) = distance(&a.alpha, &b.alpha);
    let color_difference = a
        .print
        .mean_rgb
        .iter()
        .zip(b.print.mean_rgb)
        .map(|(x, y)| f32::from(x.abs_diff(y)) / 255.0)
        .fold(0.0_f32, f32::max);
    // Use the worst average channel, so opacity or tint differences cannot be
    // diluted by the other channels. Peak differences remain separately visible.
    let difference = brightness_difference
        .max(opacity_difference)
        .max(color_difference);
    SimilarComparison {
        score: if identical {
            100.0
        } else {
            (100.0 * (1.0 - difference)).clamp(0.0, 99.9)
        },
        identical,
        brightness_difference,
        opacity_difference,
        color_difference,
        max_cell_difference: luma_peak.max(alpha_peak),
    }
}

fn alike(a: &Entry, b: &Entry) -> bool {
    if a.variant == b.variant {
        return false;
    }
    if a.sha256.is_some() && a.sha256 == b.sha256 {
        return true;
    }
    a.frames == 1
        && b.frames == 1
        && a.print.detailed
        && b.print.detailed
        && (a.aspect - b.aspect).abs() <= MAX_ASPECT_DIFFERENCE * a.aspect.max(b.aspect)
        // Cheapest checks first: this runs for every pair of images.
        && a.print.mean_rgb.iter().zip(b.print.mean_rgb)
            .all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= MAX_COLOR_DISTANCE)
        && {
            let metrics = compare(a, b);
            metrics.brightness_difference <= MAX_LUMA_DISTANCE
                && metrics.opacity_difference <= MAX_ALPHA_DISTANCE
                && metrics.max_cell_difference <= MAX_CELL_DISTANCE
        }
}

/// Group analyzed images that show the same picture.
pub(crate) fn group(resources: &[ResourceAnalysis]) -> Vec<SimilarGroup> {
    let mut entries: Vec<Entry> = resources
        .iter()
        .enumerate()
        .filter_map(|(index, resource)| {
            let print = resource.fingerprint.clone()?;
            let image = resource.image.as_ref()?;
            Some(Entry {
                index,
                bytes: resource.resource.bytes,
                sha256: resource.sha256.clone(),
                luma: decode(&print.luma)?,
                alpha: decode(&print.alpha)?,
                aspect: image.width as f64 / image.height.max(1) as f64,
                dimensions: (image.width, image.height),
                frames: image.frames,
                variant: variant_key(&resource.resource.path),
                print,
            })
        })
        .collect();
    // A fixed, largest-file reference makes every score interpretable. Do not
    // union transitive A~B~C matches: C must match A directly to join A's group.
    // This is deterministic and O(n²) without storing an O(n²) matrix.
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.index.cmp(&b.index)));
    let mut assigned = vec![false; entries.len()];
    let mut clusters = Vec::new();
    for a in 0..entries.len() {
        if assigned[a] {
            continue;
        }
        let mut members = vec![&entries[a]];
        for b in a + 1..entries.len() {
            if !assigned[b] && alike(&entries[a], &entries[b]) {
                assigned[b] = true;
                members.push(&entries[b]);
            }
        }
        if members.len() > 1 {
            assigned[a] = true;
            clusters.push(members);
        }
    }
    let mut groups: Vec<SimilarGroup> = clusters
        .into_iter()
        .filter(|members| members.len() > 1)
        .map(|mut members| {
            members.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.index.cmp(&b.index)));
            let first = members[0];
            let identical = members
                .iter()
                .all(|m| m.sha256.is_some() && m.sha256 == first.sha256);
            let resized = members.iter().any(|m| m.dimensions != first.dimensions)
                && members
                    .iter()
                    .all(|m| distance(&m.luma, &first.luma).0 <= RESIZED_DISTANCE);
            SimilarGroup {
                kind: if identical {
                    "identical"
                } else if resized {
                    "resized"
                } else {
                    "similar"
                }
                .into(),
                comparisons: members.iter().map(|m| compare(first, m)).collect(),
                redundant_bytes: members.iter().skip(1).map(|m| m.bytes).sum(),
                members: members.iter().map(|m| m.index).collect(),
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.redundant_bytes
            .cmp(&a.redundant_bytes)
            .then(a.members.cmp(&b.members))
    });
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageInfo, Resource};

    fn picture(width: usize, height: usize, paint: impl Fn(f32, f32) -> [f32; 4]) -> Decoded {
        let mut pixels = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, a] = paint(
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                );
                pixels.extend([r * a, g * a, b * a, a]);
            }
        }
        Decoded {
            info: ImageInfo {
                decoder_type: "test".into(),
                width,
                height,
                frames: 1,
                bits_per_component: 8,
                orientation: 1,
                transparent_pixels: 0,
                has_transparent_pixels: false,
            },
            pixels,
        }
    }

    /// A badge: colored disc with a lighter stripe, transparent outside.
    fn badge(tint: [f32; 3]) -> impl Fn(f32, f32) -> [f32; 4] {
        move |x, y| {
            let inside = (x - 0.5).powi(2) + (y - 0.5).powi(2) < 0.2;
            let stripe = (y - 0.35).abs() < 0.08 && x > 0.3;
            let shade = if stripe { 1.0 } else { 0.55 + 0.4 * x };
            [
                tint[0] * shade,
                tint[1] * shade,
                tint[2] * shade,
                if inside { 1.0 } else { 0.0 },
            ]
        }
    }

    fn row(path: &str, image: &Decoded, sha: &str) -> ResourceAnalysis {
        let mut row = ResourceAnalysis::new(&Resource::for_tests(path, "png"), "inspected");
        row.resource.bytes = (image.info.width * image.info.height) as u64;
        row.sha256 = Some(sha.repeat(64));
        row.image = Some(image.info.clone());
        row.fingerprint = fingerprint(image);
        row
    }

    #[test]
    fn the_same_picture_at_another_size_is_found_and_variants_are_not() {
        let red = badge([0.9, 0.2, 0.2]);
        let large = picture(300, 300, &red);
        let small = picture(96, 96, &red);
        let rows = vec![
            row("Feature/A/badge_big.png", &large, "a"),
            row("Feature/B/medal.png", &small, "b"),
            // Intended scale variants of one asset.
            row("Feature/C/icon@2x.png", &small, "c"),
            row("Feature/C/icon@3x.png", &large, "d"),
        ];
        let groups = group(&rows);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].kind, "resized");
        // All four show one picture, but @2x/@3x alone would not be a finding.
        assert_eq!(groups[0].members.len(), 4);
        let only_variants = group(&rows[2..]);
        assert!(only_variants.is_empty(), "{only_variants:?}");
    }

    #[test]
    fn identical_bytes_are_reported_as_identical_with_redundant_bytes() {
        let image = picture(64, 64, badge([0.2, 0.5, 0.9]));
        let rows = vec![
            row("a/one.png", &image, "a"),
            row("b/two.png", &image, "a"),
            row("c/three.png", &image, "a"),
        ];
        let groups = group(&rows);
        assert_eq!(groups[0].kind, "identical");
        assert_eq!(groups[0].redundant_bytes, 2 * 64 * 64);
    }

    #[test]
    fn different_tint_shape_or_alpha_is_not_a_duplicate() {
        let base = picture(128, 128, badge([0.9, 0.2, 0.2]));
        let blue = picture(128, 128, badge([0.2, 0.3, 0.9]));
        let square = picture(128, 128, |x, y| {
            let inside = (x - 0.5).abs() < 0.4 && (y - 0.5).abs() < 0.4;
            [0.9 * x, 0.2, 0.2 * y, if inside { 1.0 } else { 0.0 }]
        });
        let opaque = picture(128, 128, |x, y| {
            let [r, g, b, _] = badge([0.9, 0.2, 0.2])(x, y);
            [r, g, b, 1.0]
        });
        let rows = vec![
            row("a/base.png", &base, "a"),
            row("b/blue.png", &blue, "b"),
            row("c/square.png", &square, "c"),
            row("d/opaque.png", &opaque, "d"),
        ];
        assert!(group(&rows).is_empty(), "{:?}", group(&rows));
    }

    #[test]
    fn flat_images_only_match_by_exact_bytes() {
        let white = picture(40, 40, |_, _| [1.0, 1.0, 1.0, 1.0]);
        let nearly = picture(80, 80, |_, _| [0.99, 1.0, 1.0, 1.0]);
        assert!(!fingerprint(&white).unwrap().detailed);
        let rows = vec![row("a/w.png", &white, "a"), row("b/n.png", &nearly, "b")];
        assert!(group(&rows).is_empty());
        let same = vec![row("a/w.png", &white, "a"), row("b/w.png", &white, "a")];
        assert_eq!(group(&same)[0].kind, "identical");
    }

    #[test]
    fn android_density_and_locale_folders_of_one_name_are_variants() {
        let image = picture(64, 64, badge([0.3, 0.8, 0.4]));
        let big = picture(128, 128, badge([0.3, 0.8, 0.4]));
        let rows = vec![
            row("app/src/main/res/drawable-xhdpi/ic_ok.png", &image, "a"),
            row("app/src/main/res/drawable-xxxhdpi/ic_ok.png", &big, "b"),
            row(
                "app/src/main/res/drawable-ldrtl-xhdpi/ic_ok.png",
                &image,
                "a",
            ),
        ];
        assert!(group(&rows).is_empty());
        let mut with_copy = rows;
        with_copy.push(row(
            "module/room/src/main/res/drawable-xhdpi/ic_done.png",
            &image,
            "a",
        ));
        assert_eq!(group(&with_copy).len(), 1);
    }

    #[test]
    fn scores_distinguish_exact_reencoded_and_small_tint_changes() {
        let base = picture(128, 128, badge([0.9, 0.2, 0.2]));
        let slight = picture(128, 128, badge([0.89, 0.2, 0.2]));
        let stronger = picture(128, 128, badge([0.86, 0.2, 0.2]));
        let rows = vec![
            row("a/base.png", &base, "a"),
            row("b/exact.png", &base, "a"),
            row("c/reencoded.png", &base, "b"),
            row("d/slight.png", &slight, "c"),
            row("e/stronger.png", &stronger, "d"),
        ];
        let groups = group(&rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, vec![0, 1, 2, 3, 4]);
        let scores: Vec<_> = groups[0].comparisons.iter().map(|m| m.score).collect();
        assert_eq!(scores[1], 100.0);
        assert_eq!(scores[2], 99.9);
        assert!(scores[2] > scores[3] && scores[3] > scores[4], "{scores:?}");
        assert!(!groups[0].comparisons[2].identical);
        println!(
            "exact / reencoded / slight tint / stronger tint: {:?}",
            &scores[1..]
        );
    }

    #[test]
    fn a_similarity_chain_cannot_claim_a_match_to_the_reference() {
        // A~B and B~C pass the mean threshold, but A~C does not.
        let image = picture(64, 64, badge([0.2, 0.5, 0.9]));
        let rows: Vec<_> = [100, 105, 110]
            .iter()
            .enumerate()
            .map(|(i, level)| {
                let mut resource = row(&format!("{i}/image.png"), &image, &i.to_string());
                resource.fingerprint.as_mut().unwrap().luma =
                    format!("{level:02x}").repeat(GRID * GRID);
                resource
            })
            .collect();
        assert_eq!(group(&rows[..2])[0].members.len(), 2);
        assert_eq!(group(&rows[1..])[0].members.len(), 2);
        assert!(group(&[rows[0].clone(), rows[2].clone()]).is_empty());
        let groups = group(&rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, vec![0, 1]);
        assert_eq!(groups[0].comparisons.len(), 2);
    }

    #[test]
    fn a_local_opacity_change_is_not_hidden_by_the_average() {
        let image = picture(64, 64, |x, y| [x, y, 0.5, 1.0]);
        let base = row("a/base.png", &image, "a");
        let mut changed = row("b/changed.png", &image, "b");
        // One cell becomes transparent. Its mean error is only 1/256.
        // Keep luma fixed to isolate (ablate) the alpha guard.
        changed.fingerprint.as_mut().unwrap().alpha = format!("00{}", "ff".repeat(GRID * GRID - 1));
        assert!(group(&[base, changed]).is_empty());
    }

    #[test]
    fn scores_follow_the_largest_reference_and_legacy_reports_remain_readable() {
        let image = picture(64, 64, badge([0.2, 0.5, 0.9]));
        let mut small = row("a/small.png", &image, "a");
        small.resource.bytes = 10;
        let large = row("b/large.png", &image, "b");
        let groups = group(&[small, large]);
        assert_eq!(groups[0].members, vec![1, 0]);
        assert!(groups[0].comparisons[0].identical);
        assert!(!groups[0].comparisons[1].identical);
        let json = serde_json::to_string(&groups).unwrap();
        let restored: Vec<SimilarGroup> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored[0].comparisons[1].score, 99.9);
        let legacy: SimilarGroup =
            serde_json::from_str(r#"{"kind":"similar","members":[0,1],"redundant_bytes":10}"#)
                .unwrap();
        assert!(legacy.comparisons.is_empty());
    }

    #[test]
    fn matching_posters_do_not_prove_matching_animations() {
        let image = picture(64, 64, badge([0.2, 0.5, 0.9]));
        let still = row("a/still.png", &image, "a");
        let mut animation = row("b/animated.png", &image, "b");
        animation.image.as_mut().unwrap().frames = 2;
        assert!(group(&[still, animation.clone()]).is_empty());
        let mut copy = animation.clone();
        copy.resource.path = "c/copy.png".into();
        assert_eq!(group(&[animation, copy])[0].comparisons[1].score, 100.0);
    }

    #[test]
    fn tiny_images_and_malformed_fingerprints_are_handled() {
        let tiny = picture(5, 3, |x, y| [x, y, 0.5, 1.0]);
        let print = fingerprint(&tiny).unwrap();
        assert_eq!(print.luma.len(), GRID * GRID * 2);
        assert_eq!(print.alpha, "");
        let mut broken = row("a/x.png", &tiny, "a");
        broken.fingerprint.as_mut().unwrap().luma = "zz".into();
        assert!(group(&[broken, row("b/y.png", &tiny, "b")]).is_empty());
    }
}
