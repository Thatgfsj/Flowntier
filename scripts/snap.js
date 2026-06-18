// Quick screenshot script. Tolerant of slow HMR.
import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
await page.goto('http://127.0.0.1:1420/', { waitUntil: 'domcontentloaded', timeout: 30000 });
await page.waitForSelector('ol[aria-label="工作流时间线"]', { timeout: 20000 });
await page.waitForTimeout(800);
await page.screenshot({ path: '.validation/ui.png', fullPage: true });
console.log('ok');
await browser.close();
