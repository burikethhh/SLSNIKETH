import { test, expect } from '@playwright/test';

/**
 * Auth-recovery + dashboard screens (Phase: owner 401-storm fix).
 *
 * Serves cloud/dashboard statically on :8090 (see playwright.config.ts
 * `cloud-static` webServer) and mocks every /api/** call with page.route —
 * no DATABASE_URL / live cloud needed, so these tests ALWAYS run.
 *
 * What they prove (regression for the reported portal 401 waterfall):
 *  1. All CEO + portal screens/components render (headers, tables, tabs).
 *  2. A dead owner token (expired OR server-401) shows ONE login prompt and
 *     purges the stored session instead of 401-waterfalling forever.
 *  3. CEO 401s re-open the CEO login modal via adminFetch.
 *  4. Peso pricing + no demo credentials/products anywhere.
 */
const STATIC = 'http://127.0.0.1:8090';
const FUTURE_EXP = 9999999999;
const goodOwnerToken = `owner:qa@titan.fitness:${FUTURE_EXP}:deadbeef`;
const expiredOwnerToken = `owner:qa@titan.fitness:1:deadbeef`;

const CHART_STUB = `window.Chart = window.Chart || class { constructor() {} destroy() {} update() {} };`;

async function seedOwnerSession(page: any, token: string) {
  await page.addInitScript((t: string) => {
    localStorage.setItem('gympos_owner_session', JSON.stringify({
      authenticated: true, token: t, owner_email: 'qa@titan.fitness', company_name: 'QA Gyms',
    }));
  }, token);
}

const portalAnalytics = {
  owner_email: 'qa@titan.fitness', company_name: 'QA Gyms', total_branches: 1,
  total_active_members: 42, today_total_revenue: 12500.5, month_total_revenue: 310000,
  today_checkins: 17, branches: [{ gym_id: '11111111-1111-1111-1111-111111111111', name: 'QA Makati', tier: 'pro', active_members: 42, today_sales: 12500.5, today_checkins: 17, license_key: null, is_disabled: false, expires_at: null, hwid: null }],
  recent_transactions: [], revenue_by_branch: { 'QA Makati': 310000 },
  revenue_by_category: { 'Store POS': 310000 }, hourly_traffic: Array(24).fill(0),
};

const portalIncidents = {
  incidents: [
    { id: 'ATT-ABCD1234', gym_id: '11111111-1111-1111-1111-111111111111', gym_name: 'QA Makati', owner_email: 'qa@titan.fitness', member_name: '⚠️ Tailgate Intrusion', linked_member_id: 'MEM-001', person_count: 2, timestamp: new Date().toISOString(), acknowledged: false, acknowledged_by: null },
    { id: 'ATT-OLD99999', gym_id: '11111111-1111-1111-1111-111111111111', gym_name: 'QA Makati', owner_email: 'qa@titan.fitness', member_name: '⚠️ Tailgate Intrusion', linked_member_id: null, person_count: 3, timestamp: new Date().toISOString(), acknowledged: true, acknowledged_by: 'qa@titan.fitness' },
  ],
  unacked: 1, by_gym_7d: { '11111111-1111-1111-1111-111111111111': 2 },
};

async function mockPortalHappy(page: any) {
  await page.route('**/api/v1/owner/analytics', (r: any) => r.fulfill({ json: portalAnalytics }));
  await page.route('**/api/v1/owner/catalog', (r: any) => r.fulfill({ json: { products: [], plans: [], promos: [] } }));
  await page.route('**/api/v1/owner/staff', (r: any) => r.fulfill({ json: { staff: [], count: 0 } }));
  await page.route('**/api/v1/owner/incidents*', (r: any) => r.fulfill({ json: portalIncidents }));
}

async function mockPortal401(page: any) {
  await page.route('**/api/v1/owner/**', (r: any) => r.fulfill({ status: 401, json: { error: 'Unauthorized', code: 'OWNER_AUTH_REQUIRED' } }));
}

const isHidden = (page: any, sel: string) =>
  page.evaluate((s: string) => !!document.querySelector(s)?.classList.contains('hidden'), sel);

test.describe('owner portal screens + 401 recovery', () => {
  test('all 8 tabs render with views', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, goodOwnerToken);
    await mockPortalHappy(page);
    await page.goto(`${STATIC}/portal.html`);
    for (const tab of ['overview', 'keys', 'staff', 'catalog', 'plans', 'promos', 'transactions', 'incidents']) {
      await expect(page.locator(`#tab-${tab}`)).toBeAttached();
      await page.locator(`#tab-${tab}`).click();
      expect(await isHidden(page, `#view-${tab}`)).toBe(false);
    }
  });

  test('empty catalog shows builder states, never demo products', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, goodOwnerToken);
    await mockPortalHappy(page);
    await page.goto(`${STATIC}/portal.html`);
    await expect(page.locator('#catalog-tbody')).toContainText('No products yet');
    await expect(page.locator('#plans-grid')).toContainText('No membership plans yet');
    await expect(page.locator('#promos-tbody')).toContainText('No promo codes yet');
    await expect(page.locator('body')).not.toContainText('Optimum Whey');
    await expect(page.locator('body')).not.toContainText('SUMMER2026');
    await expect(page.locator('body')).not.toContainText('VIP Executive Annual');
  });

  test('incidents tab renders feed, badge, ack flow', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, goodOwnerToken);
    await mockPortalHappy(page);
    let acked = false;
    await page.route('**/api/v1/owner/incidents/*/ack', (r: any) => { acked = true; return r.fulfill({ json: { acknowledged: true } }); });
    await page.goto(`${STATIC}/portal.html`);
    await page.locator('#tab-incidents').click();
    await expect(page.locator('#incidents-tbody')).toContainText('MEM-001');
    await expect(page.locator('#incidents-tbody')).toContainText('MEM-001');
    await expect(page.locator('#incidents-badge')).toContainText('1');
    await page.getByRole('button', { name: 'Acknowledge' }).first().click();
    await expect.poll(() => acked).toBe(true);
  });

  test('peso pricing, no demo credentials in portal', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, goodOwnerToken);
    await mockPortalHappy(page);
    await page.goto(`${STATIC}/portal.html`);
    await expect(page.locator('#owner-new-branch-tier')).toContainText('₱199');
    const emailVal = await page.locator('#auth-email-input').inputValue();
    const passVal = await page.locator('#auth-pass-input').inputValue();
    expect(emailVal).toBe('');
    expect(passVal).toBe('');
  });

  test('REGRESSION: expired stored token shows login, purges session (no waterfall)', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, expiredOwnerToken);
    let apiHits = 0;
    await page.route('**/api/**', (r: any) => { apiHits++; return r.fulfill({ status: 401, json: {} }); });
    await page.goto(`${STATIC}/portal.html`);
    expect(await isHidden(page, '#modal-auth')).toBe(false);
    const stored = await page.evaluate(() => localStorage.getItem('gympos_owner_session'));
    expect(stored).toBeNull();
    expect(apiHits).toBe(0); // proactive expiry: zero doomed calls leave the browser
  });

  test('REGRESSION: server-401 storm shows ONE login prompt, purges session', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await seedOwnerSession(page, goodOwnerToken);
    await mockPortal401(page);
    await page.goto(`${STATIC}/portal.html`);
    await expect(page.locator('#auth-login-error')).toContainText('log in again', { timeout: 8000 });
    expect(await isHidden(page, '#modal-auth')).toBe(false);
    const stored = await page.evaluate(() => localStorage.getItem('gympos_owner_session'));
    expect(stored).toBeNull();
  });

  test('login without token in response stays on login screen', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await page.route('**/api/v1/owner/auth/login', (r: any) => r.fulfill({ json: { authenticated: true } }));
    await page.goto(`${STATIC}/portal.html`);
    await page.locator('#auth-email-input').fill('qa@titan.fitness');
    await page.locator('#auth-pass-input').fill('longenoughpassword');
    await page.locator('#form-auth-login button[type="submit"]').click();
    expect(await isHidden(page, '#modal-auth')).toBe(false);
  });

  test('login with token stores session and unlocks', async ({ page }) => {
    await page.addInitScript(CHART_STUB);
    await mockPortalHappy(page);
    await page.route('**/api/v1/owner/auth/login', (r: any) => r.fulfill({
      json: { authenticated: true, token: goodOwnerToken, owner_email: 'qa@titan.fitness', company_name: 'QA Gyms' },
    }));
    await page.goto(`${STATIC}/portal.html`);
    await page.locator('#auth-email-input').fill('qa@titan.fitness');
    await page.locator('#auth-pass-input').fill('longenoughpassword');
    await page.locator('#form-auth-login button[type="submit"]').click();
    await expect.poll(() => isHidden(page, '#modal-auth'), { timeout: 8000 }).toBe(true);
  });
});

test.describe('CEO command center screens + 401 recovery', () => {
  async function seedCeo(page: any) {
    await page.addInitScript(() => {
      localStorage.setItem('gympos_ceo_token', 'ceo:ceo@test.local:9999999999:deadbeef');
    });
  }

  test('screens render: fleet, vault, releases, security console', async ({ page }) => {
    await seedCeo(page);
    await page.route('**/api/**', (r: any) => {
      const url = r.request().url();
      if (url.includes('/admin/owners')) return r.fulfill({ json: [] });
      if (url.includes('/licenses')) return r.fulfill({ json: [] });
      if (url.includes('/updates/releases')) return r.fulfill({ json: [] });
      if (url.includes('/admin/incidents')) return r.fulfill({ json: { incidents: [], unacked: 0 } });
      if (url.includes('/analytics/fleet')) return r.fulfill({ json: { security_breach_by_gym_7d: {} } });
      return r.fulfill({ status: 404, json: {} });
    });
    await page.goto(`${STATIC}/`);
    await expect(page.locator('body')).toContainText('CEO COMMAND CENTER');
    await expect(page.locator('#owners-hierarchy-container')).toBeAttached();
    await expect(page.locator('#license-vault-tbody')).toBeAttached();
    await expect(page.locator('body')).toContainText('Tailgate Security Console');
    await expect(page.locator('#security-incidents-tbody')).toContainText('No tailgate incidents');
  });

  test('security console renders incidents, ack flow, 7d chips, peso tiers', async ({ page }) => {
    await seedCeo(page);
    let acked = false;
    let policySet: any = null;
    await page.route('**/api/**', (r: any) => {
      const url = r.request().url();
      if (url.includes('/admin/owners')) return r.fulfill({
        json: [{ owner_email: 'o@titan.fitness', company_name: 'Titan', total_branches: 1, pending_licenses_count: 0, branches: [{ gym_id: 'g1', name: 'Makati', tier: 'pro', license_key: 'k', is_license_active: true, days_remaining: 30 }] }],
      });
      if (url.includes('/licenses')) return r.fulfill({ json: [] });
      if (url.includes('/updates/releases')) return r.fulfill({ json: [] });
      if (url.endsWith('/ack') && r.request().method() === 'POST') { acked = true; return r.fulfill({ json: { acknowledged: true } }); }
      if (url.includes('/admin/incidents')) return r.fulfill({ json: portalIncidents });
      if (url.includes('/analytics/fleet')) return r.fulfill({ json: { security_breach_by_gym_7d: { g1: 2 } } });
      if (url.includes('/tailgate') && r.request().method() === 'POST') { policySet = r.request().postDataJSON(); return r.fulfill({ json: { ok: true } }); }
      return r.fulfill({ status: 404, json: {} });
    });
    await page.goto(`${STATIC}/`);
    await expect(page.locator('#security-incidents-tbody')).toContainText('MEM-001');
    await expect(page.locator('#ceo-security-unacked')).toContainText('1 unacked');
    await expect(page.locator('#ceo-security-branches')).toContainText('2/7d');
    await expect(page.locator('#gym-tier')).toContainText('₱199');
    await expect(page.locator('#stat-mrr')).toContainText('₱199.00'); // 1 mocked Pro branch
    await page.getByRole('button', { name: 'Acknowledge' }).first().click();
    await expect.poll(() => acked).toBe(true);
    await page.getByRole('button', { name: 'OFF' }).first().click();
    await expect.poll(() => policySet).toEqual({ enabled: false, siren_cooldown_secs: 300 });
  });

  test('REGRESSION: CEO 401 re-opens login modal', async ({ page }) => {
    await seedCeo(page);
    await page.route('**/api/**', (r: any) => {
      const url = r.request().url();
      if (url.includes('/admin/owners')) return r.fulfill({ json: [] });
      if (url.includes('/licenses')) return r.fulfill({ json: [] });
      if (url.includes('/updates/releases')) return r.fulfill({ json: [] });
      if (url.includes('/analytics/fleet')) return r.fulfill({ json: {} });
      if (url.includes('/admin/incidents')) return r.fulfill({ status: 401, json: { error: 'Unauthorized' } });
      return r.fulfill({ status: 404, json: {} });
    });
    await page.goto(`${STATIC}/`);
    expect(await isHidden(page, '#admin-key-modal')).toBe(false);
  });
});
