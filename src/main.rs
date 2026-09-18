use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use resopt::{AnalysisOptions, Policy, analyze_with_progress, apply, create_plan, restore};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "resopt",
    version,
    about = "Find, review and safely apply resource optimizations for Apple and Android projects"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Options shared by `web` and `analyze`.
#[derive(Args)]
struct AnalysisArgs {
    /// Encoder quality parameters for lossy candidates (not savings percentages).
    #[arg(long, value_delimiter = ',', default_value = "75,85,95")]
    qualities: Vec<u8>,
    /// Parallel image workers; 0 uses the CPU count, capped at 8.
    #[arg(long, default_value_t = 0)]
    jobs: usize,
    /// Largest decoded image to analyze, in pixels (16 bytes each per decode).
    #[arg(long, default_value_t = resopt::DEFAULT_MAX_PIXELS)]
    max_pixels: usize,
    /// oxipng effort (0..=6) for the lossless PNG candidate.
    #[arg(long, default_value_t = Policy::default().png_level)]
    png_level: u8,
    /// Allow lossless PNG color-type, bit-depth and palette reductions.
    #[arg(long)]
    png_reductions: bool,
    /// Also compare WebP candidates for loose files and Android resources.
    #[arg(long)]
    webp: bool,
    /// Lowest SSIMULACRA2 score a lossy candidate may have and still be recommended.
    #[arg(long, default_value_t = AnalysisOptions::default().min_score)]
    min_score: f64,
    /// Maximum per-pixel alpha error (0 is exact).
    #[arg(long, default_value_t = AnalysisOptions::default().max_alpha_error)]
    max_alpha_error: f32,
    /// Android minSdk when it cannot be read from Gradle files.
    #[arg(long)]
    android_min_sdk: Option<u32>,
    /// Include files matched by Git ignore rules.
    #[arg(long)]
    include_ignored: bool,
    /// Do not read or write the persistent analysis cache.
    #[arg(long)]
    no_cache: bool,
    /// Cache directory (default: the per-user cache directory).
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

impl AnalysisArgs {
    fn into_options(self) -> AnalysisOptions {
        AnalysisOptions {
            qualities: self.qualities,
            jobs: self.jobs,
            max_pixels: self.max_pixels,
            png_level: self.png_level,
            png_reductions: self.png_reductions,
            webp: self.webp,
            min_score: self.min_score,
            max_alpha_error: self.max_alpha_error,
            android_min_sdk: self.android_min_sdk,
            include_ignored: self.include_ignored,
            cache_dir: if self.no_cache {
                None
            } else {
                self.cache_dir.or_else(resopt::cache_directory)
            },
            ..AnalysisOptions::default()
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a local project and open its loopback-only review UI. No uploads.
    Web {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// New report directory outside the project; defaults to a retained temporary directory.
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long)]
        no_open: bool,
        #[command(flatten)]
        analysis: AnalysisArgs,
    },
    /// Report embedded and optional tools. Installs nothing.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Inventory all resource files, including loose files and actual image formats.
    Scan {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        json: bool,
        /// Use the legacy catalog-rendition-only inventory.
        #[arg(long)]
        catalog_only: bool,
        /// Include files matched by Git ignore rules.
        #[arg(long)]
        include_ignored: bool,
    },
    /// Probe all images and compare lossless PNG / JPEG / HEIC candidates without modifying sources.
    Analyze {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// New output directory outside the project: JSON, HTML, previews, candidates.
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        min_input_bytes: u64,
        /// Decode and inspect only; produce no candidates.
        #[arg(long)]
        probe_only: bool,
        /// Print per-phase timings to stderr.
        #[arg(long)]
        timings: bool,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        analysis: AnalysisArgs,
    },
    /// Refresh report.html from saved analysis.json without re-encoding resources.
    Report { directory: PathBuf },
    /// Open a loopback report server with per-image apply and restore.
    Serve {
        directory: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Stage verified PNG candidates and originals into a new plan directory.
    Plan {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// New directory containing plan.json, originals/, and candidates/.
        #[arg(long)]
        out: PathBuf,
        /// TOML file with lossless PNG policy settings.
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Include files matched by Git ignore rules.
        #[arg(long)]
        include_ignored: bool,
    },
    /// Apply a reviewed PNG plan, or batch-apply candidates from an analysis report.
    ///
    /// For an analysis report the default policy applies verified lossless,
    /// same-format candidates only; the flags below widen it explicitly.
    Apply {
        directory: PathBuf,
        #[arg(long)]
        json: bool,
        /// Report only: also apply lossy candidates that pass every check.
        #[arg(long)]
        lossy: bool,
        /// Report only: do not apply lossless candidates.
        #[arg(long)]
        no_lossless: bool,
        /// Report only: allow format changes (renames files, migrates references).
        #[arg(long)]
        cross_format: bool,
        /// Report only: accept a warning kind for the whole batch
        /// (alpha_error_exceeds_policy, transparency_presence_changed, quality_below_policy).
        #[arg(long = "accept-warning")]
        accept_warnings: Vec<String>,
        /// Report only: extra perceptual-score floor for lossy candidates.
        #[arg(long)]
        min_score: Option<f64>,
        /// Report only: limit target formats, e.g. `--format png,webp`.
        #[arg(long = "format", value_delimiter = ',')]
        formats: Vec<String>,
        /// Report only: show what would be applied without changing files.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show or clear the persistent analysis cache.
    Cache {
        /// Delete every cached result.
        #[arg(long)]
        clear: bool,
    },
    /// Restore originals, refusing to overwrite files edited since application.
    Restore {
        directory: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("resopt: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match cli.command {
        Commands::Web {
            root,
            out,
            port,
            no_open,
            analysis,
        } => {
            resopt::web(
                root,
                resopt::WebOptions {
                    out,
                    port,
                    no_open,
                    analysis: analysis.into_options(),
                },
            )?;
        }
        Commands::Serve { directory, port } => resopt::serve(directory, port)?,
        Commands::Report { directory } => {
            let path = resopt::refresh_report(directory)?;
            writeln!(
                stdout,
                "Updated {} (measurements and resources unchanged)",
                path.display()
            )?;
        }
        Commands::Doctor { json } => {
            let report = resopt::capabilities();
            if json {
                output_json(&mut stdout, &report)?;
            } else {
                writeln!(stdout, "resopt {} on {}", report.version, report.platform)?;
                for feature in &report.features {
                    writeln!(
                        stdout,
                        "{:<14} {}{}",
                        feature.id,
                        if feature.available {
                            "available"
                        } else {
                            "unavailable"
                        },
                        if feature.note.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", feature.note)
                        }
                    )?;
                }
                writeln!(stdout, "\nOptional tools (resopt installs nothing):")?;
                for tool in &report.tools {
                    if tool.available {
                        writeln!(stdout, "{:<14} found — {}", tool.name, tool.purpose)?;
                    } else {
                        writeln!(
                            stdout,
                            "{:<14} not found — {} Install: {}",
                            tool.name, tool.purpose, tool.install
                        )?;
                    }
                }
            }
        }
        Commands::Cache { clear } => {
            let directory =
                resopt::cache_directory().context("no cache directory on this platform")?;
            if clear && directory.exists() {
                fs::remove_dir_all(&directory)
                    .with_context(|| format!("clearing {}", directory.display()))?;
            }
            writeln!(
                stdout,
                "{}{}",
                directory.display(),
                if clear { " (cleared)" } else { "" }
            )?;
        }
        Commands::Scan {
            root,
            json,
            catalog_only: true,
            include_ignored,
        } => {
            let inventory =
                resopt::scan_with_options(root, resopt::ScanOptions { include_ignored })?;
            if json {
                output_json(&mut stdout, &inventory)?;
            } else {
                writeln!(
                    stdout,
                    "{} catalogs; {} referenced files; {} eligible for PNG analysis",
                    inventory.catalogs,
                    inventory.assets.len(),
                    inventory
                        .assets
                        .iter()
                        .filter(|asset| asset.eligible)
                        .count()
                )?;
                for asset in &inventory.assets {
                    writeln!(
                        stdout,
                        "{}\t{} bytes\t{}",
                        asset.path.display(),
                        asset.bytes,
                        asset.reason.as_deref().unwrap_or("eligible")
                    )?;
                }
                for diagnostic in &inventory.diagnostics {
                    eprintln!("warning: {diagnostic}");
                }
            }
        }
        Commands::Scan {
            root,
            json,
            catalog_only: false,
            include_ignored,
        } => {
            let report =
                resopt::inventory_with_options(root, resopt::ScanOptions { include_ignored })?;
            if json {
                output_json(&mut stdout, &report)?;
            } else {
                writeln!(
                    stdout,
                    "{} catalogs; {} resource files; {} source/tooling files excluded",
                    report.catalogs,
                    report.assets.len(),
                    report.skipped_source_or_tooling_files
                )?;
                for resource in &report.assets {
                    writeln!(
                        stdout,
                        "{}\t{} bytes\t{}\t{}{}",
                        resource.path.display(),
                        resource.bytes,
                        resource.kind,
                        resource.format,
                        if resource.extension_mismatch {
                            " (extension mismatch)"
                        } else {
                            ""
                        }
                    )?;
                }
                for diagnostic in &report.diagnostics {
                    eprintln!("warning: {diagnostic}");
                }
            }
        }
        Commands::Analyze {
            root,
            out,
            min_input_bytes,
            probe_only,
            timings,
            json,
            analysis,
        } => {
            let options = AnalysisOptions {
                min_input_bytes,
                probe_only,
                ..analysis.into_options()
            };
            let report = analyze_with_progress(root, &out, options, |done, total| {
                if done % 25 == 0 || done == total {
                    eprintln!("analyze {done}/{total}");
                }
            })?;
            if json {
                output_json(&mut stdout, &report)?;
            } else {
                writeln!(
                    stdout,
                    "Analyzed {} resources; {} with image candidates; {} potential source bytes saved",
                    report.resources.len(),
                    report
                        .status_counts
                        .get("candidates_available")
                        .unwrap_or(&0),
                    report.potential_source_bytes_saved
                )?;
                writeln!(
                    stdout,
                    "Review {}/report.html and analysis.json. Lossy candidates require visual review; sources were not modified.",
                    out.display()
                )?;
                for (status, count) in &report.status_counts {
                    writeln!(stdout, "{status}: {count}")?;
                }
            }
            if let (true, Some(performance)) = (timings, &report.performance) {
                eprintln!(
                    "wall {:.2}s; first result {}; {} workers; {} cache hits; {} duplicates reused",
                    performance.wall_seconds,
                    performance
                        .first_result_seconds
                        .map_or("n/a".into(), |s| format!("{s:.2}s")),
                    performance.workers,
                    performance.cache_hits,
                    performance.duplicate_reuses
                );
                for (phase, seconds) in &performance.phase_seconds {
                    eprintln!("  {phase:<16} {seconds:>8.2}s (summed over workers)");
                }
            }
        }
        Commands::Plan {
            root,
            out,
            policy,
            json,
            include_ignored,
        } => {
            let mut policy = match policy {
                Some(path) => {
                    toml::from_str::<Policy>(&fs::read_to_string(path).context("reading policy")?)
                        .context("invalid policy")?
                }
                None => Policy::default(),
            };
            policy.include_ignored |= include_ignored;
            let plan = create_plan(root, &out, policy)?;
            if json {
                output_json(&mut stdout, &plan)?;
            } else {
                writeln!(
                    stdout,
                    "{} candidates; {} source bytes saved; {} skipped",
                    plan.candidates.len(),
                    plan.savings_bytes(),
                    plan.skipped.len()
                )?;
                for item in &plan.candidates {
                    writeln!(
                        stdout,
                        "{}\t{} -> {} bytes (-{:.1}%)",
                        item.path.display(),
                        item.original_bytes,
                        item.optimized_bytes,
                        (item.original_bytes - item.optimized_bytes) as f64 * 100.0
                            / item.original_bytes as f64
                    )?;
                }
                for (path, reason) in &plan.skipped {
                    writeln!(stdout, "SKIP\t{}\t{reason}", path.display())?;
                }
                for diagnostic in &plan.diagnostics {
                    eprintln!("warning: {diagnostic}");
                }
                writeln!(
                    stdout,
                    "Review {}/plan.json; sources are unchanged. Savings do not measure app size.",
                    out.display()
                )?;
            }
        }
        Commands::Apply {
            directory,
            json,
            lossy,
            no_lossless,
            cross_format,
            accept_warnings,
            min_score,
            formats,
            dry_run,
        } => {
            if directory.join("analysis.json").is_file() {
                let policy = resopt::BatchPolicy {
                    lossless: !no_lossless,
                    lossy,
                    cross_format,
                    min_score,
                    formats,
                    accept_warnings,
                    resources: None,
                };
                if dry_run {
                    let plan = resopt::plan_report(&directory, &policy)?;
                    if json {
                        output_json(&mut stdout, &plan)?;
                    } else {
                        for item in &plan.items {
                            writeln!(
                                stdout,
                                "{}\t{}{}\t-{} bytes{}",
                                item.path.display(),
                                item.format,
                                item.quality.map_or(String::new(), |q| format!(" q{q}")),
                                item.savings_bytes,
                                item.warning
                                    .as_ref()
                                    .map_or(String::new(), |w| format!("\tWARNING {w}"))
                            )?;
                        }
                        writeln!(
                            stdout,
                            "Would apply {} files ({} lossy, {} with accepted warnings, {} format changes); {} source bytes saved. Nothing was changed.",
                            plan.items.len(),
                            plan.lossy_items,
                            plan.warning_items,
                            plan.cross_format_items,
                            plan.savings_bytes
                        )?;
                    }
                } else {
                    let status = resopt::apply_report(&directory, &policy)?;
                    report_batch(&mut stdout, &status, json, "Applied")?;
                }
            } else {
                let report = apply(directory)?;
                if json {
                    output_json(&mut stdout, &report)?;
                } else {
                    writeln!(
                        stdout,
                        "Applied {}; already current {}; saved {} source bytes",
                        report.changed, report.already_current, report.source_bytes_saved
                    )?;
                }
            }
        }
        Commands::Restore { directory, json } => {
            if directory.join("analysis.json").is_file() {
                let status = resopt::restore_report(&directory)?;
                report_batch(&mut stdout, &status, json, "Restored")?;
            } else {
                let report = restore(directory)?;
                if json {
                    output_json(&mut stdout, &report)?;
                } else {
                    writeln!(
                        stdout,
                        "Restored {}; already original {}",
                        report.changed, report.already_current
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Per-file outcomes; a batch with failures exits non-zero after reporting them.
fn report_batch(
    output: &mut impl Write,
    status: &resopt::BatchStatus,
    json: bool,
    verb: &str,
) -> Result<()> {
    if json {
        output_json(output, status)?;
    } else {
        for outcome in status.outcomes.iter().filter(|o| o.outcome != "applied") {
            writeln!(
                output,
                "{}\t{}\t{}",
                outcome.outcome.to_uppercase(),
                outcome.path.display(),
                outcome.error.as_deref().unwrap_or("")
            )?;
        }
        writeln!(
            output,
            "{verb} {}; failed {}; saved {} source bytes. Every applied file can be restored with `resopt restore <report>`.",
            status.applied, status.failed, status.savings_bytes
        )?;
    }
    anyhow::ensure!(
        status.failed == 0,
        "{} files could not be processed",
        status.failed
    );
    Ok(())
}

fn output_json(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut *output, value)?;
    writeln!(output)?;
    Ok(())
}
