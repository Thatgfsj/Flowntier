// Take a screenshot of the running Mission Control UI at the
// configured Vite dev URL. Saves to .validation/ui.png (gitignored).
//
// Usage: node scripts/screenshot_ui.js [url] [outPath]

import { chromium } from 'playwright';
import { mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

const url = process.argv[2] ?? 'http://127.0.0.1:1420/';
const out = process.argv[3] ?? resolve(ROOT, '.validation', 'ui.png');

await mkdir(dirname(out), { recursive: true });

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
const page = await context.newPage();

const errors = [];
page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
page.on('console', (msg) => {
  if (msg.type() === 'error') errors.push(`console.error: ${msg.text()}`);
});

console.log(`Loading ${url}...`);
const resp = await page.goto(url, { waitUntil: 'networkidle', timeout: 15_000 });
console.log(`  status: ${resp?.status() ?? 'no response'}`);

// Wait for the React app to render (PhaseTimeline is rendered on mount)
await page.waitForSelector('ol[aria-label="Workflow phase timeline"]', { timeout: 10_000 });
console.log('  Mission Control UI rendered.');

await page.waitForTimeout(500); // settle animations

await page.screenshot({ path: out, fullPage: true });
console.log(`Screenshot saved to ${out}`);

if (errors.length > 0) {
  console.log('--- page errors ---');
  for (const e of errors) console.log('  ' + e);
}

await browser.close();
