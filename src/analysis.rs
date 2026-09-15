use crate::{
    ImageDifference, ImageInfo, Policy, Resource, ResourceInventory,
    filesystem::{contained_file, hash, write_new},
    image_backend, optimizer,
    resources::{bounded_read, inventory},
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisOptions {
    pub qualities: Vec<u8>,
    pub jobs: usize,
    /// All sizes are included by default, unlike the legacy lossless plan.
    pub min_input_bytes: u64,
    pub min_savings_bytes: u64,
    pub probe_only: bool,
    /// Lossy HEIC may quantize alpha. 0 requires exact alpha samples.
    pub max_alpha_error: f32,
}
impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            qualities: vec![75, 85, 95],
            jobs: 2,
            min_input_bytes: 0,
            min_savings_bytes: 1,
            probe_only: false,
            max_alpha_error: 1.0 / 255.0 + 0.000001,
        }
    }
}
impl AnalysisOptions {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.max_alpha_error.is_finite() && (0.0..=1.0).contains(&self.max_alpha_error),
            "max_alpha_error must be 0..=1"
        );
        ensure!((1..=8).contains(&self.jobs), "jobs must be 1..=8");
        ensure!(
            !self.qualities.is_empty()
                && self.qualities.len() <= 8
                && self.qualities.iter().all(|q| (1..=100).contains(q)),
            "qualities must contain 1..=8 values in 1..=100"
        );
        let mut qualities = self.qualities.clone();
        qualities.sort_unstable();
        qualities.dedup();
        ensure!(
            qualities.len() == self.qualities.len(),
            "duplicate quality values"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCandidate {
    pub format: String,
    pub quality: Option<u8>,
    pub lossy: bool,
    pub bytes: u64,
    pub savings_bytes: u64,
    pub valid: bool,
    pub rejection: Option<String>,
    pub difference: Option<ImageDifference>,
    pub artifact: Option<PathBuf>,
    pub preview: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAnalysis {
    pub resource: Resource,
    pub sha256: Option<String>,
    pub image: Option<ImageInfo>,
    pub status: String,
    pub issues: Vec<String>,
    pub candidates: Vec<ImageCandidate>,
    /// A size winner, not an assertion of acceptable visual quality.
    pub smallest_candidate: Option<usize>,
    pub original_preview: Option<PathBuf>,
    pub original_artifact: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub root: PathBuf,
    pub backend: String,
    pub options: AnalysisOptions,
    pub inventory: ResourceInventory,
    pub resources: Vec<ResourceAnalysis>,
    pub status_counts: BTreeMap<String, usize>,
    pub potential_source_bytes_saved: u64,
}

/// Read-only analysis of all inventoried resources, with lossy candidates staged
/// solely for review. This report is deliberately not an executable apply plan.
pub fn analyze(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
    options: AnalysisOptions,
) -> Result<AnalysisReport> {
    analyze_with_progress(root, out, options, |_, _| {})
}

pub fn analyze_with_progress(
    root: impl AsRef<Path>,
    out: impl AsRef<Path>,
    options: AnalysisOptions,
    progress: impl Fn(usize, usize) + Sync,
) -> Result<AnalysisReport> {
    options.validate()?;
    if !options.probe_only {
        image_backend::check_encoders()?;
    }
    let inventory = inventory(root)?;
    let out = out.as_ref();
    let parent = fs::canonicalize(
        out.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let out = parent.join(out.file_name().context("output directory has no name")?);
    ensure!(
        !out.starts_with(&inventory.root),
        "analysis output must be outside the scanned project"
    );
    fs::create_dir(&out).context("analysis output must be a new directory")?;
    fs::create_dir(out.join("candidates"))?;
    fs::create_dir(out.join("previews"))?;
    fs::create_dir(out.join("originals"))?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.jobs)
        .build()?;
    let complete = AtomicUsize::new(0);
    let resources = pool.install(|| {
        inventory
            .assets
            .par_iter()
            .enumerate()
            .map(|(index, resource)| {
                #[cfg(target_os = "macos")]
                let result = objc2::rc::autoreleasepool(|_| {
                    analyze_resource(resource, index, &inventory.root, &out, &options)
                });
                #[cfg(not(target_os = "macos"))]
                let result = analyze_resource(resource, index, &inventory.root, &out, &options);
                let done = complete.fetch_add(1, Ordering::Relaxed) + 1;
                progress(done, inventory.assets.len());
                result
            })
            .collect::<Vec<_>>()
    });
    let mut status_counts = BTreeMap::new();
    let mut savings = 0;
    for resource in &resources {
        *status_counts.entry(resource.status.clone()).or_insert(0) += 1;
        if let Some(index) = resource.smallest_candidate {
            savings += resource.candidates[index].savings_bytes;
        }
    }
    let report = AnalysisReport {
        schema_version: 1,
        root: inventory.root.clone(),
        backend: if image_backend::image_backend_available() {
            "Apple ImageIO + CoreGraphics sRGB float comparison"
        } else {
            "ImageIO unavailable on this platform"
        }
        .into(),
        options,
        inventory,
        resources,
        status_counts,
        potential_source_bytes_saved: savings,
    };
    write_new(
        &out.join("analysis.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    write_new(&out.join("report.html"), render_html(&report).as_bytes())?;
    Ok(report)
}

fn analyze_resource(
    resource: &Resource,
    index: usize,
    root: &Path,
    out: &Path,
    options: &AnalysisOptions,
) -> ResourceAnalysis {
    let mut result = ResourceAnalysis {
        resource: resource.clone(),
        sha256: None,
        image: None,
        status: "inventory_only".into(),
        issues: vec![],
        candidates: vec![],
        smallest_candidate: None,
        original_preview: None,
        original_artifact: None,
    };
    if resource.kind != "image" {
        result.issues.push(format!(
            "{}_optimization_backend_not_implemented",
            resource.kind
        ));
        return result;
    }
    let attempt = (|| -> Result<()> {
        let path = contained_file(root, &resource.path)?;
        let original = bounded_read(&path)?;
        result.sha256 = Some(hash(&original));
        let decoded = image_backend::decode(&original)?;
        result.image = Some(decoded.info.clone());
        result.status = "inspected".into();
        if decoded.info.frames != 1 {
            result.issues.push("multiple_frames_not_transcoded".into());
            return Ok(());
        }
        if let Some(reason) = &resource.conversion_exclusion {
            result.issues.push(reason.clone());
            return Ok(());
        }
        if options.probe_only {
            return Ok(());
        }
        if original.len() < options.min_input_bytes as usize {
            result.issues.push("below_explicit_input_threshold".into());
            return Ok(());
        }
        let targets = if decoded.info.has_transparent_pixels {
            vec!["heic"]
        } else {
            vec!["jpeg", "heic"]
        };
        let mut trials: Vec<(&str, Option<u8>)> = targets
            .iter()
            .flat_map(|format| {
                options
                    .qualities
                    .iter()
                    .map(move |quality| (*format, Some(*quality)))
            })
            .collect();
        if resource.format == "png" {
            trials.insert(0, ("png", None));
        }
        for (format, quality) in trials {
            let mut trial = ImageCandidate {
                format: format.into(),
                quality,
                lossy: quality.is_some(),
                bytes: 0,
                savings_bytes: 0,
                valid: false,
                rejection: None,
                difference: None,
                artifact: None,
                preview: None,
            };
            let encoded = match quality {
                Some(quality) => image_backend::encode(&original, format, quality),
                None => optimizer::optimize(&original, &Policy::default()),
            };
            let checked = (|| -> Result<()> {
                let bytes = encoded?;
                trial.bytes = bytes.len() as u64;
                trial.savings_bytes = (original.len() as u64).saturating_sub(trial.bytes);
                let candidate = image_backend::decode(&bytes)?;
                let difference = image_backend::compare(&decoded, &candidate)?;
                trial.difference = Some(difference.clone());
                ensure!(
                    difference.max_alpha_error <= options.max_alpha_error,
                    "alpha_error_exceeds_policy"
                );
                ensure!(
                    decoded.info.has_transparent_pixels == candidate.info.has_transparent_pixels,
                    "transparency_presence_changed"
                );
                trial.valid = true;
                if trial.savings_bytes >= options.min_savings_bytes
                    && trial.bytes < original.len() as u64
                {
                    let stem = format!("{index}-{format}-{}", quality.unwrap_or(0));
                    let artifact = PathBuf::from(format!("candidates/{stem}.{format}"));
                    let preview = PathBuf::from(format!("previews/{stem}.png"));
                    write_new(&out.join(&artifact), &bytes)?;
                    write_new(&out.join(&preview), &image_backend::preview(&candidate)?)?;
                    trial.artifact = Some(artifact);
                    trial.preview = Some(preview);
                }
                Ok(())
            })();
            if let Err(error) = checked {
                trial.valid = false;
                trial.rejection = Some(format!("{error:#}"));
            }
            result.candidates.push(trial);
        }
        result.smallest_candidate = result
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.valid && candidate.artifact.is_some())
            .min_by_key(|(_, candidate)| candidate.bytes)
            .map(|(index, _)| index);
        if result.smallest_candidate.is_some() {
            let preview = PathBuf::from(format!("previews/{index}-original.png"));
            write_new(&out.join(&preview), &image_backend::preview(&decoded)?)?;
            result.original_preview = Some(preview);
            let artifact = PathBuf::from(format!("originals/{index}.{}", resource.format));
            write_new(&out.join(&artifact), &original)?;
            result.original_artifact = Some(artifact);
            result.status = "candidates_available".into();
        }
        // Report a moving source instead of presenting stale candidate estimates.
        ensure!(
            hash(&bounded_read(&path)?)
                == *result.sha256.as_ref().context("source hash missing")?,
            "source_changed_during_analysis"
        );
        Ok(())
    })();
    if let Err(error) = attempt {
        result.status = "failed".into();
        result.issues.push(format!("{error:#}"));
        result.smallest_candidate = None;
    }
    result
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_html(report: &AnalysisReport) -> String {
    use std::fmt::Write;
    let mut html = String::from(
        "<!doctype html><html lang=zh-CN><meta charset=utf-8><title>resopt 资源分析</title><style>body{font:15px system-ui;margin:32px;line-height:1.6;color:#222}table{border-collapse:collapse;width:100%}td,th{padding:8px;border-bottom:1px solid #ddd;text-align:left}img{max-width:256px;max-height:256px;background:repeating-conic-gradient(#ddd 0% 25%,#fff 0% 50%) 0/16px 16px}section{margin:24px 0;padding:16px;border:1px solid #ddd}code{word-break:break-all}.variants{display:flex;flex-wrap:wrap;gap:20px}figure{margin:0;max-width:280px}small{color:#666}</style><h1>resopt 资源分析</h1><p>此报告只生成候选，不修改项目。JPEG/HEIC 为有损编码；质量数值不是节省比例。体积最小不代表画质最佳。预览缩略图仅供初筛，正式选用前应检查原尺寸候选。MAE/PSNR 在统一 sRGB 预乘 Alpha 像素上计算，不是视觉验收。</p>",
    );
    let _ = write!(
        html,
        "<p>资源文件：{} · 有候选：{} · 按每个文件最小候选估算可节省 {} 字节（源文件体积）</p>",
        report.resources.len(),
        report
            .status_counts
            .get("candidates_available")
            .unwrap_or(&0),
        report.potential_source_bytes_saved
    );
    html.push_str("<p>不支持优化的音视频、动效、字体、压缩包等仍列入清单；不会解包或改写。详情见同目录 analysis.json。编译包体与构建目标归属未测量。</p>");
    for resource in &report.resources {
        let _ = write!(
            html,
            "<section><code>{}</code><p>{} · {} bytes · {}</p>",
            escaped(&resource.resource.path.to_string_lossy()),
            escaped(&resource.resource.format),
            resource.resource.bytes,
            escaped(&resource.status)
        );
        if let Some(info) = &resource.image {
            let _ = write!(
                html,
                "<p>{}×{} · {} 帧 · 透明像素 {} · JPEG {}</p>",
                info.width,
                info.height,
                info.frames,
                info.transparent_pixels,
                if info.has_transparent_pixels {
                    "禁止"
                } else {
                    "可比较"
                }
            );
        }
        if !resource.issues.is_empty() {
            let _ = write!(
                html,
                "<small>{}</small>",
                escaped(&resource.issues.join("; "))
            );
        }
        html.push_str("<div class=variants>");
        if let (Some(preview), Some(artifact)) =
            (&resource.original_preview, &resource.original_artifact)
        {
            let _ = write!(
                html,
                "<figure><a href=\"{}\"><img loading=lazy src=\"{}\"></a><figcaption>原图（缩略图，点击查看原文件）</figcaption></figure>",
                escaped(&artifact.to_string_lossy()),
                escaped(&preview.to_string_lossy())
            );
        }
        for candidate in &resource.candidates {
            if let (Some(preview), Some(artifact)) = (&candidate.preview, &candidate.artifact) {
                let _ = write!(
                    html,
                    "<figure><a href=\"{}\"><img loading=lazy src=\"{}\"></a><figcaption>{} · 质量 {} · {} bytes<br>节省 {} bytes</figcaption></figure>",
                    escaped(&artifact.to_string_lossy()),
                    escaped(&preview.to_string_lossy()),
                    escaped(&candidate.format),
                    candidate.quality.map_or("无损".into(), |q| q.to_string()),
                    candidate.bytes,
                    candidate.savings_bytes
                );
            }
        }
        html.push_str("</div><table><tr><th>格式 / 质量</th><th>字节</th><th>RGB MAE</th><th>PSNR dB</th><th>Alpha 最大误差</th><th>结果</th></tr>");
        for candidate in &resource.candidates {
            let (mae, psnr) =
                candidate
                    .difference
                    .as_ref()
                    .map_or(("—".into(), "—".into()), |d| {
                        (
                            format!("{:.3}", d.rgb_mae_255),
                            d.psnr_db.map_or("∞".into(), |p| format!("{p:.2}")),
                        )
                    });
            let alpha = candidate
                .difference
                .as_ref()
                .map_or("—".into(), |d| format!("{:.6}", d.max_alpha_error));
            let _ = write!(
                html,
                "<tr><td>{} / {}</td><td>{}</td><td>{mae}</td><td>{psnr}</td><td>{alpha}</td><td>{}</td></tr>",
                escaped(&candidate.format),
                candidate.quality.map_or("无损".into(), |q| q.to_string()),
                candidate.bytes,
                escaped(candidate.rejection.as_deref().unwrap_or(
                    if candidate.artifact.is_some() {
                        "可审阅"
                    } else {
                        "无足够体积收益"
                    }
                ))
            );
        }
        html.push_str("</table></section>");
    }
    html.push_str("</html>");
    html
}
