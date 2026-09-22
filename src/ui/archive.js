// Archive entries stay inside their parent package; paths are display text only.
function archiveBlock(r) {
  const info = r.archive, block = el('section', 'archive-block');
  block.setAttribute('aria-label', t('archiveContents'));
  block.append(el('h2', '', t('archiveContents')), el('p', 'hint', t('archiveSummary', count(info.entries.length), size(r.resource.bytes), size(info.expanded_bytes))), el('p', 'hint', t('archivePreviewLimit', info.preview_limit)));
  if (info.optimized_images) block.append(el('p', 'hint', t('archiveOptimized', count(info.optimized_images))));
  if (info.rewrite_blockers.length) block.append(el('p', 'reasons', t('archiveProtected')));
  const input = el('input'); input.type = 'search'; input.placeholder = t('archiveSearch'); input.setAttribute('aria-label', t('archiveSearch'));
  const select = el('select'); select.setAttribute('aria-label', t('archiveType'));
  select.append(Object.assign(el('option', '', t('allFormats')), { value: '' }));
  for (const format of [...new Set(info.entries.map(e => e.format).filter(Boolean))].sort()) select.append(Object.assign(el('option', '', format.toUpperCase()), { value: format }));
  const filters = el('div', 'archive-filters'); filters.append(input, select); block.append(filters);
  const list = el('div', 'archive-entries'), pager = el('div', 'archive-pager'), range = el('span', 'hint');
  const previous = el('button', '', t('previous')), next = el('button', '', t('next')); previous.type = next.type = 'button';
  let page = 0;
  const draw = () => {
    const query = input.value.trim().toLocaleLowerCase();
    const entries = info.entries.filter(e => (!query || e.path.toLocaleLowerCase().includes(query)) && (!select.value || e.format === select.value));
    page = Math.min(page, Math.max(0, Math.ceil(entries.length / 50) - 1));
    list.replaceChildren();
    for (const entry of entries.slice(page * 50, (page + 1) * 50)) {
      const row = el('div', 'archive-entry'), thumb = el('span', 'thumb'), preview = assetUrl(entry.preview);
      if (preview) { const link = el('a'); link.href = preview; link.target = '_blank'; link.rel = 'noopener'; link.title = t('dupOpenPreview'); const img = el('img'); img.src = preview; img.alt = entry.path; img.loading = 'lazy'; link.append(img); thumb.append(link); }
      else thumb.textContent = entry.directory ? '▸' : (entry.format || '—').toUpperCase().slice(0, 5);
      const identity = el('div', 'archive-entry-name'); identity.append(el('span', 'path', entry.path));
      if (entry.metadata) identity.append(el('span', 'hint', t('archiveMetadata')));
      if (entry.issues.length) identity.append(el('span', 'hint', entry.issues.map(x => x.startsWith('archive_image_unchanged: ') ? t('archiveImageUnchanged', x.slice(25)) : t('archivePreviewUnavailable', x.replace(/^preview_unavailable: /, ''))).join('; ')));
      const bytes = el('span', 'number'); bytes.append(sizeNode(entry.bytes), el('small', '', t('archiveStored', size(entry.compressed_bytes))));
      row.append(thumb, identity, bytes); list.append(row);
    }
    if (!entries.length) list.append(el('p', 'hint', t('noMatch')));
    range.textContent = entries.length ? t('range', count(page * 50 + 1), count(Math.min((page + 1) * 50, entries.length)), count(entries.length)) : t('none');
    previous.disabled = page === 0; next.disabled = (page + 1) * 50 >= entries.length;
  };
  input.addEventListener('input', () => { page = 0; draw(); }); select.addEventListener('change', () => { page = 0; draw(); });
  previous.addEventListener('click', () => { page--; draw(); }); next.addEventListener('click', () => { page++; draw(); });
  pager.append(range, previous, next); block.append(list, pager); draw(); return block;
}
