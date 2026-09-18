// Minimal Chrome DevTools Protocol driver (Node 22+, no dependencies).
// Used by tests/browser/e2e.mjs to verify the embedded UI in a real browser.
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CANDIDATES = [
  process.env.CHROME_BIN,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].filter(Boolean);

export async function launch({ width = 1440, height = 900 } = {}) {
  const profile = mkdtempSync(join(tmpdir(), 'resopt-e2e-'));
  let chrome, endpoint;
  for (const binary of CANDIDATES) {
    try {
      chrome = spawn(binary, ['--headless=new', '--disable-gpu', '--no-first-run', '--remote-debugging-port=0', `--user-data-dir=${profile}`, `--window-size=${width},${height}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
      endpoint = await new Promise((resolve, reject) => {
        let log = '';
        chrome.stderr.on('data', chunk => { log += chunk; const m = log.match(/DevTools listening on (ws:\/\/\S+)/); if (m) resolve(m[1]); });
        chrome.on('error', reject); chrome.on('exit', () => reject(new Error('chrome exited')));
        setTimeout(() => reject(new Error('chrome did not start')), 15000);
      });
      break;
    } catch { chrome = undefined; }
  }
  if (!endpoint) throw new Error('Chrome not found; set CHROME_BIN');
  const port = new URL(endpoint).port;
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const socket = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0; const pending = new Map(); const problems = [];
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) { const { resolve, reject } = pending.get(message.id); pending.delete(message.id); message.error ? reject(new Error(message.error.message)) : resolve(message.result); }
    if (message.method === 'Runtime.exceptionThrown') problems.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') problems.push(`${message.params.entry.text} ${message.params.entry.url || ''}`);
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => { pending.set(++id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
  for (const domain of ['Page', 'Runtime', 'Log']) await send(`${domain}.enable`);
  const evaluate = async expression => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description || exceptionDetails.text);
    return result.value;
  };
  return {
    problems, send, evaluate,
    goto: async url => { await send('Page.navigate', { url }); await new Promise(r => setTimeout(r, 300)); },
    waitFor: async (expression, timeout = 60000) => {
      const deadline = Date.now() + timeout;
      while (Date.now() < deadline) { if (await evaluate(expression)) return; await new Promise(r => setTimeout(r, 100)); }
      throw new Error(`timed out waiting for: ${expression}`);
    },
    resize: (w, h) => send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: w < 600 }),
    media: features => send('Emulation.setEmulatedMedia', { features }),
    screenshot: async path => { const { data } = await send('Page.captureScreenshot', { format: 'png' }); writeFileSync(path, Buffer.from(data, 'base64')); },
    close: () => { socket.close(); chrome.kill(); try { rmSync(profile, { recursive: true, force: true }); } catch { /* best effort */ } },
  };
}
