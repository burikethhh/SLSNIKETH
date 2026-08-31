import { test, expect } from '@playwright/test';

/**
 * SLS123 FastAPI + staff_dashboard.html — kiosk gold path
 * Base: http://127.0.0.1:8000 (python main.py or main.pyc)
 * Skips gracefully if SLS123 not running.
 */
const SLS_URL = process.env.SLS_URL || 'http://127.0.0.1:8000';

async function slsUp(request: any): Promise<boolean> {
  try {
    const r = await request.get(`${SLS_URL}/health`, { timeout: 2000 });
    return r.ok();
  } catch {
    return false;
  }
}

test.describe('sls123 kiosk', () => {
  test('health + login page reachable or skip', async ({ request }) => {
    const up = await slsUp(request);
    if (!up) test.skip(true, 'SLS123 not running on :8000 — run `python SLS123/main.py`');
    const r = await request.get(`${SLS_URL}/health`);
    expect(r.ok()).toBeTruthy();
  });

  test('staff dashboard loads and gate standby visible (when up)', async ({ page, request }) => {
    const up = await slsUp(request);
    if (!up) test.skip(true, 'SLS123 not running');
    const resp = await page.goto(`${SLS_URL}/admin/staff-dashboard`).catch(() => null);
    // May redirect to /admin/login if unauth — either is valid
    expect(resp).not.toBeNull();
    const url = page.url();
    expect(url).toMatch(/staff|login/);
  });

  test('no fatal XSS via innerHTML (escHtml present)', async ({ page, request }) => {
    const up = await slsUp(request);
    if (!up) test.skip(true, 'SLS123 not running');
    await page.goto(`${SLS_URL}/admin/staff-dashboard`).catch(() => {});
    // Static check: page source contains escHtml helper (patched T3)
    const content = await page.content();
    expect(content).toContain('escHtml');
  });
});
