// Localization tables are inspected, never changed: coverage per language and
// the issues found against the source language. Strings are display text only.
const LOCALIZATION_ISSUE_KINDS = ['placeholder_type', 'placeholder_count', 'empty_value'];
const LOCALIZATION_PAGE = 50;

function localizationBlock(r) {
  const info = r.localization, block = el('section', 'localization-block');
  const own = localizationOwnLanguage(r);
  block.setAttribute('aria-label', t('locTitle'));
  block.append(el('h2', '', t('locTitle')), el('p', 'hint', t('locSummary', count(info.keys), count(info.languages.length), languageName(info, info.source_language))));
  if (info.stale_keys) block.append(el('p', 'hint', t('locStale', count(info.stale_keys))));
  block.append(localizationIssues(r, own), el('h3', '', t('locCoverage')), coverageTable(info, own));
  if (info.files?.length) {
    const files = el('details'); files.append(el('summary', '', t('locFiles', count(info.files.length))));
    for (const file of info.files) files.append(el('p', 'loc-file', `${languageName(info, file.language)} · ${file.path}`));
    block.append(files);
  }
  return block;
}

function languageName(info, language) {
  if (language === 'default' && info.format === 'android_strings') return t('locAndroidDefault');
  return language || '—';
}

function coverageTable(info, own) {
  const scroll = el('div', 'table-scroll'), table = el('table', 'coverage'), head = el('tr');
  const extra = info.languages.some(language => language.extra);
  for (const [key, hint] of [['locLanguage'], ['locTranslated'], ['locMissing', 'locMissingHint'], ['locEmpty'], ['locReview', 'locReviewHint'], ...(extra ? [['locExtra', 'locExtraHint']] : [])]) {
    const th = el('th', '', t(key)); if (hint) th.title = t(hint); head.append(th);
  }
  const thead = el('thead'), tbody = el('tbody'); thead.append(head);
  for (const language of info.languages) {
    const row = el('tr', language.language === own ? 'selected' : ''), share = translatedShare(language, info.keys);
    const name = el('td', '', languageName(info, language.language));
    if (language.language === info.source_language) name.append(el('span', 'metric-sub', t('locSource')));
    const translated = el('td', '', count(language.translated)); translated.append(el('span', 'metric-sub', share === null ? '—' : formatPercent(share)));
    const cell = (value, warn) => el('td', value && warn ? 'status-warn' : value ? '' : 'status-muted', count(value));
    row.append(name, translated, cell(language.missing, true), cell(language.empty, true), cell(language.needs_review, false), ...(extra ? [cell(language.extra, false)] : []));
    tbody.append(row);
  }
  table.append(thead, tbody); scroll.append(table); return scroll;
}

function localizationIssues(r, own) {
  const info = r.localization, section = el('div', 'loc-issues'), total = localizationIssueTotal(r);
  section.append(el('h3', '', t('locIssues', count(total))));
  if (own) section.append(el('p', 'hint', t('locOwnIssues', languageName(info, own))));
  if (!total) { section.append(el('p', 'hint', t(own ? 'locNoOwnIssues' : 'locNoIssues'))); return section; }
  const counts = own ? info.issues.reduce((all, issue) => ({ ...all, [issue.kind]: (all[issue.kind] || 0) + 1 }), {}) : info.issue_counts;
  const summary = LOCALIZATION_ISSUE_KINDS.filter(kind => counts[kind]).map(kind => `${t(`locKind_${kind}`)} ${count(counts[kind])}`);
  section.append(el('p', 'hint', summary.join(' · ')));
  if (info.issues.length < total) section.append(el('p', 'hint', t('locIssueLimit', count(info.issues.length))));
  const input = el('input'); input.type = 'search'; input.placeholder = t('locSearch'); input.setAttribute('aria-label', t('locSearch'));
  const select = el('select'); select.setAttribute('aria-label', t('locKind'));
  select.append(Object.assign(el('option', '', t('locAllKinds')), { value: '' }));
  for (const kind of LOCALIZATION_ISSUE_KINDS.filter(kind => counts[kind])) select.append(Object.assign(el('option', '', t(`locKind_${kind}`)), { value: kind }));
  const filters = el('div', 'archive-filters'); filters.append(input, select);
  const list = el('div', 'loc-issue-list'), pager = el('div', 'archive-pager'), range = el('span', 'hint');
  const previous = el('button', '', t('previous')), next = el('button', '', t('next')); previous.type = next.type = 'button';
  let page = 0;
  const draw = () => {
    const issues = filterLocalizationIssues(info.issues, input.value, select.value);
    page = Math.min(page, Math.max(0, Math.ceil(issues.length / LOCALIZATION_PAGE) - 1));
    list.replaceChildren();
    for (const issue of issues.slice(page * LOCALIZATION_PAGE, (page + 1) * LOCALIZATION_PAGE)) list.append(issueRow(info, issue));
    if (!issues.length) list.append(el('p', 'hint', t('noMatch')));
    range.textContent = issues.length ? t('range', count(page * LOCALIZATION_PAGE + 1), count(Math.min((page + 1) * LOCALIZATION_PAGE, issues.length)), count(issues.length)) : t('none');
    previous.disabled = page === 0; next.disabled = (page + 1) * LOCALIZATION_PAGE >= issues.length;
  };
  input.addEventListener('input', () => { page = 0; draw(); }); select.addEventListener('change', () => { page = 0; draw(); });
  previous.addEventListener('click', () => { page--; draw(); }); next.addEventListener('click', () => { page++; draw(); });
  pager.append(range, previous, next); section.append(filters, list, pager); draw(); return section;
}

function issueRow(info, issue) {
  const row = el('div', 'loc-issue'), head = el('div', 'loc-issue-head');
  head.append(el('span', issue.kind === 'placeholder_type' ? 'badge danger' : 'badge warn', t(`locKind_${issue.kind}`)), el('code', '', issue.key), el('span', 'hint', languageName(info, issue.language)));
  row.append(head);
  if (issue.kind !== 'empty_value') row.append(el('p', 'loc-args', t('locArguments', (issue.expected || []).join(' ') || t('locNone'), (issue.found || []).join(' ') || t('locNone'))));
  if (issue.text) row.append(el('p', 'loc-text', issue.text));
  return row;
}
