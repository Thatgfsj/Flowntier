// Capture the Settings drawer open with all 11 providers.
import { chromium } from 'playwright';

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
await page.goto('http://127.0.0.1:1420/', { waitUntil: 'domcontentloaded', timeout: 30000 });
await page.waitForSelector('ol[aria-label="工作流时间线"]', { timeout: 20000 });
await page.waitForTimeout(800);

await page.getByRole('button', { name: '设置' }).click();
await page.waitForSelector('h2:has-text("设置")', { timeout: 5000 });
await page.waitForTimeout(600);

await page.screenshot({ path: '.validation/ui-settings.png', fullPage: true });
console.log('Settings screenshot saved to .validation/ui-settings.png');
await browser.close();
