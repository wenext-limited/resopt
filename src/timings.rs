//! Per-phase analysis timers. Values are summed across workers, so they show
//! where CPU time goes rather than wall-clock time.
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Scan,
    Hash,
    Decode,
    EncodeLossless,
    EncodeJpeg,
    EncodeHeic,
    EncodeWebp,
    EncodeLossyPng,
    Score,
    Preview,
    Write,
    Cache,
    Localization,
    Report,
}

const PHASES: [(Phase, &str); 14] = [
    (Phase::Scan, "scan"),
    (Phase::Hash, "hash"),
    (Phase::Decode, "decode"),
    (Phase::EncodeLossless, "encode_lossless"),
    (Phase::EncodeJpeg, "encode_lossy_jpeg"),
    (Phase::EncodeHeic, "encode_lossy_heic"),
    (Phase::EncodeWebp, "encode_lossy_webp"),
    (Phase::EncodeLossyPng, "encode_lossy_png"),
    (Phase::Score, "score"),
    (Phase::Preview, "preview"),
    (Phase::Write, "write"),
    (Phase::Cache, "cache"),
    (Phase::Localization, "localization"),
    (Phase::Report, "report"),
];

#[derive(Default)]
pub(crate) struct Timings {
    nanos: [AtomicU64; PHASES.len()],
}

impl Timings {
    pub fn time<T>(&self, phase: Phase, work: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let value = work();
        let index = PHASES.iter().position(|(p, _)| *p == phase).unwrap_or(0);
        self.nanos[index].fetch_add(
            u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        value
    }

    /// Seconds per phase, omitting phases that never ran.
    pub fn snapshot(&self) -> BTreeMap<String, f64> {
        PHASES
            .iter()
            .enumerate()
            .map(|(index, (_, name))| {
                (
                    (*name).to_string(),
                    self.nanos[index].load(Ordering::Relaxed) as f64 / 1e9,
                )
            })
            .filter(|(_, seconds)| *seconds > 0.0)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_accumulate_independently() {
        let timings = Timings::default();
        assert_eq!(
            timings.time(Phase::Decode, || {
                std::thread::sleep(std::time::Duration::from_millis(2));
                7
            }),
            7
        );
        timings.time(Phase::Decode, || {
            std::thread::sleep(std::time::Duration::from_millis(2))
        });
        let snapshot = timings.snapshot();
        assert!(snapshot["decode"] >= 0.004);
        assert!(!snapshot.contains_key("score"));
    }
}
