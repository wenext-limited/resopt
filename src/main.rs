use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use resopt::{AnalysisOptions, Policy, analyze_with_progress, apply, create_plan, restore};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::{Command, ExitCode},
};

#[derive(Parser)]
#[command(
    name = "resopt",
    version,
    about = "Reviewable resource optimization for Apple projects"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        #[arg(long, value_delimiter = ',', default_value = "75,85,95")]
        qualities: Vec<u8>,
        #[arg(long, default_value_t = 2)]
        jobs: usize,
        #[arg(long, default_value_t = resopt::DEFAULT_MAX_PIXELS)]
        max_pixels: usize,
        #[arg(long, default_value_t = Policy::default().png_level)]
        png_level: u8,
        #[arg(long)]
        png_reductions: bool,
        /// Also compare WebP candidates for loose resources (additional encoding time).
        #[arg(long)]
        webp: bool,
        #[arg(long)]
        include_ignored: bool,
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
        #[arg(long, value_delimiter = ',', default_value = "75,85,95")]
        qualities: Vec<u8>,
        #[arg(long, default_value_t = 2)]
        jobs: usize,
        #[arg(long, default_value_t = 0)]
        min_input_bytes: u64,
        #[arg(long)]
        probe_only: bool,
        /// Maximum per-pixel alpha error (0 is exact).
        #[arg(long, default_value_t = 1.0 / 255.0 + 0.000001)]
        max_alpha_error: f32,
        /// Largest decoded image to analyze, in pixels (16 bytes each per decode).
        #[arg(long, default_value_t = resopt::DEFAULT_MAX_PIXELS)]
        max_pixels: usize,
        /// oxipng effort (0..=6) for the lossless PNG candidate.
        #[arg(long, default_value_t = Policy::default().png_level)]
        png_level: u8,
        /// Allow lossless PNG color-type, bit-depth and palette reductions.
        #[arg(long)]
        png_reductions: bool,
        /// Also compare WebP candidates for loose resources (additional encoding time).
        #[arg(long)]
        webp: bool,
        #[arg(long)]
        json: bool,
        /// Include files matched by Git ignore rules.
        #[arg(long)]
        include_ignored: bool,
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
    /// Apply the exact candidates in a reviewed plan directory.
    Apply {
        directory: PathBuf,
        #[arg(long)]
        json: bool,
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
            qualities,
            jobs,
            max_pixels,
            png_level,
            png_reductions,
            webp,
            include_ignored,
        } => {
            resopt::web(
                root,
                resopt::WebOptions {
                    out,
                    port,
                    no_open,
                    analysis: AnalysisOptions {
                        qualities,
                        jobs,
                        max_pixels,
                        png_level,
                        png_reductions,
                        webp,
                        include_ignored,
                        ..Default::default()
                    },
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
            let report = serde_json::json!({
                "schema_version": 1,
                "resopt": env!("CARGO_PKG_VERSION"),
                "backends": [
                    {"name":"oxipng", "version":"10.2.1", "status":"embedded", "mode":"strict_lossless_png"},
                    {"name":"libwebp", "status":"embedded", "mode":"opt_in_webp_candidates"},
                    {"name":"Apple ImageIO", "available":resopt::image_backend_available(), "mode":"image_analysis_jpeg_heic"}
                ],
                "optional_tools": [
                    {"name":"sips", "available":std::path::Path::new("/usr/bin/sips").is_file(), "backend_implemented":false},
                    {"name":"ffmpeg", "available":available("ffmpeg"), "backend_implemented":false},
                    {"name":"ffprobe", "available":available("ffprobe"), "backend_implemented":false}
                ]
            });
            if json {
                output_json(&mut stdout, &report)?;
            } else {
                writeln!(stdout, "resopt {}", env!("CARGO_PKG_VERSION"))?;
                writeln!(
                    stdout,
                    "PNG: embedded Oxipng 10.2.1; no separate executable required"
                )?;
                writeln!(
                    stdout,
                    "JPEG/HEIC analysis: {}",
                    if resopt::image_backend_available() {
                        "Apple ImageIO (native; no sips/ffmpeg install required)"
                    } else {
                        "requires macOS; scan and lossless PNG plan remain available"
                    }
                )?;
                for tool in report["optional_tools"]
                    .as_array()
                    .context("invalid doctor report")?
                {
                    writeln!(
                        stdout,
                        "{}: {} (future backend; not required)",
                        tool["name"].as_str().unwrap_or("unknown"),
                        if tool["available"] == true {
                            "available"
                        } else {
                            "not found"
                        }
                    )?;
                }
            }
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
            qualities,
            jobs,
            min_input_bytes,
            probe_only,
            include_ignored,
            max_alpha_error,
            max_pixels,
            png_level,
            png_reductions,
            webp,
            json,
        } => {
            let options = AnalysisOptions {
                qualities,
                jobs,
                min_input_bytes,
                probe_only,
                include_ignored,
                max_alpha_error,
                max_pixels,
                png_level,
                png_reductions,
                webp,
                ..AnalysisOptions::default()
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
        Commands::Apply { directory, json } => {
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
        Commands::Restore { directory, json } => {
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
    Ok(())
}

fn available(program: &str) -> bool {
    Command::new(program)
        .arg("-version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn output_json(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(&mut *output, value)?;
    writeln!(output)?;
    Ok(())
}
