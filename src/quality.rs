use crate::image_backend::Decoded;
use anyhow::{Context, Result, ensure};
use fast_ssim2::{LinearRgbImage, compute_ssimulacra2, compute_ssimulacra2_strip, srgb_to_linear};

/// Backdrops for alpha-aware scoring: black, white and mid-gray in sRGB.
const BACKDROPS: [f32; 3] = [0.0, 1.0, 0.5];
/// Above this size the metric runs in strips so peak memory stays bounded.
const STRIP_THRESHOLD_PIXELS: usize = 2 * 1024 * 1024;
const STRIP_HEIGHT: u32 = 256;

/// Worst SSIMULACRA2 score across the backdrops (100 = identical, 90+ is
/// usually imperceptible). Opaque pairs are scored once.
pub(crate) fn ssimulacra2(original: &Decoded, candidate: &Decoded) -> Result<f64> {
    ensure!(
        original.info.width == candidate.info.width
            && original.info.height == candidate.info.height,
        "dimensions_changed"
    );
    if original.pixels == candidate.pixels {
        return Ok(100.0);
    }
    let opaque = !original.info.has_transparent_pixels && !candidate.info.has_transparent_pixels;
    let backdrops = if opaque {
        &BACKDROPS[..1]
    } else {
        &BACKDROPS[..]
    };
    let mut worst = f64::INFINITY;
    for backdrop in backdrops {
        let source = composite(original, *backdrop)?;
        let distorted = composite(candidate, *backdrop)?;
        let score = if original.info.width * original.info.height > STRIP_THRESHOLD_PIXELS {
            compute_ssimulacra2_strip(source, distorted, STRIP_HEIGHT)
        } else {
            compute_ssimulacra2(source, distorted)
        }
        .context("ssimulacra2_failed")?;
        worst = worst.min(score);
    }
    Ok(worst)
}

/// Reuse the source pyramid across quality/format trials of one image.
/// Cap retained pyramids to about 96 MiB per worker, keeping large images on strips.
#[cfg(feature = "native")]
pub(crate) struct ReferenceScorer<'a> {
    original: &'a Decoded,
    references: [Option<fast_ssim2::Ssimulacra2Reference>; 3],
}
#[cfg(feature = "native")]
impl<'a> ReferenceScorer<'a> {
    pub fn new(original: &'a Decoded) -> Self {
        Self {
            original,
            references: [None, None, None],
        }
    }
    pub fn score(&mut self, candidate: &Decoded) -> Result<f64> {
        let original = self.original;
        ensure!(
            original.info.width == candidate.info.width
                && original.info.height == candidate.info.height,
            "dimensions_changed"
        );
        if original.pixels == candidate.pixels {
            return Ok(100.0);
        }
        let count = if original.info.has_transparent_pixels || candidate.info.has_transparent_pixels
        {
            3
        } else {
            1
        };
        if original.info.width * original.info.height * count > 2 * 1024 * 1024 {
            return ssimulacra2(original, candidate);
        }
        let mut worst = f64::INFINITY;
        for (index, backdrop) in BACKDROPS.iter().enumerate().take(count) {
            if self.references[index].is_none() {
                self.references[index] = Some(
                    fast_ssim2::Ssimulacra2Reference::new(composite(original, *backdrop)?)
                        .context("ssimulacra2_reference_failed")?,
                );
            }
            let score = self.references[index]
                .as_ref()
                .unwrap()
                .compare(composite(candidate, *backdrop)?)
                .context("ssimulacra2_failed")?;
            worst = worst.min(score);
        }
        Ok(worst)
    }
}

/// Premultiplied sRGB over a solid backdrop, converted to linear light.
fn composite(image: &Decoded, backdrop: f32) -> Result<LinearRgbImage> {
    let data = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            let uncovered = (1.0 - pixel[3].clamp(0.0, 1.0)) * backdrop;
            [0, 1, 2].map(|channel| srgb_to_linear((pixel[channel] + uncovered).clamp(0.0, 1.0)))
        })
        .collect();
    LinearRgbImage::try_new(data, image.info.width, image.info.height)
        .context("ssimulacra2_invalid_image")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImageInfo;

    const SIDE: usize = 64;

    fn image(pixel: impl Fn(usize, usize) -> [f32; 4]) -> Decoded {
        sized(SIDE, SIDE, pixel)
    }

    fn sized(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [f32; 4]) -> Decoded {
        let pixels: Vec<f32> = (0..width * height)
            .flat_map(|i| pixel(i % width, i / width))
            .collect();
        let transparent_pixels = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] < 1.0)
            .count();
        Decoded {
            info: ImageInfo {
                decoder_type: "test".into(),
                width,
                height,
                frames: 1,
                bits_per_component: 8,
                orientation: 1,
                transparent_pixels,
                has_transparent_pixels: transparent_pixels > 0,
            },
            pixels,
        }
    }

    fn gradient(x: usize, y: usize) -> [f32; 4] {
        [x as f32 / SIDE as f32, y as f32 / SIDE as f32, 0.5, 1.0]
    }

    #[test]
    fn tiny_images_are_scored_instead_of_failing() {
        for (width, height) in [(1, 1), (3, 7), (8, 2)] {
            let original = sized(width, height, |_, _| [0.2, 0.4, 0.6, 1.0]);
            let changed = sized(width, height, |_, _| [0.9, 0.1, 0.1, 1.0]);
            let same = ssimulacra2(&original, &original).unwrap();
            assert!((same - 100.0).abs() < 0.01, "{width}x{height}: {same}");
            assert!(ssimulacra2(&original, &changed).unwrap() < same);
        }
    }

    #[test]
    fn identical_images_score_100() {
        let score = ssimulacra2(&image(gradient), &image(gradient)).unwrap();
        assert!((score - 100.0).abs() < 0.01, "{score}");
    }

    #[cfg(feature = "native")]
    #[test]
    fn cached_reference_matches_one_shot_for_repeated_alpha_and_rgb_trials() {
        for alpha in [1.0, 0.5] {
            let original = image(|x, y| {
                let [r, g, b, _] = gradient(x, y);
                [r * alpha, g * alpha, b * alpha, alpha]
            });
            let mut scorer = ReferenceScorer::new(&original);
            for delta in [0.0, 0.02, 0.1] {
                let changed = image(|x, y| {
                    let [r, g, b, _] = gradient(x, y);
                    [(r + delta).min(1.0) * alpha, g * alpha, b * alpha, alpha]
                });
                let expected = ssimulacra2(&original, &changed).unwrap();
                let actual = scorer.score(&changed).unwrap();
                assert!((expected - actual).abs() < 0.001, "{expected} vs {actual}");
            }
        }
    }

    #[test]
    fn heavier_distortion_scores_lower() {
        let posterize = |levels: f32| {
            image(move |x, y| {
                let [r, g, b, a] = gradient(x, y);
                [
                    (r * levels).round() / levels,
                    (g * levels).round() / levels,
                    b,
                    a,
                ]
            })
        };
        let original = image(gradient);
        let mild = ssimulacra2(&original, &posterize(24.0)).unwrap();
        let heavy = ssimulacra2(&original, &posterize(4.0)).unwrap();
        assert!(heavy < mild && mild < 100.0, "mild {mild}, heavy {heavy}");
    }

    #[test]
    fn alpha_loss_invisible_on_black_is_still_penalized() {
        // Opaque black and fully transparent look the same over black only.
        let checker = |x: usize, y: usize| (x / 8 + y / 8).is_multiple_of(2);
        let original = image(|x, y| [0.0, 0.0, 0.0, if checker(x, y) { 1.0 } else { 0.5 }]);
        let candidate = image(|x, y| [0.0, 0.0, 0.0, if checker(x, y) { 0.0 } else { 0.5 }]);
        let score = ssimulacra2(&original, &candidate).unwrap();
        assert!(score < 50.0, "{score}");
    }
}
