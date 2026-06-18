// Drive a real AI workflow through the UI and capture screenshots
// at three checkpoints: (1) initial, (2) mid-run (~6s in), (3) done.
import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();

page.on('pageerror', (e) => console.log('PAGE ERROR:', e.message));
page.on('console', (msg) => {
  if (msg.type() === 'error') console.log('CONSOLE ERROR:', msg.text());
});

await page.goto('http://127.0.0.1:1420/', { waitUntil: 'domcontentloaded', timeout: 30000 });
await page.waitForSelector('ol[aria-label="工作流时间线"]', { timeout: 20000 });
await page.waitForTimeout(1500);

console.log('1) initial screenshot');
await page.screenshot({ path: '.validation/ui-1-initial.png', fullPage: true });

console.log('Clicking 发送...');
const input = page.getByLabel('向首席代理输入指令');
await input.fill('实现一个计算斐波那契数列第 n 项的 Python 函数，附带单元测试');
await page.getByRole('button', { name: '发送' }).click();

console.log('Waiting 6s for workflow to progress...');
await page.waitForTimeout(6000);
console.log('2) mid-run screenshot');
await page.screenshot({ path: '.validation/ui-2-running.png', fullPage: true });

console.log('Waiting for completion (button text → 重置)...');
await page.waitForFunction(() => {
  const btn = document.querySelector('form[aria-label="命令输入栏"] button');
  return btn && btn.textContent && btn.textContent.includes('重置');
}, undefined, { timeout: 600_000 });
await page.waitForTimeout(800);
console.log('3) done screenshot');
await page.screenshot({ path: '.validation/ui-3-done.png', fullPage: true });

await browser.close();
console.log('All three screenshots saved to .validation/');
