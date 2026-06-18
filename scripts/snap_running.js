// Capture a mid-run screenshot: type a request, hit 发送, wait 3s, snapshot.
import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
await page.goto('http://127.0.0.1:1420/', { waitUntil: 'domcontentloaded', timeout: 30000 });
await page.waitForSelector('ol[aria-label="工作流时间线"]', { timeout: 20000 });
await page.waitForTimeout(800);

console.log('Typing request and clicking 发送...');
const input = page.getByLabel('向首席代理输入指令');
await input.fill('实现 POST /auth/login 接口');
await page.getByRole('button', { name: '发送' }).click();

// Wait until the workflow hits the development phase
await page.waitForFunction(() => {
  const ol = document.querySelector('ol[aria-label="工作流时间线"]');
  if (!ol) return false;
  const items = ol.querySelectorAll('li button');
  for (let i = 0; i < items.length; i++) {
    const label = items[i].getAttribute('aria-label') || '';
    if (label.includes('开发') && label.includes('active')) return true;
  }
  return false;
}, { timeout: 15000 });

await page.waitForTimeout(1500);

await page.screenshot({ path: '.validation/ui-running.png', fullPage: true });
console.log('Mid-run screenshot saved to .validation/ui-running.png');

await browser.close();
