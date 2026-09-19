// Batch apply and restore: explicit scope → preview → confirm → per-file outcomes.
// `undefined` means apply, `null` means restore all, and an array restores a selection.
let batchPlan = null, batchTimer = null, restoreResources;

function batchPolicy() {
  const warnings = [];
  if ($('batch-alpha').checked) warnings.push('alpha_error_exceeds_policy', 'transparency_presence_changed');
  if ($('batch-quality').checked) warnings.push('quality_below_policy');
  const floor = $('batch-min-score').value.trim();
  return {
    lossless: $('batch-lossless').checked, lossy: $('batch-lossy').checked, cross_format: $('batch-cross').checked,
    min_score: floor === '' ? null : Number(floor), formats: [], accept_warnings: warnings,
    resources: state.selectedRecords.size > 1 ? [...state.selectedRecords].map(indexOf) : $('batch-scope').checked ? state.filtered.map(indexOf) : null,
  };
}

function resetBatchPolicy() {
  $('batch-lossless').checked = true; $('batch-lossy').checked = false; $('batch-cross').checked = false;
  $('batch-alpha').checked = false; $('batch-quality').checked = false; $('batch-min-score').value = '';
}

function candidatePolicyReasons(resource, candidate, policy) {
  const reasons = [];
  if (candidate.lossy ? !policy.lossy : !policy.lossless) reasons.push(candidate.lossy ? 'batchExcludedLossy' : 'batchExcludedLossless');
  const crossing = candidate.format !== resource.resource?.format || resource.resource?.extension_mismatch;
  if (crossing && !policy.cross_format) reasons.push('batchExcludedCross');
  if (!candidate.valid && warningKind(candidate) && warningKinds(candidate).some(kind => !policy.accept_warnings.includes(kind))) reasons.push('batchExcludedWarning');
  const score = candidate.difference?.ssimulacra2;
  if (candidate.lossy && policy.min_score !== null && !(typeof score === 'number' && score >= policy.min_score)) reasons.push('batchExcludedScore');
  return [...new Set(reasons)];
}

function selectedBatchExclusions(plan) {
  const included = new Set(plan.items.map(item => item.resource));
  return (plan.policy.resources || []).filter(index => !included.has(index)).map(index => {
    const resource = state.records[index];
    if (!resource) return { index, path: String(index), reasons: ['batchExcludedUnavailable'] };
    if (isApplied(resource)) return { index, path: pathText(resource), reasons: ['batchExcludedApplied'] };
    if (resource.status !== 'candidates_available' || resource.resource?.conversion_exclusion) return { index, path: pathText(resource), reasons: ['batchExcludedUnavailable'] };
    const choices = (resource.candidates || []).filter(candidate => candidate.artifact && (candidate.valid || warningKind(candidate)))
      .map(candidate => candidatePolicyReasons(resource, candidate, plan.policy))
      .sort((a, b) => a.length - b.length);
    return { index, path: pathText(resource), reasons: choices[0]?.length ? choices[0] : ['batchExcludedUnavailable'] };
  });
}

function renderBatchExclusions(exclusions) {
  const title = $('batch-excluded-title'), list = $('batch-excluded-items'); list.replaceChildren();
  title.hidden = !exclusions.length; list.hidden = !exclusions.length;
  if (!exclusions.length) return;
  title.textContent = t('batchExcludedTitle', count(exclusions.length));
  for (const item of exclusions.slice(0, 200)) {
    const row = el('li'); row.append(el('span', 'path', item.path), el('span', 'status-muted', item.reasons.map(reason => t(reason)).join(` · `))); list.append(row);
  }
  if (exclusions.length > 200) list.append(el('li', 'status-muted', `+ ${count(exclusions.length - 200)}`));
}

function batchView(name) { for (const view of ['policy', 'plan', 'progress']) $(`batch-${view}-view`).hidden = view !== name; }

function syncBatchForm() {
  const lossy = $('batch-lossy').checked;
  for (const id of ['batch-alpha', 'batch-quality', 'batch-min-score']) { $(id).disabled = !lossy; if (!lossy && $(id).type === 'checkbox') $(id).checked = false; }
  const selected = state.selectedRecords.size > 1;
  $('batch-scope').disabled = selected;
  if (selected) { $('batch-scope').checked = true; $('batch-scope').dataset.selectionForced = 'true'; }
  else if ($('batch-scope').dataset.selectionForced) { $('batch-scope').checked = false; delete $('batch-scope').dataset.selectionForced; }
  $('batch-scope-label').textContent = selected ? t('batchSelectedScope', count(state.selectedRecords.size)) : t('batchScope', count(state.filtered.length));
  $('batch-preview').disabled = !$('batch-lossless').checked && !lossy;
  $('batch-error').textContent = '';
}

async function previewBatch() {
  $('batch-error').textContent = '';
  try {
    batchPlan = await api('/api/batch/preview', { policy: batchPolicy() });
    const selected = state.selectedRecords.size > 1 ? batchPlan.policy.resources?.length : null;
    if (!batchPlan.items.length && !selected) { $('batch-error').textContent = t('batchNothing'); return; }
    const exclusions = selected ? selectedBatchExclusions(batchPlan) : [];
    $('batch-summary').textContent = selected
      ? t('batchSelectedSummary', count(selected), count(batchPlan.items.length), count(exclusions.length), size(batchPlan.savings_bytes), count(batchPlan.lossy_items), count(batchPlan.warning_items), count(batchPlan.cross_format_items))
      : t('batchSummary', count(batchPlan.items.length), size(batchPlan.savings_bytes), count(batchPlan.lossy_items), count(batchPlan.warning_items), count(batchPlan.cross_format_items));
    const list = $('batch-items'); list.replaceChildren();
    for (const item of batchPlan.items.slice(0, 200)) {
      const row = el('li'); row.append(el('span', 'path', item.path), el('span', item.warning ? 'status-warn' : '', `${candidateLabel(item)}${item.warning ? ` · ${t(`warn_${item.warning}`)}` : ''}`), sizeNode(item.savings_bytes, 'number')); list.append(row);
    }
    if (batchPlan.items.length > 200) list.append(el('li', 'status-muted', `+ ${count(batchPlan.items.length - 200)}`));
    renderBatchExclusions(exclusions);
    $('batch-confirm').textContent = t('batchConfirm', count(batchPlan.items.length));
    $('batch-confirm').disabled = !batchPlan.items.length;
    $('batch-confirm').classList.toggle('danger', batchPlan.warning_items > 0);
    batchView('plan'); $('batch-back').focus();
  } catch (error) { $('batch-error').textContent = error.message; }
}

function renderOutcomes(status, restoring) {
  const skipped = status.outcomes.filter(o => o.outcome === 'cancelled').length;
  $('batch-progress-text').textContent = status.running ? t(restoring ? 'restoreRunning' : 'batchRunning', count(status.outcomes.length), count(status.total))
    : restoring ? t('restoreAllDone', count(status.applied), count(status.failed)) : t('batchDone', count(status.applied), count(status.failed), count(skipped), size(status.savings_bytes));
  $('batch-bar').max = status.total || 1; $('batch-bar').value = status.outcomes.length;
  const failures = status.outcomes.filter(o => o.outcome === 'failed'), list = $('batch-failures'); list.replaceChildren();
  $('batch-failures-title').textContent = t(restoring ? 'restoreFailures' : 'batchFailures'); $('batch-failures-title').hidden = !failures.length;
  for (const failure of failures) { const row = el('li'); row.append(el('span', 'path', failure.path), el('span', 'status-warn', failure.error || '')); list.append(row); }
  $('batch-stopped').textContent = t(restoring ? 'restoreStopped' : 'batchStopped'); $('batch-stopped').hidden = status.running || !status.cancelled;
  $('batch-stop').hidden = !status.running; $('batch-done').hidden = status.running;
}

async function watchBatch(restoring) {
  clearTimeout(batchTimer);
  try {
    const status = await api('/api/batch');
    if (status.running) { renderOutcomes(status, restoring); batchTimer = setTimeout(() => watchBatch(restoring), 500); return; }
    state.operations = await api('/api/state'); refresh(false); renderDetail();
    // Reveal Done only after the operation state and counters are refreshed.
    renderOutcomes(status, restoring);
  } catch (error) { $('batch-progress-text').textContent = error.message; batchTimer = setTimeout(() => watchBatch(restoring), 2000); }
}

async function startBatch(route, body, restoring) {
  batchView('progress'); $('batch-stop').disabled = false;
  renderOutcomes({ running: true, total: 0, outcomes: [], applied: 0, failed: 0, savings_bytes: 0 }, restoring);
  try { await api(route, body); watchBatch(restoring); }
  catch (error) { $('batch-progress-text').textContent = error.message; $('batch-stop').hidden = true; $('batch-done').hidden = false; }
}

function setupBatch() {
  for (const id of ['batch-lossless', 'batch-lossy', 'batch-cross', 'batch-alpha', 'batch-quality', 'batch-scope']) $(id).addEventListener('change', syncBatchForm);
  $('batch-open').addEventListener('click', () => { restoreResources = undefined; resetBatchPolicy(); $('batch-title').textContent = t('batchTitle'); batchView('policy'); syncBatchForm(); $('batch-dialog').showModal(); $('batch-lossless').focus(); });
  $('batch-preview').addEventListener('click', previewBatch);
  $('batch-back').addEventListener('click', () => batchView('policy'));
  // One handler decides by mode, so a restore can never also start an apply.
  $('batch-confirm').addEventListener('click', () => {
    if (restoreResources === undefined) startBatch('/api/batch/apply', { policy: batchPlan.policy, token: batchPlan.token }, false);
    else if (restoreResources === null) startBatch('/api/restore-all', {}, true);
    else startBatch('/api/batch/restore', { resources: restoreResources }, true);
  });
  $('batch-stop').addEventListener('click', async () => { $('batch-stop').disabled = true; try { await api('/api/batch/cancel', {}); } catch { /* shown by the watcher */ } });
  for (const id of ['batch-close', 'batch-done']) $(id).addEventListener('click', () => $('batch-dialog').close());
  $('batch-dialog').addEventListener('cancel', event => { if (!$('batch-stop').hidden) event.preventDefault(); });
  const openRestore = resources => {
    const all = resources === null;
    const pending = all ? Object.values(state.operations).filter(s => s.state !== 'original').length : resources.length;
    $('batch-title').textContent = t(all ? 'restoreAllTitle' : 'restoreSelectedTitle');
    $('batch-summary').textContent = all ? t('restoreAllIntro') : t('restoreSelectedIntro', count(pending));
    const list = $('batch-items'); list.replaceChildren();
    renderBatchExclusions([]);
    if (!all) for (const index of resources) list.append(el('li', 'path', pathText(state.records[index])));
    $('batch-confirm').textContent = t(all ? 'restoreAllConfirm' : 'restoreSelectedConfirm', count(pending)); $('batch-confirm').disabled = false; $('batch-confirm').classList.remove('danger');
    batchView('plan'); $('batch-dialog').showModal(); $('batch-back').hidden = true; $('batch-confirm').focus();
    restoreResources = resources;
  };
  $('restore-selected-open').addEventListener('click', () => openRestore(selectedAppliedResources()));
  $('restore-all-open').addEventListener('click', () => openRestore(null));
  // `close` is dispatched asynchronously; ignore it if the dialog was reopened meanwhile.
  $('batch-dialog').addEventListener('close', () => { if ($('batch-dialog').open) return; $('batch-back').hidden = false; restoreResources = undefined; clearTimeout(batchTimer); });
}
