// Application state, data loading, filters and the resource list.
const boot = JSON.parse(document.getElementById('report-data').textContent);
const $ = id => document.getElementById(id);
const reducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const state = {
  token: boot.sessionToken || null,
  meta: boot.meta || null,
  records: [],            // sparse, indexed like the final report
  loaded: 0,              // rows received from the live feed
  phase: boot.sessionToken ? 'analyzing' : 'ready',
  progress: { completed: 0, total: 0 },
  error: null, disconnected: false,
  operations: {}, busy: false, message: '', capabilities: null,
  mode: 'candidates', page: 0, selected: null, selectedRecords: new Set(), selectionAnchor: null,
  chosen: null, background: 'checker', filtered: [],
};
const PAGE_SIZE = 50;

function readStored(key) { try { return localStorage.getItem(key); } catch { return null; } }
function store(key, value) { try { localStorage.setItem(key, value); } catch { /* private mode */ } }
let locale = pickLocale(readStored('resopt-language'), navigator.languages || [navigator.language]);
const t = (key, ...args) => translate(locale, key, args);
const size = value => formatSize(value, locale);
const count = value => formatNumber(value, locale);

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}
function sizeNode(value, cls) {
  const node = el('span', cls || '', size(value));
  if (typeof value === 'number') node.title = t('exactBytes', count(value));
  return node;
}
function candidateLabel(c) {
  const setting = !c.lossy ? t('lossless') : c.format === 'heic' && c.quality === 100 ? t('nearLossless') : t('quality', c.quality ?? '—');
  return `${String(c.format || '').toUpperCase()} · ${setting}`;
}
const pathText = r => String(r.resource?.path || '');
const indexOf = r => state.records.indexOf(r);
const operation = r => state.operations[indexOf(r)];
const isApplied = r => { const s = operation(r); return !!s && s.state !== 'original'; };
const isOptimized = r => operation(r)?.state === 'applied';
function operationBadge(r) {
  const status = operation(r)?.state;
  return status === 'partial' ? t('operationPartial') : status === 'conflict' ? t('operationConflict') : '';
}
function selectedAppliedResources() {
  return [...state.selectedRecords].filter(isApplied).map(indexOf);
}
function appliedSavings(r) {
  const s = operation(r);
  return s?.state === 'applied' ? (r.candidates?.[s.candidate]?.savings_bytes || 0) : 0;
}
function lowestScore(r) {
  const scores = (r.candidates || []).map(c => c.difference?.ssimulacra2).filter(v => typeof v === 'number');
  return scores.length ? Math.min(...scores) : Infinity;
}

function selectionAfterClick(current, anchor, clicked, ordered, shift, alt) {
  const next = alt ? new Set(current) : new Set();
  const start = ordered.indexOf(anchor), end = ordered.indexOf(clicked);
  if (shift && start >= 0 && end >= 0) {
    for (let i = Math.min(start, end); i <= Math.max(start, end); i++) next.add(ordered[i]);
    return { records: next, anchor };
  }
  if (alt && next.has(clicked)) next.delete(clicked); else next.add(clicked);
  return { records: next, anchor: clicked };
}

async function api(path, body) {
  const headers = { 'X-Resopt-Token': state.token };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const response = await fetch(path, { method: body === undefined ? 'GET' : 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body), cache: 'no-store' });
  const result = await response.json();
  if (result.states) state.operations = result.states;
  if (!response.ok) throw new Error(result.error || `HTTP ${response.status}`);
  return result;
}

// ---- live feed -------------------------------------------------------------
async function poll() {
  try {
    let page;
    do {
      page = await api(`/api/results?after=${state.loaded}`);
      if (page.phase === 'ready' && state.phase !== 'ready') { state.records = []; state.loaded = 0; page = await api('/api/results?after=0'); state.phase = 'ready'; }
      for (const { index, row } of page.rows) state.records[index] = row;
      state.loaded = page.next;
      state.progress = { completed: page.completed, total: page.total };
      state.error = page.error || null;
      if (page.phase === 'failed') state.phase = 'failed';
    } while (page.rows.length && state.loaded < page.completed);
    state.disconnected = false;
    if (state.phase === 'ready') {
      const [meta, operations] = await Promise.all([api('/api/report'), api('/api/state')]);
      state.meta = meta; state.operations = operations;
    }
  } catch (error) {
    state.disconnected = true;
  }
  renderStatus(); refresh(false);
  if (state.phase === 'analyzing' || state.disconnected) setTimeout(poll, state.disconnected ? 2000 : 600);
}

// ---- status, summary ---------------------------------------------------------
function renderStatus() {
  const banner = $('status'); banner.replaceChildren(); banner.className = 'status';
  const line = (text, cls) => banner.append(el('p', cls, text));
  if (state.disconnected) { banner.classList.add('error'); line(t('disconnected')); return; }
  if (state.phase === 'failed') { banner.classList.add('error'); line(t('failed', state.error)); line(t('failedAction'), 'hint'); return; }
  if (state.phase === 'analyzing') {
    const { completed, total } = state.progress;
    line(total ? t('analyzing', count(completed), count(total)) : t('scanning'));
    const bar = el('progress'); bar.max = total || 1; bar.value = completed; bar.setAttribute('aria-label', t('analyzing', completed, total)); banner.append(bar);
    const stop = el('button', '', state.cancelling ? t('cancelling') : t('cancel')); stop.type = 'button'; stop.disabled = !!state.cancelling;
    stop.addEventListener('click', async () => { state.cancelling = true; renderStatus(); try { await api('/api/cancel', {}); } catch { /* surfaced by polling */ } });
    banner.append(stop); line(t('localOnly'), 'hint'); return;
  }
  if (state.meta?.cancelled) { banner.classList.add('warn'); line(t('cancelled')); }
  const p = state.meta?.performance;
  if (p) line(t('perf', p.wall_seconds.toFixed(1), p.workers, count(p.cache_hits), count(p.duplicate_reuses)), 'hint');
  for (const tool of state.capabilities?.tools || []) if (!tool.available) line(t('tool_missing', tool.name, tool.purpose, tool.install), 'hint');
  for (const feature of state.capabilities?.features || []) if (!feature.available) line(`${feature.id.toUpperCase()}: ${feature.note}`, 'hint');
}

function renderSummary() {
  const rows = state.records.filter(Boolean);
  const opportunities = rows.filter(r => recommendedSavings(r) > 0);
  const setStat = (id, text, title) => { $(id).textContent = text; if (title) $(id).title = title; };
  const savings = opportunities.reduce((n, r) => n + recommendedSavings(r), 0);
  const applied = rows.reduce((n, r) => n + appliedSavings(r), 0);
  setStat('stat-resources', count(state.phase === 'analyzing' && state.progress.total ? state.progress.total : rows.length));
  setStat('stat-opportunities', count(opportunities.length));
  setStat('stat-savings', size(savings), t('exactBytes', count(savings)));
  setStat('stat-warnings', count(rows.filter(hasWarningCandidate).length));
  setStat('stat-applied', size(applied), t('exactBytes', count(applied)));
  const options = state.meta?.options;
  $('scope-note').textContent = options ? t('scope', size(rows.reduce((n, r) => n + (r.resource?.bytes || 0), 0)), (options.qualities || []).join(' / '), options.min_score ?? '—') : '';
  const modes = { candidates: opportunities.length, warnings: rows.filter(hasWarningCandidate).length, duplicates: duplicateGroups().size, applied: rows.filter(isApplied).length,
    images: rows.filter(r => r.resource?.kind === 'image').length, unsupported: rows.filter(r => ['unsupported', 'inventory_only'].includes(r.status)).length,
    failed: rows.filter(r => r.status === 'failed').length, all: rows.length };
  for (const [mode, value] of Object.entries(modes)) $(`mode-${mode}`).textContent = count(value);
  const formats = [...new Set(rows.map(r => r.resource?.format).filter(Boolean))].sort();
  const select = $('format-filter'), current = select.value;
  if (select.options.length !== formats.length + 1) {
    select.replaceChildren(Object.assign(el('option', '', t('allFormats')), { value: 'all' }), ...formats.map(f => Object.assign(el('option', '', f.toUpperCase()), { value: f })));
    select.value = formats.includes(current) ? current : 'all';
  }
  const live = !!state.token && state.phase === 'ready';
  const selectedApplied = selectedAppliedResources().length;
  $('batch-open').hidden = !live; $('restore-all-open').hidden = !live;
  $('restore-selected-open').hidden = !live || state.selectedRecords.size <= 1 || !selectedApplied;
  $('batch-open').textContent = state.selectedRecords.size > 1 ? t('batchSelected', count(state.selectedRecords.size)) : t('batch');
  $('restore-selected-open').textContent = t('restoreSelected', count(selectedApplied));
  $('batch-open').disabled = !opportunities.length && !modes.warnings; $('restore-all-open').disabled = !modes.applied;
}

// ---- list ---------------------------------------------------------------------
// Report index -> position of its duplicate group (groups are sorted by redundant bytes).
function duplicateGroups() {
  const groups = state.meta?.similarGroups || [];
  if (state.groupSource !== groups) { state.groupSource = groups; state.groupOf = new Map(); groups.forEach((g, rank) => g.members.forEach(i => state.groupOf.set(i, rank))); }
  return state.groupOf;
}
const MODE_FILTERS = {
  candidates: r => recommendedSavings(r) > 0, warnings: hasWarningCandidate, duplicates: r => duplicateGroups().has(indexOf(r)), applied: isApplied,
  images: r => r.resource?.kind === 'image', unsupported: r => ['unsupported', 'inventory_only'].includes(r.status),
  failed: r => r.status === 'failed', all: () => true,
};
function applyFilters() {
  const query = $('search').value.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const format = $('format-filter').value, sort = $('sort').value;
  state.filtered = state.records.filter(r => r && MODE_FILTERS[state.mode](r) && (format === 'all' || r.resource?.format === format)
    && query.every(q => `${pathText(r)} ${r.resource?.format} ${r.resource?.kind}`.toLocaleLowerCase().includes(q)));
  const by = { savings: (a, b) => recommendedSavings(b) - recommendedSavings(a), size: (a, b) => (b.resource?.bytes || 0) - (a.resource?.bytes || 0),
    name: (a, b) => basename(pathText(a)).localeCompare(basename(pathText(b))), score: (a, b) => lowestScore(a) - lowestScore(b) }[sort];
  // Members of one duplicate group stay adjacent, largest group first.
  const rank = state.mode === 'duplicates' ? (a, b) => duplicateGroups().get(indexOf(a)) - duplicateGroups().get(indexOf(b)) : () => 0;
  state.filtered.sort((a, b) => rank(a, b) || by(a, b) || pathText(a).localeCompare(pathText(b)));
}
// `reset` moves to the first page and selection; live updates keep the user's place.
function refresh(reset) {
  renderSummary(); applyFilters();
  state.selectedRecords = new Set([...state.selectedRecords].filter(r => state.filtered.includes(r)));
  if (reset) state.page = 0;
  state.page = Math.min(state.page, Math.max(0, Math.ceil(state.filtered.length / PAGE_SIZE) - 1));
  const selectionChanged = !state.selected || !state.filtered.includes(state.selected);
  if (selectionChanged) { state.selected = state.filtered[state.page * PAGE_SIZE] || null; state.chosen = preferredCandidate(state.selected); }
  if (state.selected && !state.selectedRecords.size) state.selectedRecords.add(state.selected);
  else if (state.selected && !state.selectedRecords.has(state.selected)) {
    state.selected = state.filtered.find(r => state.selectedRecords.has(r)) || state.selected;
    state.chosen = preferredCandidate(state.selected);
  }
  if (reset && state.selected) state.chosen = preferredCandidate(state.selected);
  if (!state.filtered.includes(state.selectionAnchor)) state.selectionAnchor = state.selected;
  document.querySelectorAll('.mode').forEach(b => { const on = b.dataset.mode === state.mode; b.classList.toggle('active', on); b.setAttribute('aria-pressed', String(on)); });
  renderList();
  if (selectionChanged || reset || state.phase === 'ready') renderDetail();
}
function preferredCandidate(r) {
  if (!r) return null;
  const s = operation(r);
  if (s?.state === 'applied') return s.candidate;
  // In the warnings view the point is to review the warning candidate itself.
  if (state.mode === 'warnings') {
    const warnings = (r.candidates || []).map((c, i) => [c, i]).filter(([c]) => warningKind(c)).sort((a, b) => a[0].bytes - b[0].bytes);
    if (warnings.length) return warnings[0][1];
  }
  if (Number.isInteger(r.smallest_candidate)) return r.smallest_candidate;
  const warning = (r.candidates || []).findIndex(c => warningKind(c));
  return warning >= 0 ? warning : (r.candidates?.length ? 0 : null);
}
function renderList() {
  const list = $('results'); const focused = document.activeElement?.dataset?.index; list.replaceChildren();
  const start = state.page * PAGE_SIZE, items = state.filtered.slice(start, start + PAGE_SIZE);
  if (!items.length) {
    const empty = el('div', 'empty');
    if (state.phase === 'analyzing' && !state.records.some(Boolean)) empty.append(el('strong', '', t('waiting')));
    else {
      empty.append(el('strong', '', t('noMatch')), el('span', '', t('noMatchHint')));
      const reset = el('button', '', t('clearFilters')); reset.type = 'button';
      reset.addEventListener('click', () => { $('search').value = ''; $('format-filter').value = 'all'; state.mode = 'all'; refresh(true); });
      empty.append(reset);
    }
    list.append(empty);
  }
  for (const r of items) {
    const row = el('button', 'resource-row'); row.type = 'button'; row.dataset.index = String(indexOf(r));
    row.dataset.active = String(r === state.selected); row.dataset.optimized = String(isOptimized(r));
    row.setAttribute('role', 'option'); row.setAttribute('aria-selected', String(state.selectedRecords.has(r))); row.title = pathText(r);
    const identity = el('span', 'identity'), thumb = el('span', 'thumb'), src = assetUrl(r.original_preview);
    if (src) { const img = el('img'); img.src = src; img.alt = ''; img.loading = 'lazy'; img.decoding = 'async'; thumb.append(img); }
    else thumb.textContent = String(r.resource?.format || 'file').toUpperCase().slice(0, 5);
    const copy = el('span', 'file-copy');
    copy.append(el('span', 'filename', basename(pathText(r))), el('span', 'file-context', `${String(r.resource?.format || '').toUpperCase()} · ${pathText(r).replace(/[\\/]?[^\\/]+$/, '')}`));
    identity.append(thumb, copy);
    const saved = recommendedSavings(r), cell = el('span', saved ? 'number saving' : 'number status-muted');
    if (isOptimized(r)) cell.append(operationStatus('success', '✓', t('modeApplied')));
    else if (isApplied(r)) cell.append(operationStatus('warning', '!', operationBadge(r)));
    else if (saved) cell.append(sizeNode(saved), el('small', '', `−${formatPercent(saved / (r.resource.bytes || 1))}`));
    else if (hasWarningCandidate(r)) cell.append(el('span', 'badge warn', t('modeWarnings')));
    else cell.append(el('span', '', '—'));
    row.append(identity, sizeNode(r.resource?.bytes, 'number'), cell);
    row.addEventListener('click', event => { selectRecord(r, event); if (window.matchMedia('(max-width: 760px)').matches) $('inspector').scrollIntoView({ block: 'start', behavior: reducedMotion() ? 'instant' : 'smooth' }); });
    row.addEventListener('keydown', event => {
      const move = { ArrowDown: 1, ArrowUp: -1 }[event.key]; if (!move) return;
      event.preventDefault(); const next = items[items.indexOf(r) + move];
      if (next) { selectRecord(next); list.querySelector(`[data-index="${indexOf(next)}"]`)?.focus(); }
    });
    list.append(row);
  }
  if (focused !== undefined) list.querySelector(`[data-index="${focused}"]`)?.focus();
  const total = state.filtered.length;
  renderListRange();
  $('page-number').textContent = `${total ? state.page + 1 : 0} / ${Math.ceil(total / PAGE_SIZE)}`;
  $('previous').disabled = state.page === 0; $('next').disabled = start + PAGE_SIZE >= total;
}
function renderListRange() {
  const total = state.filtered.length, start = state.page * PAGE_SIZE;
  $('range').textContent = `${total ? t('range', count(start + 1), count(Math.min(start + PAGE_SIZE, total)), count(total)) : t('none')} · ${t('selectedCount', count(state.selectedRecords.size))}`;
}
function operationStatus(kind, symbol, label) {
  const status = el('span', `operation-status ${kind}`), icon = el('span', 'operation-icon', symbol);
  icon.setAttribute('aria-hidden', 'true'); status.append(icon, el('span', '', label)); return status;
}
function selectRecord(r, event = {}) {
  const result = selectionAfterClick(state.selectedRecords, state.selectionAnchor, r, state.filtered, !!event.shiftKey, !!event.altKey);
  state.selectedRecords = result.records; state.selectionAnchor = result.anchor;
  state.selected = state.selectedRecords.has(r) ? r : state.filtered.find(item => state.selectedRecords.has(item)) || r;
  state.chosen = preferredCandidate(state.selected); state.message = '';
  for (const row of $('results').children) {
    const item = state.records[Number(row.dataset?.index)];
    row.setAttribute?.('aria-selected', String(state.selectedRecords.has(item)));
    row.dataset.active = String(item === state.selected);
  }
  renderListRange();
  renderSummary();
  renderDetail();
}
function changePage(delta) { state.page += delta; const items = state.filtered.slice(state.page * PAGE_SIZE, (state.page + 1) * PAGE_SIZE); state.selected = items.find(r => state.selectedRecords.has(r)) || items[0] || null; if (state.selected && !state.selectedRecords.size) state.selectedRecords.add(state.selected); state.chosen = preferredCandidate(state.selected); renderList(); renderDetail(); $('results').scrollTop = 0; }

// ---- static chrome ------------------------------------------------------------
function applyStaticText() {
  document.documentElement.lang = locale; document.title = `resopt · ${t('title')}`;
  document.querySelectorAll('[data-i18n]').forEach(node => { node.textContent = t(node.dataset.i18n); });
  document.querySelectorAll('[data-i18n-label]').forEach(node => { node.setAttribute('aria-label', t(node.dataset.i18nLabel)); });
  document.querySelectorAll('[data-i18n-placeholder]').forEach(node => { node.placeholder = t(node.dataset.i18nPlaceholder); });
  $('session-mode').textContent = state.token ? t('live') : t('offline');
  $('format-filter').replaceChildren();
}
function applyTheme() {
  const value = $('theme').value;
  document.documentElement.dataset.theme = value === 'system' ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : value;
}
function start() {
  $('theme').value = ['light', 'dark', 'system'].includes(readStored('resopt-theme')) ? readStored('resopt-theme') : 'system';
  $('theme').addEventListener('change', () => { applyTheme(); store('resopt-theme', $('theme').value); });
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme); applyTheme();
  const browserLanguages = navigator.languages?.length ? navigator.languages : [navigator.language];
  $('language').value = ['en', 'zh-CN'].includes(readStored('resopt-language')) ? readStored('resopt-language') : 'auto';
  $('language').addEventListener('change', () => { const choice = $('language').value; store('resopt-language', choice); locale = pickLocale(choice, browserLanguages); applyStaticText(); renderStatus(); refresh(false); renderDetail(); });
  applyStaticText();
  for (const id of ['search', 'format-filter', 'sort']) $(id).addEventListener(id === 'search' ? 'input' : 'change', () => refresh(true));
  document.querySelectorAll('.mode').forEach(b => b.addEventListener('click', () => { state.mode = b.dataset.mode; refresh(true); }));
  $('previous').addEventListener('click', () => changePage(-1)); $('next').addEventListener('click', () => changePage(1));
  document.addEventListener('keydown', event => { if (event.key === '/' && !/INPUT|SELECT|TEXTAREA/.test(document.activeElement?.tagName || '')) { event.preventDefault(); $('search').focus(); } });
  setupDialogs();
  // The launch key has done its job once the page is loaded; keep it out of the address bar and history.
  if (location.search) history.replaceState(null, '', location.pathname);
  if (state.token) {
    api('/api/capabilities').then(c => { state.capabilities = c; renderStatus(); }).catch(() => {});
    poll();
  } else {
    state.records = Array.isArray(boot.resources) ? boot.resources : [];
    if (!state.records.some(r => recommendedSavings(r) > 0)) state.mode = 'all';
    renderStatus(); refresh(true);
  }
}
