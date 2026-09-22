const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ui = name => fs.readFileSync(path.join(__dirname, '../src/ui', name), 'utf8');
const FILES = ['core.js', 'i18n.js', 'app.js', 'compare.js', 'detail.js', 'archive.js', 'batch.js'];
const api = vm.runInNewContext(`${ui('core.js')}\n${ui('i18n.js')}
({similarGroupRows, differencePixels, formatSize, assetUrl, warningKind, warningKinds, blockedReason, recommendedSavings, hasWarningCandidate, displayableInBrowser, pickLocale, translate, issueText, MESSAGES})`);

test('the embedded UI parses as one script, exactly as the binary ships it', () => {
  assert.doesNotThrow(() => new vm.Script(`'use strict';(() => {${FILES.map(ui).join('\n')}\nstart();})();`));
});

test('report rows support Shift ranges and Option/Alt toggles', () => {
  const source = ui('app.js');
  const start = source.indexOf('function selectionAfterClick');
  const end = source.indexOf('async function api', start);
  const { selectionAfterClick } = vm.runInNewContext(`${source.slice(start, end)}; ({selectionAfterClick})`);
  const rows = ['a', 'b', 'c', 'd', 'e'];
  let selection = selectionAfterClick(new Set(), null, 'b', rows, false, false);
  selection = selectionAfterClick(selection.records, selection.anchor, 'e', rows, true, false);
  assert.deepEqual([...selection.records], ['b', 'c', 'd', 'e']);
  selection = selectionAfterClick(selection.records, selection.anchor, 'd', rows, false, true);
  assert.deepEqual([...selection.records], ['b', 'c', 'e']);
});

test('batch policy prefers an explicit multi-selection over the current view', () => {
  const source = ui('batch.js');
  const start = source.indexOf('function batchPolicy');
  const end = source.indexOf('function batchView', start);
  const controls = { 'batch-alpha': { checked: false }, 'batch-quality': { checked: false }, 'batch-min-score': { value: '' }, 'batch-lossless': { checked: true }, 'batch-lossy': { checked: false }, 'batch-cross': { checked: false }, 'batch-scope': { checked: false } };
  const a = {}, b = {}, c = {}, records = [a, b, c];
  const context = { state: { selectedRecords: new Set([a, c]), filtered: records }, $: id => controls[id], indexOf: r => records.indexOf(r) };
  const { batchPolicy } = vm.runInNewContext(`${source.slice(start, end)}; ({batchPolicy})`, context);
  assert.deepEqual([...batchPolicy().resources], [0, 2]);
});

test('opening a batch resets to the safe lossless same-format policy', () => {
  const source = ui('batch.js');
  const start = source.indexOf('function resetBatchPolicy');
  const end = source.indexOf('function candidatePolicyReasons', start);
  const controls = { 'batch-lossless': { checked: false }, 'batch-lossy': { checked: true }, 'batch-cross': { checked: true }, 'batch-alpha': { checked: true }, 'batch-quality': { checked: true }, 'batch-min-score': { value: '60' } };
  const { resetBatchPolicy } = vm.runInNewContext(`${source.slice(start, end)}; ({resetBatchPolicy})`, { $: id => controls[id] });
  resetBatchPolicy();
  assert.equal(controls['batch-lossless'].checked, true);
  for (const id of ['batch-lossy', 'batch-cross', 'batch-alpha', 'batch-quality']) assert.equal(controls[id].checked, false, id);
  assert.equal(controls['batch-min-score'].value, '');
});

test('batch exclusions explain every broader policy required by a candidate', () => {
  const source = ui('batch.js');
  const start = source.indexOf('function candidatePolicyReasons');
  const end = source.indexOf('function selectedBatchExclusions', start);
  const context = { warningKind: () => null, warningKinds: () => [] };
  const { candidatePolicyReasons } = vm.runInNewContext(`${source.slice(start, end)}; ({candidatePolicyReasons})`, context);
  const resource = { resource: { format: 'jpg' } };
  const candidate = { format: 'heic', lossy: true, valid: true, artifact: 'candidates/image.heic' };
  const policy = { lossless: true, lossy: false, cross_format: false, min_score: null, accept_warnings: [] };
  assert.deepEqual([...candidatePolicyReasons(resource, candidate, policy)], ['batchExcludedLossy', 'batchExcludedCross']);
});

test('selected restore scope includes only selected non-original operations', () => {
  const source = ui('app.js');
  const start = source.indexOf('function selectedAppliedResources');
  const end = source.indexOf('function appliedSavings', start);
  const a = {}, b = {}, c = {}, records = [a, b, c];
  const states = new Map([[a, 'applied'], [b, 'original'], [c, 'conflict']]);
  const context = {
    state: { selectedRecords: new Set(records) },
    isApplied: record => states.get(record) !== 'original',
    indexOf: record => records.indexOf(record),
  };
  const { selectedAppliedResources } = vm.runInNewContext(`${source.slice(start, end)}; ({selectedAppliedResources})`, context);
  assert.deepEqual([...selectedAppliedResources()], [0, 2]);
});

test('operation badges distinguish successful apply from warning states', () => {
  const source = ui('app.js');
  const start = source.indexOf('function operationBadge');
  const end = source.indexOf('function appliedSavings', start);
  const labels = { operationPartial: '未完成', operationConflict: '有冲突' };
  const operation = r => r.operation;
  const { operationBadge } = vm.runInNewContext(`${source.slice(start, end)}; ({operationBadge})`, { operation, t: key => labels[key] });
  assert.equal(operationBadge({ operation: { state: 'partial' } }), '未完成');
  assert.equal(operationBadge({ operation: { state: 'conflict' } }), '有冲突');
  assert.equal(operationBadge({ operation: { state: 'applied' } }), '');
});

test('sizes use binary units with readable precision', () => {
  assert.equal(api.formatSize(0), '0 B');
  assert.equal(api.formatSize(1023), '1,023 B');
  assert.equal(api.formatSize(1024), '1.0 KiB');
  assert.equal(api.formatSize(1536), '1.5 KiB');
  assert.equal(api.formatSize(1048576), '1.0 MiB');
  assert.equal(api.formatSize(1871973), '1.79 MiB');
  assert.equal(api.formatSize(1073741824), '1.0 GiB');
  assert.equal(api.formatSize(null), '—');
  assert.equal(api.formatSize(-1), '—');
});

test('artifact links cannot navigate to external or parent locations', () => {
  for (const value of ['javascript:alert(1)', 'https://example.com/a.png', '../outside.png', 'previews/../../outside.png', '/previews/a.png', 'previews/../a.png', 'operations/0/transaction.json']) {
    assert.equal(api.assetUrl(value), null);
  }
  assert.equal(api.assetUrl('previews/a.png'), 'previews/a.png');
  assert.equal(api.assetUrl('previews/图 1.png'), `previews/${encodeURIComponent('图 1.png')}`);
});

test('only visual policy warnings are approvable; hard failures never are', () => {
  const warning = { lossy: true, valid: false, rejection: 'alpha_error_exceeds_policy', artifact: 'candidates/a.webp', warnings: ['alpha_error_exceeds_policy', 'quality_below_policy'] };
  assert.equal(api.warningKind(warning), 'alpha_error_exceeds_policy');
  assert.deepEqual([...api.warningKinds(warning)], ['alpha_error_exceeds_policy', 'quality_below_policy']);
  assert.deepEqual([...api.warningKinds({ ...warning, warnings: undefined })], ['alpha_error_exceeds_policy']);
  assert.equal(api.warningKind({ ...warning, rejection: 'quality_below_policy' }), 'quality_below_policy');
  for (const hard of ['dimensions_changed', 'source_changed_during_analysis', 'webp_decode_failed']) {
    assert.equal(api.warningKind({ ...warning, rejection: hard }), null);
    assert.equal(api.blockedReason({ resource: { format: 'png' } }, { ...warning, rejection: hard }), 'failed_verification');
  }
  assert.equal(api.warningKind({ ...warning, lossy: false }), null);
  assert.equal(api.warningKind({ ...warning, artifact: null }), null);
  assert.equal(api.blockedReason({ resource: { format: 'png' } }, { ...warning, format: 'png' }), '');
});

test('warning candidates never count as recommended savings', () => {
  const resource = { smallest_candidate: null, candidates: [{ lossy: true, valid: false, rejection: 'quality_below_policy', artifact: 'candidates/0-heic-75.heic', savings_bytes: 900 }] };
  assert.equal(api.recommendedSavings(resource), 0);
  assert.equal(api.hasWarningCandidate(resource), true);
  assert.equal(api.recommendedSavings({ smallest_candidate: 0, candidates: [{ valid: true, artifact: 'candidates/0-png-0.png', savings_bytes: 120 }] }), 120);
  // A recorded winner that is not valid is ignored rather than trusted.
  assert.equal(api.recommendedSavings({ smallest_candidate: 0, candidates: resource.candidates }), 0);
});

test('format locks and exclusions block application with their reason', () => {
  const candidate = { valid: true, artifact: 'candidates/0-webp-85.webp', format: 'webp', lossy: true };
  assert.equal(api.blockedReason({ resource: { format: 'png', format_lock: 'android_nine_patch' } }, candidate), 'android_nine_patch');
  assert.equal(api.blockedReason({ resource: { format: 'png', format_lock: 'android_nine_patch' } }, { ...candidate, format: 'png', lossy: false }), '');
  assert.equal(api.blockedReason({ resource: { format: 'png', conversion_exclusion: 'app_icon' } }, candidate), 'app_icon');
  assert.equal(api.blockedReason({ resource: { format: 'png' } }, { ...candidate, artifact: null }), 'no_smaller_candidate');
});

test('every English string has a Chinese translation and placeholders match', () => {
  const en = api.MESSAGES.en, zh = api.MESSAGES['zh-CN'];
  assert.deepEqual(Object.keys(zh).sort(), Object.keys(en).sort());
  const holes = text => [...text.matchAll(/\{(\d+)\}/g)].map(m => m[1]).sort().join();
  for (const key of Object.keys(en)) assert.equal(holes(zh[key]), holes(en[key]), key);
});

test('every translation key used by the UI exists', () => {
  const source = FILES.map(ui).join('\n');
  const used = new Set([...source.matchAll(/\bt\('([A-Za-z0-9_]+)'/g)].map(m => m[1]));
  const report = fs.readFileSync(path.join(__dirname, '../src/report.rs'), 'utf8');
  for (const match of report.matchAll(/data-i18n(?:-label|-placeholder)?="([A-Za-z0-9_]+)"/g)) used.add(match[1]);
  for (const mode of ['Candidates', 'Warnings', 'Duplicates', 'Applied', 'Images', 'Unsupported', 'Failed', 'All']) used.add(`mode${mode}`);
  for (const kind of ['identical', 'resized', 'similar']) used.add(`dup_${kind}`);
  for (const mode of ['two-up', 'swipe', 'onion', 'difference']) used.add(`compareMode_${mode}`);
  const missing = [...used].filter(key => !api.MESSAGES.en[key]);
  assert.deepEqual(missing, []);
});

test('locale selection and issue sentences', () => {
  assert.equal(api.pickLocale(null, ['zh-TW', 'en']), 'zh-CN');
  // Preference order decides; an explicit or cleared ("auto") choice behaves as expected.
  assert.equal(api.pickLocale(null, ['en-US', 'zh-CN']), 'en');
  assert.equal(api.pickLocale('auto', ['zh-Hans-CN']), 'zh-CN');
  assert.equal(api.pickLocale('zh-CN', ['en-US']), 'zh-CN');
  assert.equal(api.pickLocale(null, []), 'en');
  assert.equal(api.pickLocale(null, ['fr-FR']), 'en');
  assert.equal(api.pickLocale('en', ['zh-CN']), 'en');
  assert.equal(api.translate('en', 'analyzing', [3, 10]), 'Analyzed 3 of 10 resources');
  assert.match(api.issueText('en', 'android_min_sdk_16_below_webp_requirement_18'), /API 18\+.*minSdk is 16/);
  assert.match(api.issueText('en', 'audio_optimization_backend_not_implemented'), /no optimizer/);
  assert.match(api.issueText('en', 'metadata_not_carried_over: tEXt, eXIf'), /tEXt, eXIf/);
  assert.match(api.issueText('en', 'palette_colors: 128'), /palette of 128 colours/);
  assert.match(api.issueText('en', 'near_lossless'), /no lossless mode/);
  assert.match(api.issueText('en', 'not_encoded_quality_75_was_not_smaller'), /quality 75 was already not smaller/);
  assert.equal(api.issueText('en', 'some_new_reason'), 'some_new_reason');
});

test('difference view: identical pictures are black, errors are amplified, alpha counts', () => {
  const px = (...values) => Uint8ClampedArray.from(values);
  const same = api.differencePixels(px(10, 20, 30, 255, 0, 0, 0, 0), px(10, 20, 30, 255, 0, 0, 0, 0), 64);
  assert.deepEqual([...same.pixels], [0, 0, 0, 255, 0, 0, 0, 255]);
  assert.deepEqual([same.max, same.changed], [0, 0]);
  // A 2/255 colour error becomes clearly visible at x64 and is reported unamplified.
  const small = api.differencePixels(px(100, 100, 100, 255), px(102, 100, 99, 255), 64);
  assert.deepEqual([...small.pixels], [128, 0, 64, 255]);
  assert.deepEqual([small.max, small.changed], [2, 1]);
  // Colour hidden under alpha 0 is not a difference; a change in alpha is.
  const hidden = api.differencePixels(px(255, 0, 0, 0), px(0, 255, 0, 0), 64);
  assert.deepEqual([hidden.max, hidden.changed], [0, 0]);
  const alpha = api.differencePixels(px(0, 0, 0, 255), px(0, 0, 0, 250), 4);
  assert.deepEqual([...alpha.pixels], [20, 20, 20, 255]);
  assert.equal(alpha.max, 5);
  // Amplified values saturate instead of wrapping.
  assert.equal(api.differencePixels(px(255, 255, 255, 255), px(0, 0, 0, 255), 64).pixels[0], 255);
});

test('browser-displayable formats exclude HEIC', () => {
  assert.equal(api.displayableInBrowser('heic'), false);
  assert.equal(api.displayableInBrowser('webp'), true);
});

test('UI sources contain no raw control characters (HTML parsing would corrupt them)', () => {
  for (const name of [...FILES, 'style.css']) {
    const bad = [...ui(name)].filter(ch => ch.charCodeAt(0) < 32 && !'\n\r\t'.includes(ch));
    assert.equal(bad.length, 0, name);
  }
});


test('similarity scores reserve 100 for identical files and handle old reports', () => {
  const source = ui('detail.js');
  const start = source.indexOf('function duplicateScore');
  const end = source.indexOf('function duplicateBlock', start);
  for (const locale of ['en', 'zh-CN']) {
    const { duplicateScore } = vm.runInNewContext(`${source.slice(start, end)}; ({duplicateScore})`, {
      t: (key, ...args) => api.translate(locale, key, args),
    });
    const group = { members: [7, 2, 9], comparisons: [
      { identical: true, score: 100 }, { identical: false, score: 99.999 }, { identical: false, score: 97.23 },
    ] };
    assert.match(duplicateScore(group, 7), /100 \/ 100/);
    assert.match(duplicateScore(group, 2), /99.9 \/ 100/);
    assert.match(duplicateScore(group, 9), /97.2 \/ 100/);
    assert.equal(duplicateScore({ members: [7, 2] }, 2), api.translate(locale, 'dupUnscored', []));
    assert.equal(duplicateScore(group, 10), api.translate(locale, 'dupUnscored', []));
  }
});


test('similar-image results paginate groups and searching a member retains its group', () => {
  const records = Array.from({ length: 120 }, (_, index) => ({ path: `image-${index}.png` }));
  const groups = [{ members: Array.from({ length: 60 }, (_, i) => i) }, { members: Array.from({ length: 60 }, (_, i) => i + 60) }];
  const rows = api.similarGroupRows(records, groups, new Set(records.map((_, i) => i)));
  assert.deepEqual([...rows], [records[0], records[60]]);
  assert.equal(Math.ceil(rows.length / 50), 1, '120 files occupy two group rows on one page');
  assert.deepEqual([...api.similarGroupRows(records, groups, new Set([73]))], [records[60]], 'a non-reference match returns the group reference');
  assert.equal(groups[1].members.length, 60, 'search never trims the group members');
  assert.deepEqual([...api.similarGroupRows(records, groups, new Set())], []);
});

test('group scores summarize members, exclude self, and preserve missing-score state', () => {
  const source = ui('app.js');
  const start = source.indexOf('function groupScoreRange');
  const end = source.indexOf('function groupBytes', start);
  const { groupScoreRange } = vm.runInNewContext(`${source.slice(start, end)}; ({groupScoreRange})`);
  const group = { members: [0, 1, 2], comparisons: [{ score: 100, identical: true }, { score: 96.2, identical: false }, { score: 98.8, identical: false }] };
  assert.deepEqual([...groupScoreRange(group)], [96.2, 98.8]);
  assert.equal(groupScoreRange({ members: [0, 1] }), null);
  assert.equal(groupScoreRange({ members: [0, 1, 2], comparisons: group.comparisons.slice(0, 2) }), null);
});
