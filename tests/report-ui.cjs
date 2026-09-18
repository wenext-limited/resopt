const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const script = fs.readFileSync(path.join(__dirname, '../src/report.js'), 'utf8');
const helpers = script.slice(script.indexOf('const number ='), script.indexOf('function setSize'));
const api = vm.runInNewContext(`${helpers}; ({formatSize, assetUrl})`);

test('the actual embedded report script parses', () => {
  assert.doesNotThrow(() => new vm.Script(script));
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
});
test('artifact links cannot navigate to external or parent locations', () => {
  for (const value of ['javascript:alert(1)', 'https://example.com/a.png', '../outside.png', 'previews/../../outside.png', '/previews/a.png', 'previews/../a.png']) {
    assert.equal(api.assetUrl(value), null);
  }
  assert.equal(api.assetUrl('previews/a.png'), 'previews/a.png');
  assert.equal(api.assetUrl('previews/图 1.png'), `previews/${encodeURIComponent('图 1.png')}`);
});

test('Alpha override is restricted to explicit visual warnings', () => {
  const start = script.indexOf('function alphaWarning');
  const end = script.indexOf('async function loadOperationStates', start);
  const policy = vm.runInNewContext(`${script.slice(start,end)}; ({alphaWarning, applicationReason})`);
  assert.equal(policy.alphaWarning({lossy:true,rejection:'alpha_error_exceeds_policy'}), true);
  assert.equal(policy.alphaWarning({lossy:true,rejection:'source_changed_during_analysis'}), false);
  assert.equal(policy.alphaWarning({lossy:false,rejection:'alpha_error_exceeds_policy'}), false);
  assert.equal(policy.applicationReason({resource:{}},{lossy:true,valid:false,rejection:'alpha_error_exceeds_policy',artifact:'candidates/a.webp'}), '');
  assert.notEqual(policy.applicationReason({resource:{}},{lossy:true,valid:false,rejection:'dimensions_changed',artifact:'candidates/a.webp'}), '');
});
test('the incremental progress script parses', () => {
  assert.doesNotThrow(() => new vm.Script(fs.readFileSync(path.join(__dirname,'../src/progress.js'),'utf8')));
});
