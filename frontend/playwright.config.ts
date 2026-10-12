import { defineConfig } from '@playwright/test'

// End-to-end checks run against a live stack (see docs/DEVELOPMENT.md):
//   E2E_BASE=http://localhost:8080 E2E_EMAIL=… E2E_PASSWORD=… npx playwright test
export default defineConfig({
  testDir: './e2e',
  timeout: 90_000,
  expect: { timeout: 10_000 },
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: process.env.E2E_BASE ?? 'http://localhost:8080',
    viewport: { width: 1920, height: 1080 },
    locale: 'ru-RU',
    screenshot: 'only-on-failure',
    launchOptions: { executablePath: process.env.PW_CHROMIUM || undefined, args: ['--no-sandbox'] },
  },
})
