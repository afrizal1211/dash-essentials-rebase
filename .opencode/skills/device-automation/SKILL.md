---
name: device-automation
description: Use for device automation via ADB, SSH, Playwright Android, and DeviceRail. Covers remote device control, log collection, crash dump analysis, and automated testing on physical Android devices.
---

# Device Automation for dash-essential

## ADB Automation

### Device Connection
```bash
# List devices
adb devices -l

# Check device properties
adb shell getprop ro.product.model
adb shell getprop ro.build.version.sdk

# File transfer
adb push local_file /data/local/tmp/
adb pull /data/adb/dash-essential/crash.log ./crash.log
```

### Shell Commands
```bash
# Run as root (requires KernelSU/Magisk)
adb shell su -c 'command'

# Run daemon command
adb shell su -c 'echo "GET health" | nc 127.0.0.1 732'

# Kill daemon
adb shell su -c 'killall dash_daemon'

# View logs
adb shell su -c 'cat /data/adb/dash-essential/backend.log'
adb shell su -c 'cat /data/adb/dash-essential/crash.log'
```

### Sysfs/Proc Access
```bash
# GPU EB DVFS
adb shell su -c 'cat /sys/kernel/ged/hal/eb_dvfs_policy'

# GPU frequency
adb shell su -c 'cat /proc/gpufreqv2/gpufreq_status'

# Touch rate
adb shell su -c 'cat /proc/xm_htc_report_rate'

# Set touch rate
adb shell su -c 'echo 240 > /proc/xm_htc_report_rate'
```

## SSH Access (Container → Device)
```bash
# From container (if SSH daemon on device)
ssh root@127.0.0.1 -p 2222

# Or via ADB TCP forwarding
adb forward tcp:2222 tcp:22
ssh root@localhost -p 2222
```

## Playwright Android Automation

### Setup
```bash
# Install Playwright
npm install -g playwright
playwright install chromium

# Connect to device
npx playwright android devices
```

### Device Automation Script
```javascript
const { _android: android } = require('playwright');

async function automateDashEssential() {
  const [device] = await android.devices();
  
  // Screenshot
  await device.screenshot({ path: 'device.png' });
  
  // Launch Dash Essential app
  await device.shell('am start -n io.github.neeschal.dashessential/.MainActivity');
  
  // WebView testing (if app uses WebView)
  await device.shell('am force-stop org.chromium.webview_shell');
  await device.shell('am start org.chromium.webview_shell/.WebViewBrowserActivity');
  const webview = await device.webView({ pkg: 'org.chromium.webview_shell' });
  
  // Fill URL
  await webview.fill({ res: 'org.chromium.webview_shell:id/url_field' }, 'http://localhost:8080');
  await webview.press({ res: 'org.chromium.webview_shell:id/url_field' }, 'Enter');
  
  // Get page
  const page = await webview.page();
  await page.waitForLoadState('networkidle');
  
  // Test per-app UI
  await page.click('#per-app-tab');
  await page.fill('#package-input', 'com.mojang.minecraftpe');
  await page.click('#save-button');
  
  // Screenshot result
  await page.screenshot({ path: 'per-app-result.png' });
}

automateDashEssential().catch(console.error);
```

## DeviceRail Automation (Advanced)

### Setup
```bash
# Build DeviceRail
git clone https://github.com/wangweiwei/device-rail
cd device-rail
cargo build --release -p devicerail-daemon

# Start with Playwright endpoint
DEVICERAIL_PLAYWRIGHT_ENDPOINT=ws://127.0.0.1:3000/session-token \
cargo run -p devicerail-daemon
```

### JSON-RPC Commands
```json
{
  "jsonrpc": "2.0",
  "method": "execute",
  "params": {
    "device": "android",
    "action": "shell",
    "args": ["su -c 'cat /data/adb/dash-essential/crash.log'"]
  },
  "id": 1
}
```

## Crash Dump Collection

### Automated Crash Dump
```bash
#!/bin/bash
# collect_crash_dump.sh

DEVICE_ID=$1
OUTPUT_DIR="./crash_dumps/$(date +%Y%m%d_%H%M%S)"

mkdir -p "$OUTPUT_DIR"

# Crash log
adb -s "$DEVICE_ID" shell su -c 'cat /data/adb/dash-essential/crash.log' > "$OUTPUT_DIR/crash.log"

# Backend log
adb -s "$DEVICE_ID" shell su -c 'cat /data/adb/dash-essential/backend.log' > "$OUTPUT_DIR/backend.log"

# Health JSON
adb -s "$DEVICE_ID" shell su -c 'echo "GET health" | nc 127.0.0.1 732' > "$OUTPUT_DIR/health.json"

# System info
adb -s "$DEVICE_ID" shell getprop > "$OUTPUT_DIR/props.txt"
adb -s "$DEVICE_ID" shell dumpsys cpuinfo > "$OUTPUT_DIR/cpuinfo.txt"
adb -s "$DEVICE_ID" shell dumpsys meminfo > "$OUTPUT_DIR/meminfo.txt"

# Kernel log
adb -s "$DEVICE_ID" shell su -c 'dmesg' > "$OUTPUT_DIR/dmesg.txt"

echo "Crash dump collected to $OUTPUT_DIR"
```

### Crash Log Analysis
```python
#!/usr/bin/env python3
# analyze_crash.py

import re
import sys

def parse_crash_log(log_path):
    with open(log_path) as f:
        content = f.read()
    
    crashes = re.findall(
        r'DASH_CRASH signal=(\d+)\s+((?:\s+\d+: 0x[0-9a-f]+\n)+)',
        content
    )
    
    for signal, frames in crashes:
        print(f"Signal: {signal} ({signal_name(int(signal))})")
        for frame in frames.strip().split('\n'):
            if frame.strip():
                addr = frame.split(':')[1].strip()
                print(f"  {frame.strip()} -> {addr2line(addr)}")

def signal_name(sig):
    names = {11: 'SIGSEGV', 7: 'SIGBUS', 4: 'SIGILL', 8: 'SIGFPE', 6: 'SIGABRT', 5: 'SIGTRAP'}
    return names.get(sig, 'UNKNOWN')

def addr2line(addr):
    # Run addr2line on the daemon binary
    import subprocess
    try:
        result = subprocess.run(
            ['addr2line', '-e', 'dash_daemon', addr],
            capture_output=True, text=True, timeout=5
        )
        return result.stdout.strip() or '??'
    except:
        return '??'

if __name__ == '__main__':
    parse_crash_log(sys.argv[1])
```

## Log Collection & Monitoring

### Continuous Log Monitoring
```bash
#!/bin/bash
# monitor_daemon.sh

LOG_FILE="/data/adb/dash-essential/backend.log"
CRASH_FILE="/data/adb/dash-essential/crash.log"

tail -f "$LOG_FILE" | while read line; do
    echo "[$(date)] $line"
    
    # Alert on crash
    if echo "$line" | grep -q "Segmentation fault"; then
        echo "!!! DAEMON CRASH DETECTED !!!"
        # Trigger alert (email, webhook, etc.)
    fi
done
```

### Health Monitoring
```python
#!/usr/bin/env python3
# health_monitor.py

import json
import time
import subprocess

def get_health():
    cmd = ['adb', 'shell', 'su', '-c', 'echo "GET health" | nc 127.0.0.1 732']
    output = subprocess.run(cmd, capture_output=True, text=True).stdout
    if output.startswith('OK '):
        return json.loads(output[3:])
    return None

def monitor():
    while True:
        health = get_health()
        if health:
            gpu = health.get('gpu', {})
            display = health.get('display', {})
            
            print(f"GPU: {gpu.get('liveMHz')}MHz / {gpu.get('targetMHz')}MHz capped by {gpu.get('cappedBy')}")
            print(f"Display route: {display.get('route')}")
            print(f"EB DVFS: {gpu.get('vendorEbDvfs')}")
            print(f"Per-app: {health.get('perApp', {}).get('ownedControls')}")
        
        time.sleep(30)

if __name__ == '__main__':
    monitor()
```

## Automated Testing Pipeline

### Pre-Release Checklist Script
```bash
#!/bin/bash
# pre_release_check.sh

set -e

echo "=== Pre-Release Checks ==="

# 1. Build native
echo "1. Building native..."
cargo build --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon --lib --tests

# 2. Run tests
echo "2. Running tests..."
DASH_STATE_DIR=/tmp/test_state target/.../dash_essential_backend --test-threads=1

# 3. Android target check
echo "3. Checking Android target..."
cargo check --release --target aarch64-linux-android -p dash-essential-daemon --tests

# 4. Release sync
echo "4. Release sync..."
./tools/test-release-sync.sh

# 5. Build module (CI)
echo "5. Module build (requires CI)..."
# ./tools/build-kernelsu-next-module.sh

echo "=== All checks passed ==="
```

## Device-Specific Commands (MT6991 dash)

```bash
# GPU info
adb shell su -c 'cat /proc/gpufreqv2/gpufreq_status'
adb shell su -c 'cat /proc/gpufreqv2/limit_table'

# Thermal
adb shell su -c 'cat /sys/class/thermal/thermal_zone*/temp'

# Touch panel
adb shell su -c 'cat /proc/xm_htc_report_rate'
adb shell su -c 'cat /proc/tp_fw_version'

# Display
adb shell su -c 'ls /sys/class/display/'
adb shell su -c 'cat /sys/class/drm/card0/device/display_modes'

# KernelSU
adb shell su -c 'ksud --version'
adb shell su -c 'ls /data/adb/modules/nees_dash_essential_backend/'

# Daemon state
adb shell su -c 'cat /data/adb/dash-essential/state.conf'
adb shell su -c 'cat /data/adb/dash-essential/service.json'
```