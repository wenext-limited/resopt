// Inspector: previews, candidate table, single-file apply and restore.
let pendingAction = null;

function scoreLabel(score) { return t(score >= 90 ? 'score90' : score >= 70 ? 'score70' : score >= 50 ? 'score50' : 'score0'); }

function drawPreview(r, c, original) {
  const figure = el('figure', 'preview-frame'), caption = el('div', 'preview-label');
  caption.append(el('span', '', original ? t('original') : c ? candidateLabel(c) : t('candidate')));
  const raw = assetUrl(original ? r.original_artifact : c?.artifact);
  if (raw) { const link = el('a', '', t('openFull')); link.href = raw; link.target = '_blank'; link.rel = 'noopener'; caption.append(link); }
  const canvas = el('div', 'canvas'); canvas.dataset.background = state.background;
  const src = assetUrl(original ? r.original_preview : c?.preview);
  if (src) { const img = el('img'); img.src = src; img.decoding = 'async'; img.alt = `${basename(pathText(r))} — ${original ? t('original') : candidateLabel(c)}`; canvas.append(img); }
  else canvas.append(el('div', 'placeholder', c?.rejection ? issueText(locale, c.rejection) : t('statusNoGain')));
  const caption2 = el('figcaption', 'preview-size'); caption2.append(sizeNode(original ? r.resource?.bytes : c?.bytes ?? null));
  if (!original && c?.savings_bytes > 0) caption2.append(el('small', '', `−${size(c.savings_bytes)} · ${formatPercent(c.savings_bytes / (r.resource.bytes || 1))}`));
  figure.append(caption, canvas, caption2); return figure;
}

function candidateStatus(c) {
  if (warningKind(c)) return { cls: 'status-warn', text: issueText(locale, c.rejection) };
  if (c.rejection) return { cls: 'status-muted', text: issueText(locale, c.rejection) };
  if (!c.artifact) return { cls: 'status-muted', text: t('statusNoGain') };
  return { cls: 'status-ok', text: t('statusReview') };
}

function renderDetail() {
  const pane = $('inspector'); pane.replaceChildren();
  const r = state.selected;
  if (!r) { const empty = el('div', 'empty'); empty.append(el('strong', '', t('select')), el('span', '', t('selectHint'))); pane.append(empty); return; }
  renderMultiSelection(pane);
  const variants = Array.isArray(r.candidates) ? r.candidates : [], c = variants[state.chosen] || null;
  const heading = el('div', 'detail-heading'), title = el('div');
  title.append(el('div', 'eyebrow', t(`kind_${r.resource?.kind}`)), el('h1', '', basename(pathText(r))));
  heading.append(title, el('span', `tag ${isOptimized(r) ? 'optimized-status' : r.status === 'failed' ? 'danger' : ''}`, isOptimized(r) ? `✓ ${t('modeApplied')}` : t(`status_${r.status}`)));
  pane.append(heading, el('p', 'detail-path', pathText(r)));
  const facts = el('div', 'facts'); facts.append(el('span', '', String(r.resource?.format || '').toUpperCase()), sizeNode(r.resource?.bytes));
  if (r.image) {
    facts.append(el('span', '', `${count(r.image.width)} × ${count(r.image.height)}`), el('span', '', t(r.image.has_transparent_pixels ? 'transparent' : 'opaque')));
    if (r.image.frames > 1) facts.append(el('span', '', t('frames', r.image.frames)));
  }
  if (r.media) facts.append(el('span', '', t('mediaInfo', r.media.streams.join(' + '), r.media.duration_seconds?.toFixed(1) ?? '—', r.media.bit_rate ? Math.round(r.media.bit_rate / 1000) : '—')));
  if (r.resource?.extension_mismatch) facts.append(el('span', 'status-warn', t('mismatch')));
  pane.append(facts);
  const android = r.resource?.android;
  if (android) pane.append(el('p', 'android-note', `${t('android')}: ${t('androidInfo', android.area, android.res_type || '—', android.name || '—', android.qualifiers?.length ? android.qualifiers.join('-') : '—')}`));
  if (variants.length) {
    const controls = el('div', 'comparison-controls'), label = el('label', 'candidate-label', t('compareWith')), select = el('select');
    select.id = 'candidate-select'; label.htmlFor = select.id;
    variants.forEach((v, i) => { const o = el('option', '', `${candidateLabel(v)} · ${size(v.bytes || null)}${warningKind(v) ? ` · ${t('notValid')}` : !v.valid && v.rejection ? ` · ${t('failedCheck')}` : ''}`); o.value = String(i); select.append(o); });
    if (c) select.value = String(state.chosen);
    select.addEventListener('change', () => { state.chosen = Number(select.value); renderDetail(); $('candidate-select')?.focus(); });
    label.append(select); controls.append(label);
    const swatches = el('div', 'backgrounds'); swatches.setAttribute('role', 'group'); swatches.setAttribute('aria-label', t('background'));
    for (const [value, name] of [['checker', 'bgChecker'], ['light', 'bgLight'], ['dark', 'bgDark']]) {
      const b = el('button', 'swatch'); b.type = 'button'; b.dataset.background = value; b.title = t(name); b.setAttribute('aria-label', t(name)); b.setAttribute('aria-pressed', String(state.background === value));
      b.addEventListener('click', () => { state.background = value; document.querySelectorAll('.canvas,.compare-stage').forEach(n => { n.dataset.background = value; }); swatches.querySelectorAll('button').forEach(n => n.setAttribute('aria-pressed', String(n.dataset.background === value))); });
      swatches.append(b);
    }
    controls.append(swatches); pane.append(controls);
    const comparison = el('div', 'comparison'); comparison.append(drawPreview(r, c, true), drawPreview(r, c, false)); pane.append(comparison);
    if (c?.artifact && r.original_artifact) { const open = el('button', 'link-button', t('compare')); open.type = 'button'; open.addEventListener('click', () => openCompare(r, c)); pane.append(open); }
    pane.append(el('p', 'preview-note', t('previewNote')));
    if (c && warningKind(c)) pane.append(warningBlock(c));
    if (c?.notes?.length) pane.append(el('div', 'reasons', `${t('notes')}: ${c.notes.map(n => issueText(locale, n)).join('; ')}`));
    renderActions(pane, r, c);
    pane.append(candidateTable(r, variants));
  }
  const group = (state.meta?.similarGroups || [])[duplicateGroups().get(indexOf(r))];
  if (group) pane.append(duplicateBlock(r, group));
  if (r.issues?.length) pane.append(el('div', 'reasons', r.issues.map(i => issueText(locale, i)).join('; ')));
  const notes = el('details'), options = state.meta?.options || {};
  notes.append(el('summary', '', t('methodology')), el('p', '', t('methodology1', `${((options.max_alpha_error || 0) * 100).toFixed(3)}%`, options.min_score ?? '—')), el('p', '', t('methodology2')));
  pane.append(notes);
  if (!reducedMotion()) pane.animate([{ opacity: .7, transform: 'translateY(2px)' }, { opacity: 1, transform: 'none' }], { duration: 140, easing: 'ease-out' });
  pane.scrollTop = 0;
}

function renderMultiSelection(pane) {
  if (state.selectedRecords.size < 2) return;
  const items = state.filtered.filter(r => state.selectedRecords.has(r));
  const block = el('section', 'multi-selection');
  block.append(el('strong', '', t('multiSelected', count(items.length))), el('p', '', t('multiSelectedHint')));
  pane.append(block);
}

function duplicateBlock(r, group) {
  const block = el('section', 'duplicate-block'); block.setAttribute('aria-label', t(`dup_${group.kind}`));
  block.append(el('strong', '', t(`dup_${group.kind}`)), el('span', 'hint', t('dupRedundant', count(group.members.length), size(group.redundant_bytes))));
  const list = el('ul', 'batch-list');
  for (const index of group.members) {
    const member = state.records[index]; if (!member) continue;
    const item = el('li'), open = el('button', 'link-button path', pathText(member)); open.type = 'button'; open.disabled = member === r;
    open.addEventListener('click', () => { if (!state.filtered.includes(member)) { state.mode = 'duplicates'; refresh(true); } state.page = Math.floor(state.filtered.indexOf(member) / PAGE_SIZE); renderList(); selectRecord(member); });
    item.append(open, el('span', '', member.image ? `${count(member.image.width)} × ${count(member.image.height)}` : ''), sizeNode(member.resource?.bytes, 'number')); list.append(item);
  }
  block.append(list, el('p', 'hint', t('dupHint'))); return block;
}

function warningDetail(c, kind) {
  return kind === 'quality_below_policy' ? t(`warnDetail_${kind}`, Number(c.difference?.ssimulacra2 ?? 0).toFixed(1), state.meta?.options?.min_score ?? '—')
    : t(`warnDetail_${kind}`, `${((c.difference?.max_alpha_error || 0) * 100).toFixed(2)}%`);
}
function warningBlock(c) {
  const block = el('div', 'warning-block'); block.setAttribute('role', 'note');
  for (const kind of warningKinds(c)) block.append(el('strong', '', t(`warn_${kind}`)), el('span', '', warningDetail(c, kind)));
  return block;
}

function candidateTable(r, variants) {
  const wrap = el('div'), heading = el('div', 'metrics-title', t('allCandidates')); heading.append(el('span', '', t('nCandidates', variants.length)));
  const scroll = el('div', 'table-scroll'), table = el('table'), head = el('tr');
  for (const [key, hint] of [['thCandidate'], ['thSize'], ['thSaved'], ['thScore', 'thScoreHint'], ['thRgb', 'thRgbHint'], ['thAlpha', 'thAlphaHint'], ['thStatus']]) { const th = el('th', '', t(key)); th.scope = 'col'; if (hint) th.title = t(hint); head.append(th); }
  const thead = el('thead'); thead.append(head); const body = el('tbody');
  variants.forEach((v, i) => {
    const row = el('tr', i === Number(state.chosen) ? 'selected' : ''), name = el('td'), pick = el('button', 'variant-button', candidateLabel(v));
    pick.type = 'button'; pick.setAttribute('aria-pressed', String(i === Number(state.chosen))); pick.addEventListener('click', () => { state.chosen = i; renderDetail(); }); name.append(pick);
    if (i === r.smallest_candidate) name.append(el('small', 'metric-sub status-ok', t('statusOk')));
    const bytes = el('td'); bytes.append(sizeNode(v.bytes || null));
    const saved = el('td', v.savings_bytes > 0 ? 'status-ok' : 'status-muted', v.savings_bytes > 0 ? size(v.savings_bytes) : '—'); if (v.savings_bytes) saved.title = t('exactBytes', count(v.savings_bytes));
    const score = v.difference?.ssimulacra2, perceptual = el('td', score == null ? 'status-muted' : score >= 90 ? 'status-ok' : '', score == null ? '—' : Number(score).toFixed(1));
    if (score != null) perceptual.append(el('small', 'metric-sub', scoreLabel(score)));
    const rgb = el('td', '', v.difference ? Number(v.difference.rgb_mae_255).toFixed(3) : '—');
    if (v.difference) rgb.append(el('small', 'metric-sub', `${v.difference.psnr_db == null ? '∞' : Number(v.difference.psnr_db).toFixed(2)} dB`));
    const alpha = el('td', '', v.difference ? `${(v.difference.max_alpha_error * 100).toFixed(2)}%` : '—');
    const status = candidateStatus(v), verdict = el('td', status.cls, status.text);
    row.append(name, bytes, saved, perceptual, rgb, alpha, verdict); body.append(row);
  });
  table.append(thead, body); scroll.append(table); wrap.append(heading, scroll); return wrap;
}

function renderActions(pane, r, c) {
  const block = el('section', 'apply-panel'); block.setAttribute('aria-label', t('apply'));
  if (!state.token) { block.append(el('p', '', t('offlineApply')), el('code', '', 'resopt serve <report directory>')); pane.append(block); return; }
  if (state.phase !== 'ready') { block.append(el('p', '', t('analysisRunning'))); pane.append(block); return; }
  const index = indexOf(r), current = state.operations[index], reason = blockedReason(r, c);
  const apply = el('button', 'primary', t('apply')); apply.type = 'button';
  apply.disabled = state.busy || !!reason || (current && current.state !== 'original');
  apply.addEventListener('click', () => previewApply(r, c, index));
  block.append(apply);
  if (current && current.state !== 'original') {
    const restore = el('button', '', t('restore')); restore.type = 'button'; restore.disabled = state.busy;
    restore.addEventListener('click', () => { pendingAction = { kind: 'restore', resource: index }; showConfirm(t('confirmRestore'), pathText(r), t('dialogRestore'), t('confirmRestoreButton'), false); });
    block.append(restore);
    if (current.state === 'applied') block.append(el('p', '', t('applied', candidateLabel(r.candidates[current.candidate] || c))));
    else if (current.state === 'partial') block.append(el('p', 'status-warn', t('partial')));
    else { block.append(el('p', 'status-warn', t('conflict', current.error || '')), el('p', '', t('conflictAction'))); }
  } else if (reason) block.append(el('p', '', issueText(locale, reason)));
  if (state.message) { const message = el('p', 'operation-message', state.message); message.setAttribute('role', 'status'); block.append(message); }
  pane.append(block);
}

async function previewApply(r, c, index) {
  const candidate = state.chosen, kind = warningKind(c), kinds = warningKinds(c);
  state.busy = true; state.message = t('checking'); renderDetail();
  try {
    const plan = await api('/api/preview', { resource: index, candidate, approve_warnings: kinds });
    pendingAction = { kind: 'apply', resource: index, candidate, approve_lossy: !!c.lossy, approve_warnings: kinds, plan_token: plan.plan_token };
    const references = plan.reference_files || [];
    const lines = [t('dialogChange', plan.source, plan.target, candidateLabel(c), size(r.resource.bytes), size(c.bytes), size(c.savings_bytes))];
    if (c.lossy) lines.push(t('dialogLossy'));
    for (const warning of kinds) lines.push(`⚠ ${t(`warn_${warning}`)} — ${warningDetail(c, warning)}`);
    if (kind) lines.push(t('warnRecorded'));
    if (plan.android?.usage) { const a = plan.android; lines.push(t('dialogAndroid', a.resource_name, a.resource_type, a.min_sdk ?? '?', a.usage.xml_references, a.usage.code_references)); if (a.usage.dynamic_lookup_files) lines.push(t('dialogDynamic', a.usage.dynamic_lookup_files)); }
    else lines.push(references.length ? `${t('dialogRefs', references.length)}\n${references.join('\n')}` : t('dialogNoRefs'));
    if (plan.android?.aapt2) lines.push(t('dialogCompiled', size(plan.android.aapt2.original_compiled_bytes), size(plan.android.aapt2.candidate_compiled_bytes)));
    for (const note of plan.notes || []) lines.push(issueText(locale, note));
    showConfirm(t(kind ? 'confirmWarning' : c.lossy ? 'confirmLossy' : 'confirmLossless'), lines.join('\n'), t(plan.loose_conversion ? 'dialogLoose' : 'dialogBackup'), t(kind ? 'confirmAccept' : 'confirmApply'), !!kind);
    state.message = '';
  } catch (error) { pendingAction = null; state.message = error.message; }
  finally { state.busy = false; renderDetail(); }
}

function showConfirm(title, description, note, confirmLabel, danger) {
  $('apply-title').textContent = title; $('apply-description').textContent = description; $('apply-note').textContent = note;
  $('apply-confirm').textContent = confirmLabel; $('apply-confirm').classList.toggle('danger', danger);
  $('apply-dialog').showModal(); $('apply-cancel').focus();
}

async function performAction(action) {
  state.busy = true; state.message = t('applying'); renderDetail();
  try {
    const { kind, ...body } = action;
    await api(`/api/${kind}`, body);
    state.message = t(kind === 'apply' ? 'appliedOk' : 'restoredOk');
  } catch (error) { state.message = error.message; }
  finally { state.busy = false; renderSummary(); renderList(); renderDetail(); }
}

// ---- full-size comparison ------------------------------------------------------
function openCompare(r, c) {
  const stage = $('compare-stage'); stage.replaceChildren(); stage.dataset.background = state.background;
  const canShow = displayableInBrowser(c.format) && displayableInBrowser(r.resource.format);
  // The candidate is the base layer; the original is clipped over its left part.
  const before = el('img'), after = el('img'), clip = el('div', 'compare-clip'), wrap = el('div', 'compare-wrap');
  before.src = canShow ? assetUrl(r.original_artifact) : assetUrl(r.original_preview); after.src = canShow ? assetUrl(c.artifact) : assetUrl(c.preview);
  before.alt = t('original'); after.alt = candidateLabel(c);
  clip.append(before); wrap.append(after, clip); stage.append(wrap);
  const slider = $('compare-slider'); slider.value = '50';
  const update = () => { clip.style.width = `${slider.value}%`; before.style.width = `${after.clientWidth}px`; };
  slider.oninput = update; after.onload = update;
  $('compare-caption').textContent = `${t('original')} ◀ ▶ ${candidateLabel(c)} · ${size(r.resource.bytes)} → ${size(c.bytes)}`;
  const unsupported = [c.format, r.resource.format].find(f => !displayableInBrowser(f));
  $('compare-note').textContent = canShow ? t('compareHint') : t('compareUnavailable', String(unsupported).toUpperCase());
  $('compare-dialog').showModal(); slider.focus(); update();
}

function setupDialogs() {
  $('apply-cancel').addEventListener('click', () => { $('apply-dialog').close(); pendingAction = null; });
  $('apply-dialog').addEventListener('cancel', () => { pendingAction = null; });
  $('apply-confirm').addEventListener('click', () => { const action = pendingAction; pendingAction = null; $('apply-dialog').close(); if (action && !state.busy) performAction(action); });
  $('compare-close').addEventListener('click', () => $('compare-dialog').close());
  setupBatch();
}
