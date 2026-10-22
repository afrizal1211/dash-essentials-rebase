---
name: playwright-mcp
description: Use for browser automation, web scraping, UI testing, and verification via Playwright MCP. Uses the installed playwright-cli via npx @playwright/mcp.
---

# Playwright MCP Skill

## Overview
Uses Playwright MCP server via stdio transport. Requires `playwright-cli` installed (already available via npm).

## Configuration
Add to your `opencode.json`:
```json
{
  "mcp": {
    "playwright": {
      "type": "local",
      "command": ["npx", "-y", "@playwright/mcp@latest"],
      "enabled": true,
      "environment": {
        "PLAYWRIGHT_BROWSERS_PATH": "/home/ubuntu/.cache/ms-playwright"
      }
    }
  }
}
```

## Available Tools (via MCP)
- `browser_open` - Navigate to URL
- `browser_click` - Click element
- `browser_fill` - Fill form fields
- `browser_screenshot` - Capture screenshot
- `browser_evaluate` - Run JS in page context
- `browser_snapshot` - Get accessibility tree with element refs
- `browser_wait` - Wait for condition
- `browser_close` - Close page/browser

## Android Device Automation (via ADB)
```javascript
// Connect to Android device via Playwright's experimental Android support
const { _android: android } = require('playwright');

const [device] = await android.devices();
await device.screenshot({ path: 'device.png' });

// WebView automation
await device.shell('am start org.chromium.webview_shell/.WebViewBrowserActivity');
const webview = await device.webView({ pkg: 'org.chromium.webview_shell' });
await webview.fill({ res: 'org.chromium.webview_shell:id/url_field' }, 'https://example.com');
await webview.press({ res: 'org.chromium.webview_shell:id/url_field' }, 'Enter');

// Chrome automation
const context = await device.launchBrowser();
const page = await context.newPage();
await page.goto('https://example.com');
```

## Flutter Web Testing
```bash
# Build Flutter web
cd ui/flutter && flutter build web --release

# Test with Playwright
npx playwright test --project=chromium tests/flutter-web.spec.ts
```

## Common Tasks for This Project

### Test Dash Essential Flutter App (Web)
```javascript
// Navigate to local Flutter web build
await browser.open('http://localhost:8080');

// Test per-app profile UI
await browser.click('#per-app-tab');
await browser.fill('#package-input', 'com.mojang.minecraftpe');
await browser.click('#profile-gpu-1');
await browser.click('#save-button');

// Verify GPU profile applied
const result = await browser.evaluate(() => {
  return window.dashEssential?.getGpuProfile?.();
});
```

### Screenshot Comparison (Visual Regression)
```bash
# Capture baseline
playwright screenshot --url=http://localhost:8080 --output=baseline.png

# Compare after changes
playwright screenshot --url=http://localhost:8080 --output=current.png
playwright compare baseline.png current.png --threshold=0.1
```

## Environment Variables
```bash
# Playwright browsers path
export PLAYWRIGHT_BROWSERS_PATH=/home/ubuntu/.cache/ms-playwright

# For headed mode (if display available)
export PLAYWRIGHT_HEADLESS=false

# Slow motion for debugging
export PLAYWRIGHT_SLOW_MO=1000
```

## Integration with Device Testing
Since your device is accessible via ADB from the container:

```bash
# Forward ADB port for Playwright Android
adb forward tcp:9222 localabstract:chrome_devtools_remote

# Connect Playwright to device Chrome
playwright connect "http://localhost:9222"
```

## Commands
- `npx playwright install` - Install browsers
- `npx playwright install-deps` - Install system dependencies
- `npx playwright test` - Run tests
- `npx playwright show-report` - View HTML report
- `npx playwright codegen` - Record actions to generate test code