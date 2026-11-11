import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  use: { baseURL: 'http://127.0.0.1:1420', locale: 'zh-CN', viewport: { width: 1120, height: 790 }, screenshot: 'only-on-failure' },
  projects: [
    { name: 'mac-webkit', use: { browserName: 'webkit' } },
    { name: 'windows-chromium', use: { browserName: 'chromium' } },
  ],
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI },
});
