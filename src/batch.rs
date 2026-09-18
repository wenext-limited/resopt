//! Policy-driven application of many reviewed candidates.
//!
//! Every file is applied as its own journaled transaction, so a batch can be
//! cancelled or interrupted at any point: finished files stay individually
//! restorable and unfinished ones are untouched.
use crate::{
    ImageCandidate, ResourceAnalysis,
    analysis::WARNINGS,
    filesystem::hash,
    review::{Approvals, Review},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// What a batch may do. The defaults apply only verified lossless,
/// same-format candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatchPolicy {
    pub lossless: bool,
    pub lossy: bool,
    /// Allow conversions that change the file format (and migrate references).
    pub cross_format: bool,
    /// Extra floor for lossy candidates, on top of the analysis threshold.
    pub min_score: Option<f64>,
    /// Target formats to allow; empty allows every format.
    pub formats: Vec<String>,
    /// Warning kinds accepted for the whole batch. Empty skips warning candidates.
    pub accept_warnings: Vec<String>,
    /// Restrict the batch to these resource indexes (e.g. the filtered view).
    pub resources: Option<Vec<usize>>,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            lossless: true,
            lossy: false,
            cross_format: false,
            min_score: None,
            formats: vec![],
            accept_warnings: vec![],
            resources: None,
        }
    }
}

impl BatchPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lossless || self.lossy,
            "batch policy selects neither lossless nor lossy candidates"
        );
        ensure!(
            self.min_score
                .is_none_or(|s| s.is_finite() && (0.0..=100.0).contains(&s)),
            "min_score must be 0..=100"
        );
        for warning in &self.accept_warnings {
            ensure!(
                WARNINGS.contains(&warning.as_str()),
                "unknown warning kind: {warning}"
            );
        }
        ensure!(
            self.accept_warnings.is_empty() || self.lossy,
            "warning candidates are lossy; enable lossy candidates to accept warnings"
        );
        Ok(())
    }

    fn approvals(&self) -> Approvals {
        Approvals {
            lossy: self.lossy,
            warnings: self.accept_warnings.clone(),
        }
    }

    fn allows(&self, resource: &ResourceAnalysis, candidate: &ImageCandidate) -> bool {
        let accepted_warning = candidate.is_warning()
            && candidate
                .required_warnings()
                .iter()
                .all(|w| self.accept_warnings.contains(w));
        let crossing =
            candidate.format != resource.resource.format || resource.resource.extension_mismatch;
        candidate.artifact.is_some()
            && (candidate.valid || accepted_warning)
            && (if candidate.lossy {
                self.lossy
            } else {
                self.lossless
            })
            && (self.cross_format || !crossing)
            && (!crossing || resource.resource.format_lock.is_none())
            && (self.formats.is_empty() || self.formats.contains(&candidate.format))
            && (!candidate.lossy
                || self.min_score.is_none_or(|floor| {
                    candidate
                        .difference
                        .as_ref()
                        .and_then(|d| d.ssimulacra2)
                        .is_some_and(|score| score >= floor)
                }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchItem {
    pub resource: usize,
    pub candidate: usize,
    pub path: PathBuf,
    pub format: String,
    pub quality: Option<u8>,
    pub lossy: bool,
    pub warning: Option<String>,
    pub original_bytes: u64,
    pub savings_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchPlan {
    pub policy: BatchPolicy,
    pub items: Vec<BatchItem>,
    pub savings_bytes: u64,
    pub lossy_items: usize,
    pub warning_items: usize,
    pub cross_format_items: usize,
    /// Must accompany the apply request so the confirmed plan is the one run.
    pub token: String,
}

/// Choose, per resource, the smallest candidate the policy allows. Resources
/// with an existing operation are left alone.
pub(crate) fn plan(review: &Review, policy: &BatchPolicy) -> Result<BatchPlan> {
    policy.validate()?;
    let states = review.states();
    let mut items = Vec::new();
    for (index, resource) in review.report.resources.iter().enumerate() {
        if policy
            .resources
            .as_ref()
            .is_some_and(|r| !r.contains(&index))
            || resource.status != "candidates_available"
            || resource.resource.conversion_exclusion.is_some()
            || states
                .get(index.to_string())
                .is_some_and(|s| s["state"] != "original")
        {
            continue;
        }
        let Some((candidate_index, candidate)) = resource
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| policy.allows(resource, c))
            .min_by_key(|(_, c)| c.bytes)
        else {
            continue;
        };
        items.push(BatchItem {
            resource: index,
            candidate: candidate_index,
            path: resource.resource.path.clone(),
            format: candidate.format.clone(),
            quality: candidate.quality,
            lossy: candidate.lossy,
            warning: candidate
                .rejection
                .clone()
                .filter(|_| candidate.is_warning()),
            original_bytes: resource.resource.bytes,
            savings_bytes: candidate.savings_bytes,
        });
    }
    // Largest savings first: a cancelled batch has already done the most useful work.
    items.sort_by(|a, b| {
        b.savings_bytes
            .cmp(&a.savings_bytes)
            .then(a.resource.cmp(&b.resource))
    });
    let selection: Vec<_> = items.iter().map(|i| (i.resource, i.candidate)).collect();
    let token = hash(&serde_json::to_vec(&(policy, &selection))?);
    Ok(BatchPlan {
        policy: policy.clone(),
        savings_bytes: items.iter().map(|i| i.savings_bytes).sum(),
        lossy_items: items.iter().filter(|i| i.lossy).count(),
        warning_items: items.iter().filter(|i| i.warning.is_some()).count(),
        cross_format_items: items
            .iter()
            .filter(|i| {
                let r = &review.report.resources[i.resource].resource;
                i.format != r.format || r.extension_mismatch
            })
            .count(),
        items,
        token,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchOutcome {
    pub resource: usize,
    pub path: PathBuf,
    /// `applied`, `failed` or `cancelled`.
    pub outcome: &'static str,
    pub error: Option<String>,
    pub savings_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BatchStatus {
    pub running: bool,
    pub cancelled: bool,
    pub total: usize,
    pub outcomes: Vec<BatchOutcome>,
    pub applied: usize,
    pub failed: usize,
    pub savings_bytes: u64,
}

/// Run a confirmed plan. `status` is updated after every file so callers can
/// stream per-file outcomes; `cancel` stops before the next file.
pub(crate) fn run(
    review: &Review,
    plan: &BatchPlan,
    status: &Mutex<BatchStatus>,
    cancel: &AtomicBool,
) {
    {
        let mut status = status.lock().unwrap_or_else(|e| e.into_inner());
        *status = BatchStatus {
            running: true,
            total: plan.items.len(),
            ..Default::default()
        };
    }
    let approvals = plan.policy.approvals();
    for item in &plan.items {
        let (outcome, error) = if cancel.load(Ordering::SeqCst) {
            ("cancelled", None)
        } else {
            match review.apply_with_warnings(item.resource, item.candidate, &approvals, None, false)
            {
                Ok(()) => ("applied", None),
                Err(error) => ("failed", Some(format!("{error:#}"))),
            }
        };
        let mut status = status.lock().unwrap_or_else(|e| e.into_inner());
        match outcome {
            "applied" => {
                status.applied += 1;
                status.savings_bytes += item.savings_bytes;
            }
            "failed" => status.failed += 1,
            _ => status.cancelled = true,
        }
        status.outcomes.push(BatchOutcome {
            resource: item.resource,
            path: item.path.clone(),
            outcome,
            error,
            savings_bytes: if outcome == "applied" {
                item.savings_bytes
            } else {
                0
            },
        });
    }
    status.lock().unwrap_or_else(|e| e.into_inner()).running = false;
}

/// Restore every applied or partially applied operation. Operations sharing a
/// file (one Contents.json, one reference file) must be undone newest-first;
/// repeated passes find that order without trusting recorded timestamps.
pub(crate) fn restore_all(review: &Review, cancel: &AtomicBool) -> BatchStatus {
    let mut pending: Vec<usize> = review
        .states()
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, state)| state["state"] != "original")
        .filter_map(|(index, _)| index.parse().ok())
        .collect();
    pending.sort_unstable();
    let mut status = BatchStatus {
        total: pending.len(),
        ..Default::default()
    };
    let mut last_errors = std::collections::BTreeMap::new();
    loop {
        let before = pending.len();
        pending.retain(|&index| {
            if cancel.load(Ordering::SeqCst) {
                return true;
            }
            match review.restore(index) {
                Ok(()) => {
                    status.applied += 1;
                    status.outcomes.push(BatchOutcome {
                        resource: index,
                        path: review.report.resources[index].resource.path.clone(),
                        outcome: "applied",
                        error: None,
                        savings_bytes: 0,
                    });
                    false
                }
                Err(error) => {
                    last_errors.insert(index, format!("{error:#}"));
                    true
                }
            }
        });
        if pending.is_empty() || pending.len() == before || cancel.load(Ordering::SeqCst) {
            break;
        }
    }
    status.cancelled = cancel.load(Ordering::SeqCst);
    for index in pending {
        status.failed += 1;
        status.outcomes.push(BatchOutcome {
            resource: index,
            path: review.report.resources[index].resource.path.clone(),
            outcome: if status.cancelled {
                "cancelled"
            } else {
                "failed"
            },
            error: last_errors.remove(&index),
            savings_bytes: 0,
        });
    }
    status
}

/// Plan a batch for the report in `directory` without changing anything.
pub fn plan_report(
    directory: impl AsRef<std::path::Path>,
    policy: &BatchPolicy,
) -> Result<BatchPlan> {
    plan(&Review::open(directory.as_ref())?, policy)
}

/// Apply every candidate the policy allows and return per-file outcomes.
/// Interrupting the process is safe: each file is a separate journaled
/// operation that `restore_report` (or the web UI) can undo.
pub fn apply_report(
    directory: impl AsRef<std::path::Path>,
    policy: &BatchPolicy,
) -> Result<BatchStatus> {
    let review = Review::open(directory.as_ref())?;
    let plan = plan(&review, policy)?;
    let status = Mutex::new(BatchStatus::default());
    run(&review, &plan, &status, &AtomicBool::new(false));
    Ok(status.into_inner().unwrap_or_else(|e| e.into_inner()))
}

/// Restore every operation recorded in the report directory.
pub fn restore_report(directory: impl AsRef<std::path::Path>) -> Result<BatchStatus> {
    let review = Review::open(directory.as_ref())?;
    Ok(restore_all(&review, &AtomicBool::new(false)))
}
