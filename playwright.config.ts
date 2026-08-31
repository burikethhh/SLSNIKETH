import { defineConfig, devices } from '@playwright/test';
import path from 'path';

/**
 * GymPOS Playwright — 3 components: Desktop WebView, Cloud Dashboard, SLS123
 * - desktop: static Tauri WebView (index.html) served via http-server
 * - cloud: Axum dashboard (needs `cargo run -p gympos-cloud` or static preview)
 * - sls123: FastAPI templates (served by python main.py) — optional if running
 * All tests use mock Tauri IPC fallback (window.__TAURI__ undefined) so they pass without backend.
 */
export default defineConfig({
  testDir: './tests/playwright',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [['html', { open: 'never' }], ['list']],
  use: {
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  projects: [
    {
      name: 'desktop',
      testMatch: /desktop\.spec\.ts/,
      use: { ...devices['Desktop Chrome'], baseURL: 'http://127.0.0.1:5175' },
    },
    {
      name: 'cloud',
      testMatch: /(cloud|owner-guard)\.spec\.ts/,
      use: { ...devices['Desktop Chrome'], baseURL: 'http://127.0.0.1:8080' },
    },
    {
      name: 'sls123',
      testMatch: /sls123\.spec\.ts/,
      use: { ...devices['Desktop Chrome'], baseURL: 'http://127.0.0.1:8000' },
    },
  ],
  webServer: [
    {
      // Desktop static preview — serves desktop/webview as http://127.0.0.1:5175
      command: 'npx http-server desktop/webview -p 5175 --cors -c-1 --silent',
      url: 'http://127.0.0.1:5175',
      reuseExistingServer: !process.env.CI,
      timeout: 20_000,
      stdout: 'ignore',
      stderr: 'ignore',
    },
    // Cloud and SLS123 are optional — tests will be skipped if not running (see beforeAll checks)
  ],
});
