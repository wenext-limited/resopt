const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ui = name => fs.readFileSync(path.join(__dirname, '../src/ui', name), 'utf8');
const FILES = ['core.js', 'i18n.js', 'app.js', 'detail.js', 'batch.js'];
const api = vm.runInNewContext(`${ui('core.js')}\n${ui('i18n.js')}
({formatSize, assetUrl, warningKind, warningKinds, blockedReason, recommendedSavings, hasWarningCandidate, displayableInBrowser, pickLocale, translate, issueText, MESSAGES})`);

test('the embedded UI parses as one script, exactly as the binary ships it', () => {
  assert.doesNotThrow(() => new vm.Script(`'use strict';(() => {${FILES.map(ui).join('\n')}\nstart();})();`));
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
  const missing = [...used].filter(key => !api.MESSAGES.en[key]);
  assert.deepEqual(missing, []);
});

test('locale selection and issue sentences', () => {
  assert.equal(api.pickLocale(null, ['zh-TW', 'en']), 'zh-CN');
  assert.equal(api.pickLocale(null, ['fr-FR']), 'en');
  assert.equal(api.pickLocale('en', ['zh-CN']), 'en');
  assert.equal(api.translate('en', 'analyzing', [3, 10]), 'Analyzed 3 of 10 resources');
  assert.match(api.issueText('en', 'android_min_sdk_16_below_webp_requirement_18'), /API 18\+.*minSdk is 16/);
  assert.match(api.issueText('en', 'audio_optimization_backend_not_implemented'), /no optimizer/);
  assert.match(api.issueText('en', 'metadata_not_carried_over: tEXt, eXIf'), /tEXt, eXIf/);
  assert.equal(api.issueText('en', 'some_new_reason'), 'some_new_reason');
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
