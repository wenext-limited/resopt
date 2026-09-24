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
const server = spawn(binary, ['web', project, '--out', out, '--no-open', '--no-cache', '--min-score', '90'], { stdio: ['ignore', 'pipe', 'inherit'] });
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
    assert.match(await browser.evaluate(`document.getElementById('stat-savings').title`), /(bytes|字节)/);
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

  await step('Shift and Option select a batch scope and update the inspector', async () => {
    assert.ok(await browser.evaluate(`document.querySelectorAll('.resource-row').length >= 3`));
    await browser.evaluate(`(() => { const rows = document.querySelectorAll('.resource-row'); rows[0].dispatchEvent(new MouseEvent('click', { bubbles: true })); rows[2].dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true })); })()`);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.resource-row[aria-selected="true"]').length`), 3);
    assert.ok(await browser.evaluate(`!!document.querySelector('.multi-selection')`));
    await browser.evaluate(`document.querySelectorAll('.resource-row')[1].dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true }))`);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.resource-row[aria-selected="true"]').length`), 2);
    await browser.evaluate(`(() => { const rows = document.querySelectorAll('.resource-row'); rows[3].dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true })); rows[4].dispatchEvent(new MouseEvent('click', { bubbles: true, altKey: true })); })()`);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.resource-row[aria-selected="true"]').length`), 4);
    assert.match(await text('#batch-open'), /(selected|已选)/i);
    await click('#batch-open');
    assert.match(await text('#batch-scope-label'), /(selected|已选)/i);
    await click('#batch-preview');
    await browser.waitFor(`!document.getElementById('batch-plan-view').hidden`);
    const selectedSummary = await text('#batch-summary');
    assert.match(selectedSummary, /(4 selected|已选 4 个)/);
    assert.match(selectedSummary, /(excluded|已排除)/);
    const included = await browser.evaluate(`document.querySelectorAll('#batch-items li').length`);
    const excluded = await browser.evaluate(`document.querySelectorAll('#batch-excluded-items li').length`);
    assert.equal(included + excluded, 4);
    if (excluded) assert.ok(await text('#batch-excluded-items'));
    await click('#batch-close');
    await browser.evaluate(`document.querySelector('.resource-row').dispatchEvent(new MouseEvent('click', { bubbles: true }))`);
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

  await step('comparison modes: swipe, onion skin and amplified difference', async () => {
    const pick = mode => click(`#inspector [data-compare-mode="${mode}"]`);
    // A lossy candidate, so that a difference exists.
    await browser.evaluate(`(() => { const s = document.getElementById('candidate-select'); const lossy = [...s.options].find(o => /quality|q100/.test(o.textContent)); s.value = lossy.value; s.dispatchEvent(new Event('change')); })()`);
    await pick('swipe');
    await browser.waitFor(`document.querySelector('#inspector .compare-wrap.mode-swipe .layer.top')?.naturalWidth > 0`);
    await browser.evaluate(`(() => { const i = document.querySelector('#inspector .compare-range input'); i.value = '25'; i.dispatchEvent(new Event('input')); })()`);
    assert.equal(await browser.evaluate(`document.querySelector('#inspector .layer.top').style.clipPath`), 'inset(0px 75% 0px 0px)');
    // The split is measured on the picture itself: the handle sits inside it, a quarter of the way across.
    const geometry = await browser.evaluate(`(() => { const f = document.querySelector('#inspector .compare-frame').getBoundingClientRect(), h = document.querySelector('#inspector .compare-handle').getBoundingClientRect(), img = document.querySelector('#inspector .layer.base'); return { ratio: (h.left + h.width / 2 - f.left) / f.width, aspect: f.width / f.height, natural: img.naturalWidth / img.naturalHeight }; })()`);
    assert.ok(Math.abs(geometry.ratio - 0.25) < 0.02, JSON.stringify(geometry));
    assert.ok(Math.abs(geometry.aspect - geometry.natural) < 0.05, JSON.stringify(geometry));
    await browser.screenshot(join(shots, 'compare-swipe.png'));
    await pick('onion');
    await browser.evaluate(`(() => { const i = document.querySelector('#inspector .compare-range input'); i.value = '30'; i.dispatchEvent(new Event('input')); })()`);
    assert.equal(await browser.evaluate(`document.querySelector('#inspector .layer.top').style.opacity`), '0.3');
    await pick('difference');
    await browser.waitFor(`/differ|identical/.test(document.querySelector('#inspector .compare-control .hint')?.textContent || '')`);
    assert.match(await text('#inspector .compare-control .hint'), /of pixels differ · largest difference \d+\/255/);
    const lit = await browser.evaluate(`(() => { const c = document.querySelector('#inspector canvas'); const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data; let n = 0; for (let i = 0; i < d.length; i += 4) if (d[i] + d[i + 1] + d[i + 2] > 0) n++; return n; })()`);
    assert.ok(lit > 0, 'a lossy candidate must light up some pixels');
    await browser.screenshot(join(shots, 'compare-difference.png'));
    // The lossless PNG candidate differs nowhere.
    await browser.evaluate(`(() => { const s = document.getElementById('candidate-select'); const lossless = [...s.options].find(o => /PNG · lossless/.test(o.textContent)); s.value = lossless.value; s.dispatchEvent(new Event('change')); })()`);
    await browser.waitFor(`/identical/.test(document.querySelector('#inspector .compare-control .hint')?.textContent || '')`);
    // The choice is remembered and keyboard-reachable.
    assert.equal(await browser.evaluate(`localStorage.getItem('resopt-compare')`), 'difference');
    assert.equal(await browser.evaluate(`document.querySelector('#inspector [data-compare-mode="difference"]').getAttribute('aria-pressed')`), 'true');
  });

  await step('full-size comparison dialog offers the overlay modes', async () => {
    await click('#inspector .link-button');
    assert.equal(await browser.evaluate(`document.getElementById('compare-dialog').open`), true);
    assert.equal(await browser.evaluate(`document.querySelector('#compare-stage [data-compare-mode="two-up"]').hidden`), true);
    await click('#compare-stage [data-compare-mode="swipe"]');
    await browser.waitFor(`document.querySelector('#compare-stage .compare-wrap img')?.naturalWidth > 0`);
    await browser.screenshot(join(shots, 'compare.png'));
    await click('#compare-close');
    await click('#inspector [data-compare-mode="two-up"]');
  });

  await step('SVGA rows show a rendered poster and play frame by frame', async () => {
    await click('[data-mode="all"]');
    await browser.evaluate(`(() => { const s = document.getElementById('search'); s.value = '.svga'; s.dispatchEvent(new Event('input')); })()`);
    await browser.waitFor(`document.querySelector('.resource-row .thumb img')?.naturalWidth > 0`);
    await click('.resource-row');
    await browser.waitFor(`document.querySelector('.player .canvas img')?.naturalWidth > 0`);
    assert.match(await text('.facts'), /120 × 96 · 12 fps · 4 frames/);
    const before = await browser.evaluate(`document.querySelector('.player .canvas img').src`);
    await click('.player-controls button');
    await browser.waitFor(`document.querySelector('.player .canvas img').src !== ${JSON.stringify(before)} && /animation\\/\\d+\\/\\d+/.test(document.querySelector('.player .canvas img').src)`);
    await browser.waitFor(`document.querySelector('.player .canvas img').naturalWidth > 0`);
    await click('.player-controls button');
    await browser.screenshot(join(shots, 'svga-player.png'));
    await browser.evaluate(`(() => { const s = document.getElementById('search'); s.value = ''; s.dispatchEvent(new Event('input')); })()`);
  });

  await step('translations view lists catalog coverage and argument issues', async () => {
    await click('[data-mode="translations"]');
    assert.equal(await number('#mode-translations'), 1);
    await click('.resource-row');
    await browser.waitFor(`document.querySelector('.localization-block')`);
    assert.match(await text('.localization-block'), /2 keys · 2 languages/);
    assert.match(await text('.loc-issue'), /Argument type differs.*fans.*zh-Hant/s);
    assert.match(await text('.loc-args'), /%1\$d.*%1\$s/);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.coverage tbody tr').length`), 2);
    await browser.screenshot(join(shots, 'translations.png'));
    await click('[data-mode="candidates"]');
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

  await step('batch apply, restore selected, then restore all', async () => {
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
    const appliedBefore = await number('#mode-applied');
    assert.ok(appliedBefore > 1);
    assert.notEqual(await text('#stat-applied'), '0 B');

    await click('[data-mode="applied"]');
    await browser.evaluate(`(() => { const rows = document.querySelectorAll('.resource-row'); rows[0].click(); rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true })); })()`);
    assert.equal(await browser.evaluate(`document.querySelectorAll('.resource-row[aria-selected="true"]').length`), 2);
    assert.equal(await browser.evaluate(`document.getElementById('restore-selected-open').hidden`), false);
    assert.match(await text('#restore-selected-open'), /2/);
    await click('#restore-selected-open');
    assert.equal(await browser.evaluate(`document.querySelectorAll('#batch-items li').length`), 2);
    await click('#batch-confirm');
    await browser.waitFor(`!document.getElementById('batch-done').hidden`, 120000);
    assert.match(await text('#batch-progress-text'), /Restored 2 · not restored 0/);
    await click('#batch-done');
    assert.equal(await number('#mode-applied'), appliedBefore - 2);

    if (appliedBefore > 2) {
      await click('#restore-all-open');
      await click('#batch-confirm');
      await browser.waitFor(`!document.getElementById('batch-done').hidden`, 120000);
      await click('#batch-done');
    }
    assert.deepEqual(snapshot(project), before, 'restore-all is byte-exact');
    await click('[data-mode="all"]');
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
