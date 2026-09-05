import { test, expect } from '@playwright/test';

/**
 * Owner AAA — CEO guard + multi-key per owner
 * Base: http://127.0.0.1:8080 (cargo run -p gympos-cloud)
 * Skips if cloud not running.
 */
const CLOUD_URL = process.env.CLOUD_URL || 'http://127.0.0.1:8080';
const CEO_EMAIL = process.env.CEO_EMAIL || 'ceo@test.local';
const CEO_PASSWORD = process.env.CEO_PASSWORD || 'TestCEO123';

// Bootstrap the first CEO account (open only on a fresh database), login,
// and return a `ceo:<email>` session token for CEO-gated endpoints.
async function ceoToken(request: any): Promise<string> {
  await request.post(`${CLOUD_URL}/api/v1/auth/ceo-register`, {
    data: { email: CEO_EMAIL, password: CEO_PASSWORD, display_name: 'Test CEO' },
    headers: { 'Content-Type': 'application/json' },
  }).catch(() => {});
  const login = await request.post(`${CLOUD_URL}/api/v1/auth/ceo-login`, {
    data: { email: CEO_EMAIL, password: CEO_PASSWORD },
    headers: { 'Content-Type': 'application/json' },
  });
  const body = await login.json();
  return body.token as string;
}

async function cloudUp(request: any): Promise<boolean> {
  try { const r = await request.get(`${CLOUD_URL}/health`, { timeout: 2000 }); return r.ok(); } catch { return false; }
}

function uniqEmail(prefix='qa'){ return `${prefix}+${Date.now()}${Math.floor(Math.random()*1000)}@test.titan`; }

test.describe('CEO guard: qualified email + owner exists', () => {
  test('unqualified email -> 400 QUALIFIED_EMAIL_REQUIRED', async ({ request }) => {
    if (!await cloudUp(request)) test.skip(true, 'cloud not running');
    const CEO = await ceoToken(request);
    const r = await request.post(`${CLOUD_URL}/api/v1/gyms/register`, {
      headers: { 'Authorization': `Bearer ${CEO}`, 'Content-Type': 'application/json' },
      data: { name: 'Ghost Gym', owner_email: 'notanemail', tier: 'basic', duration_days: 30 }
    });
    expect(r.status()).toBe(400);
    const j = await r.json();
    expect(j.code).toBe('QUALIFIED_EMAIL_REQUIRED');
  });

  test('unregistered owner -> 422 UNREGISTERED_OWNER with invite_url', async ({ request }) => {
    if (!await cloudUp(request)) test.skip(true, 'cloud not running');
    const CEO = await ceoToken(request);
    const ghost = uniqEmail('ghost');
    const r = await request.post(`${CLOUD_URL}/api/v1/gyms/register`, {
      headers: { 'Authorization': `Bearer ${CEO}`, 'Content-Type': 'application/json' },
      data: { name: 'Phantom Branch', owner_email: ghost, tier: 'basic', duration_days: 30 }
    });
    expect(r.status()).toBe(422);
    const j = await r.json();
    expect(j.code).toBe('UNREGISTERED_OWNER');
    expect(String(j.invite_url)).toContain('/portal.html?invite=');
  });

  test('owner_register with bad email -> 400, then good -> 201, duplicate -> 409', async ({ request }) => {
    if (!await cloudUp(request)) test.skip(true, 'cloud not running');
    const email = uniqEmail('owner');
    // bad
    let r = await request.post(`${CLOUD_URL}/api/v1/owner/auth/register`, { data: { email: 'bad', password: '1234', company_name: 'Test' } });
    expect(r.status()).toBe(400);
    // good
    r = await request.post(`${CLOUD_URL}/api/v1/owner/auth/register`, { data: { email, password: 'StrongPass123', company_name: 'QA Titan' } });
    expect(r.status()).toBe(201);
    // duplicate
    r = await request.post(`${CLOUD_URL}/api/v1/owner/auth/register`, { data: { email, password: 'StrongPass123', company_name: 'QA Titan' } });
    expect(r.status()).toBe(409);
  });
});

test.describe('Multi-key per single owner email (interbranch)', () => {
  test('owner can create multiple branches up to tier limit, CEO can mint for same owner', async ({ request }) => {
    if (!await cloudUp(request)) test.skip(true, 'cloud not running');
    const owner = uniqEmail('multikey');
    // 1. register owner
    let r = await request.post(`${CLOUD_URL}/api/v1/owner/auth/register`, { data: { email: owner, password: 'StrongPass123', company_name: 'Multi Titan' } });
    expect(r.status()).toBe(201);
    const { token } = await r.json();
    expect(token.startsWith(`owner:${owner}:`)).toBeTruthy();
    // 2. owner self-service create gym (Pro allows 5)
    r = await request.post(`${CLOUD_URL}/api/v1/owner/gyms`, {
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      data: { name: 'Branch Alpha', owner_email: owner, tier: 'pro', duration_days: 30 }
    });
    expect(r.status()).toBe(201);
    const branch1 = await r.json();
    expect(branch1.license_key).toContain('GPOS-');
    // 3. CEO mints second key for SAME owner (different branch)
    const CEO = await ceoToken(request);
    r = await request.post(`${CLOUD_URL}/api/v1/gyms/register`, {
      headers: { 'Authorization': `Bearer ${CEO}`, 'Content-Type': 'application/json' },
      data: { name: 'Branch Beta', owner_email: owner, tier: 'pro', duration_days: 30 }
    });
    expect(r.status()).toBe(201);
    const branch2 = await r.json();
    expect(branch2.gym_id).not.toBe(branch1.gym_id);
    expect(branch2.owner_email).toBe(owner);
    // 4. Verify both appear in owner branches
    r = await request.get(`${CLOUD_URL}/api/v1/owner/branches`, { headers: { 'Authorization': `Bearer ${token}` } });
    expect(r.ok()).toBeTruthy();
    const branches = await r.json();
    const ids = (Array.isArray(branches) ? branches : branches.branches || []).map((b:any)=>b.gym_id || b.id);
    expect(ids.length).toBeGreaterThanOrEqual(2);
  });

  test('Basic tier limited to 1 branch -> second create -> 409 TIER_BRANCH_LIMIT', async ({ request }) => {
    if (!await cloudUp(request)) test.skip(true, 'cloud not running');
    const owner = uniqEmail('basiclimit');
    let r = await request.post(`${CLOUD_URL}/api/v1/owner/auth/register`, { data: { email: owner, password: 'StrongPass123', company_name: 'Basic Titan' } });
    expect(r.status()).toBe(201);
    const { token } = await r.json();
    // first gym Basic ok
    r = await request.post(`${CLOUD_URL}/api/v1/owner/gyms`, {
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      data: { name: 'Solo Branch', owner_email: owner, tier: 'basic', duration_days: 30 }
    });
    expect(r.status()).toBe(201);
    // second should be blocked
    r = await request.post(`${CLOUD_URL}/api/v1/owner/gyms`, {
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      data: { name: 'Second Branch', owner_email: owner, tier: 'basic', duration_days: 30 }
    });
    expect(r.status()).toBe(409);
    const j = await r.json();
    expect(j.code).toBe('TIER_BRANCH_LIMIT');
  });
});
