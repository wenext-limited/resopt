// Batch apply and restore-all: explicit policy → preview → confirm → per-file outcomes.
let batchPlan = null, batchTimer = null, restoringAll = false;

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
    if (!batchPlan.items.length) { $('batch-error').textContent = t('batchNothing'); return; }
    $('batch-summary').textContent = t('batchSummary', count(batchPlan.items.length), size(batchPlan.savings_bytes), count(batchPlan.lossy_items), count(batchPlan.warning_items), count(batchPlan.cross_format_items));
    const list = $('batch-items'); list.replaceChildren();
    for (const item of batchPlan.items.slice(0, 200)) {
      const row = el('li'); row.append(el('span', 'path', item.path), el('span', item.warning ? 'status-warn' : '', `${candidateLabel(item)}${item.warning ? ` · ${t(`warn_${item.warning}`)}` : ''}`), sizeNode(item.savings_bytes, 'number')); list.append(row);
    }
    if (batchPlan.items.length > 200) list.append(el('li', 'status-muted', `+ ${count(batchPlan.items.length - 200)}`));
    $('batch-confirm').textContent = t('batchConfirm', count(batchPlan.items.length));
    $('batch-confirm').classList.toggle('danger', batchPlan.warning_items > 0);
    batchView('plan'); $('batch-back').focus();
  } catch (error) { $('batch-error').textContent = error.message; }
}

function renderOutcomes(status, restoring) {
  const skipped = status.outcomes.filter(o => o.outcome === 'cancelled').length;
  $('batch-progress-text').textContent = status.running ? t('batchRunning', count(status.outcomes.length), count(status.total))
    : restoring ? t('restoreAllDone', count(status.applied), count(status.failed)) : t('batchDone', count(status.applied), count(status.failed), count(skipped), size(status.savings_bytes));
  $('batch-bar').max = status.total || 1; $('batch-bar').value = status.outcomes.length;
  const failures = status.outcomes.filter(o => o.outcome === 'failed'), list = $('batch-failures'); list.replaceChildren();
  $('batch-failures-title').hidden = !failures.length;
  for (const failure of failures) { const row = el('li'); row.append(el('span', 'path', failure.path), el('span', 'status-warn', failure.error || '')); list.append(row); }
  $('batch-stopped').hidden = status.running || !status.cancelled;
  $('batch-stop').hidden = !status.running; $('batch-done').hidden = status.running;
}

async function watchBatch(restoring) {
  clearTimeout(batchTimer);
  try {
    const status = await api('/api/batch');
    renderOutcomes(status, restoring);
    if (status.running) { batchTimer = setTimeout(() => watchBatch(restoring), 500); return; }
    state.operations = await api('/api/state'); refresh(false); renderDetail();
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
  $('batch-open').addEventListener('click', () => { restoringAll = false; $('batch-title').textContent = t('batchTitle'); batchView('policy'); syncBatchForm(); $('batch-dialog').showModal(); $('batch-lossless').focus(); });
  $('batch-preview').addEventListener('click', previewBatch);
  $('batch-back').addEventListener('click', () => batchView('policy'));
  // One handler decides by mode, so a restore can never also start an apply.
  $('batch-confirm').addEventListener('click', () => (restoringAll ? startBatch('/api/restore-all', {}, true) : startBatch('/api/batch/apply', { policy: batchPlan.policy, token: batchPlan.token }, false)));
  $('batch-stop').addEventListener('click', async () => { $('batch-stop').disabled = true; try { await api('/api/batch/cancel', {}); } catch { /* shown by the watcher */ } });
  for (const id of ['batch-close', 'batch-done']) $(id).addEventListener('click', () => $('batch-dialog').close());
  $('batch-dialog').addEventListener('cancel', event => { if (!$('batch-stop').hidden) event.preventDefault(); });
  $('restore-all-open').addEventListener('click', () => {
    const pending = Object.values(state.operations).filter(s => s.state !== 'original').length;
    $('batch-title').textContent = t('restoreAllTitle'); $('batch-summary').textContent = t('restoreAllIntro'); $('batch-items').replaceChildren();
    $('batch-confirm').textContent = t('restoreAllConfirm', count(pending)); $('batch-confirm').classList.remove('danger');
    batchView('plan'); $('batch-dialog').showModal(); restoringAll = true; $('batch-back').hidden = true; $('batch-confirm').focus();
  });
  // `close` is dispatched asynchronously; ignore it if the dialog was reopened meanwhile.
  $('batch-dialog').addEventListener('close', () => { if ($('batch-dialog').open) return; $('batch-back').hidden = false; restoringAll = false; clearTimeout(batchTimer); });
}
