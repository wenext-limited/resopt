// Real-browser verification of `resopt web`. Usage:
//   node tests/browser/e2e.mjs <resopt-binary> <project-copy> [screenshot-dir]
// The project directory is MODIFIED (apply/restore); always pass a disposable copy.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { launch } from './cdp.mjs';

const [binary, project, shots = mkdtempSync(join(tmpdir(), 'resopt-shots-'))] = process.argv.slice(2);
assert.ok(binary && project, 'usage: e2e.mjs <resopt-binary> <project-copy> [screenshot-dir]');
mkdirSync(shots, { recursive: true });

function snapshot(root) {
  const files = {};
  const walk = dir => { for (const name of readdirSync(dir)) { if (name === '.git') continue; const path = join(dir, name); statSync(path).isDirectory() ? walk(path) : files[path.slice(root.length + 1)] = createHash('sha256').update(readFileSync(path)).digest('hex'); } };
  walk(root); return files;
}
const before = snapshot(project);
const out = join(mkdtempSync(join(tmpdir(), 'resopt-e2e-report-')), 'report');
const server = spawn(binary, ['web', project, '--out', out, '--no-open', '--no-cache', '--webp', '--min-score', '90'], { stdio: ['ignore', 'pipe', 'inherit'] });
const url = await new Promise((resolve, reject) => { server.stdout.on('data', chunk => { const m = String(chunk).match(/Local web: (\S+)/); if (m) resolve(m[1]); }); server.on('exit', code => reject(new Error(`resopt exited ${code}`))); });
const browser = await launch();
const step = async (name, work) => { await work(); console.log(`ok - ${name}`); };
const text = selector => browser.evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent ?? null`);
const click = selector => browser.evaluate(`(() => { const n = document.querySelector(${JSON.stringify(selector)}); if (!n) throw new Error('missing ${selector}'); n.click(); })()`);
const number = async selector => Number(String(await text(selector)).replace(/[^0-9.]/g, ''));

try {
  await browser.goto(url);
  await step('live results appear before analysis completes or immediately after', async () => {
    await browser.waitFor(`document.querySelectorAll('.resource-row').length > 0 || /complete|took/.test(document.getElementById('status').textContent)`);
  });
  await step('analysis completes and enables batch actions', async () => {
    await browser.waitFor(`!document.getElementById('batch-open').hidden`, 300000);
    assert.ok(await number('#stat-opportunities') > 0);
    assert.match(await text('#stat-savings'), /(B|KiB|MiB)$/);
    assert.match(await browser.evaluate(`document.getElementById('stat-savings').title`), /bytes/);
  });
  await browser.screenshot(join(shots, 'desktop-light-or-system.png'));

  await step('filters, search and sorting', async () => {
    await click('[data-mode="all"]');
    const all = await number('#mode-all');
    await browser.evaluate(`(() => { const s = document.getElementById('search'); s.value = 'zzzz-no-such-file'; s.dispatchEvent(new Event('input')); })()`);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.resource-row').length`), 0);
    assert.ok(await text('.empty strong'));
    await click('.empty button');
    assert.ok(await browser.evaluate(`document.querySelectorAll('.resource-row').length`) > 0);
    await browser.evaluate(`(() => { const s = document.getElementById('sort'); s.value = 'size'; s.dispatchEvent(new Event('change')); })()`);
    const sizes = await browser.evaluate(`[...document.querySelectorAll('.resource-row > .number:nth-child(2)')].map(n => Number(n.title.replace(/[^0-9]/g, '')))`);
    assert.deepEqual(sizes, [...sizes].sort((a, b) => b - a));
    assert.ok(all >= sizes.length);
    await click('[data-mode="unsupported"]');
    assert.ok(await number('#mode-unsupported') > 0, 'non-image resources are inventoried');
    await click('[data-mode="candidates"]');
  });

  await step('keyboard: "/" focuses search, arrows move the selection', async () => {
    await browser.evaluate(`document.activeElement.blur(); document.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))`);
    assert.equal(await browser.evaluate(`document.activeElement.id`), 'search');
    await browser.evaluate(`document.querySelector('.resource-row').focus()`);
    const first = await text('.detail-heading h1');
    await browser.evaluate(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))`);
    assert.notEqual(await text('.detail-heading h1'), first);
    assert.equal(await browser.evaluate(`document.querySelectorAll('[aria-selected="true"]').length`), 1);
  });

  await step('accessibility basics: every control has a name, images have alt', async () => {
    const unnamed = await browser.evaluate(`[...document.querySelectorAll('button,select,input,a')].filter(n => n.offsetParent && !(n.textContent.trim() || n.getAttribute('aria-label') || n.title || n.labels?.length)).map(n => n.outerHTML.slice(0, 80))`);
    assert.deepEqual(unnamed, []);
    assert.equal(await browser.evaluate(`[...document.images].filter(i => !i.hasAttribute('alt')).length`), 0);
  });

  await step('theme and language switch without reload', async () => {
    for (const theme of ['dark', 'light']) {
      await browser.evaluate(`(() => { const s = document.getElementById('theme'); s.value = '${theme}'; s.dispatchEvent(new Event('change')); })()`);
      assert.equal(await browser.evaluate(`document.documentElement.dataset.theme`), theme);
      await new Promise(r => setTimeout(r, 400));
      await browser.screenshot(join(shots, `desktop-${theme}.png`));
    }
    await browser.evaluate(`(() => { const s = document.getElementById('language'); s.value = 'zh-CN'; s.dispatchEvent(new Event('change')); })()`);
    assert.equal(await text('[data-i18n="statResources"]'), '资源文件');
    await browser.evaluate(`(() => { const s = document.getElementById('language'); s.value = 'en'; s.dispatchEvent(new Event('change')); })()`);
    assert.equal(await text('[data-i18n="statResources"]'), 'Resources');
  });

  await step('full-size comparison dialog opens and closes', async () => {
    await click('.link-button');
    assert.equal(await browser.evaluate(`document.getElementById('compare-dialog').open`), true);
    await browser.waitFor(`document.querySelector('.compare-wrap img')?.naturalWidth > 0`);
    await browser.screenshot(join(shots, 'compare.png'));
    await click('#compare-close');
  });

  await step('warning candidates need an explicit, separately worded confirmation', async () => {
    await click('[data-mode="warnings"]');
    assert.ok(await number('#mode-warnings') > 0, 'fixture should yield warnings at --min-score 90');
    await browser.waitFor(`!!document.querySelector('.warning-block')`);
    await click('.apply-panel .primary');
    await browser.waitFor(`document.getElementById('apply-dialog').open`);
    assert.match(await text('#apply-title'), /Warning/);
    assert.match(await text('#apply-confirm'), /accept/i);
    await browser.screenshot(join(shots, 'warning-dialog.png'));
    await click('#apply-cancel');
    assert.deepEqual(snapshot(project), before, 'cancelling changes nothing');
  });

  await step('single apply and restore round-trip', async () => {
    await click('[data-mode="candidates"]');
    await click('.apply-panel .primary');
    await browser.waitFor(`document.getElementById('apply-dialog').open`);
    await click('#apply-confirm');
    await browser.waitFor(`/Applied/.test(document.querySelector('.apply-panel')?.textContent || '')`);
    assert.notDeepEqual(snapshot(project), before);
    assert.ok(await number('#mode-applied') === 1);
    await browser.evaluate(`[...document.querySelectorAll('.apply-panel button')].find(b => /Restore/.test(b.textContent)).click()`);
    await click('#apply-confirm');
    await browser.waitFor(`document.getElementById('mode-applied').textContent === '0'`);
    assert.deepEqual(snapshot(project), before, 'restore is byte-exact');
  });

  await step('batch: policy -> preview -> confirm -> outcomes, then restore all', async () => {
    await click('#batch-open');
    assert.equal(await browser.evaluate(`document.getElementById('batch-alpha').disabled`), true, 'warnings need lossy enabled');
    await click('#batch-preview');
    await browser.waitFor(`!document.getElementById('batch-plan-view').hidden`);
    const summary = await text('#batch-summary');
    assert.match(summary, /0 lossy · 0 with accepted warnings · 0 format changes/);
    await browser.screenshot(join(shots, 'batch-preview.png'));
    await click('#batch-confirm');
    await browser.waitFor(`!document.getElementById('batch-done').hidden`, 120000);
    assert.match(await text('#batch-progress-text'), /Applied \d+ · failed 0/);
    await browser.screenshot(join(shots, 'batch-done.png'));
    await click('#batch-done');
    assert.ok(await number('#mode-applied') > 1);
    assert.notEqual(await text('#stat-applied'), '0 B');
    await click('#restore-all-open');
    await click('#batch-confirm');
    await browser.waitFor(`!document.getElementById('batch-done').hidden`, 120000);
    await click('#batch-done');
    assert.deepEqual(snapshot(project), before, 'restore-all is byte-exact');
  });

  await step('responsive layout at phone width has no horizontal overflow', async () => {
    await browser.resize(390, 844);
    await new Promise(r => setTimeout(r, 300));
    assert.ok(await browser.evaluate(`document.documentElement.scrollWidth <= window.innerWidth + 1`));
    await browser.resize(1440, 900);
    assert.ok(await browser.evaluate(`[...document.querySelectorAll('.toolbar > *')].every(n => n.hidden || n.getBoundingClientRect().right <= window.innerWidth)`), 'toolbar controls stay inside the viewport');
    await browser.resize(390, 844);
    await browser.screenshot(join(shots, 'phone.png'));
    await browser.resize(1440, 900);
  });

  await step('reduced motion disables animations', async () => {
    await browser.media([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await click('.resource-row');
    assert.equal(await browser.evaluate(`document.getElementById('inspector').getAnimations().length`), 0);
  });

  await step('no script errors or failed requests', async () => { assert.deepEqual(browser.problems, []); });
  console.log(`screenshots: ${shots}`);
} catch (error) {
  await browser.screenshot(join(shots, 'failure.png')).catch(() => {});
  console.error('page state:', await browser.evaluate(`JSON.stringify({ status: document.getElementById('status').textContent, batch: document.getElementById('batch-progress-text').textContent, error: document.getElementById('batch-error').textContent, problems: ${JSON.stringify([])} })`).catch(() => 'unavailable'), browser.problems);
  throw error;
} finally { browser.close(); server.kill(); }
