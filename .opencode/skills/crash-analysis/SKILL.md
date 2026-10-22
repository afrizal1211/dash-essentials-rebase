---
name: crash-analysis
description: Use when analyzing daemon crashes on device. Covers crash log format, register dump interpretation, backtrace reconstruction, common crash patterns on MT6991, and debugging strategies.
---

# Crash Analysis for dash-essential-daemon

## Crash Log Format (v1.18.16+)
```
DASH_CRASH signal=11
   0: 0x0000007f8a3c2a40
   1: 0x0000007f8a3c2b10
   ...
```
- `signal`: SIGSEGV=11, SIGBUS=7, SIGILL=4, SIGFPE=8, SIGABRT=6, SIGTRAP=5
- Frames: `pc` (program counter), `lr` (link register), `sp` (stack pointer) in hex
- Up to 16 frames captured
- Written via async-signal-safe `write()` to pre-opened FD

## Old Format (v1.18.15 and earlier)
```
DASH_CRASH signal=11
   0: <unknown>
   1: <unknown>
```
- `std::backtrace::Backtrace::force_capture()` in signal handler
- Stripped binaries → no symbols → `<unknown>`
- No register values → hard to locate fault

## Analyzing a Crash

### 1. Get the Binary
```bash
# From release asset
unzip Dash-Essential-KernelSU-Next-Magisk-v1.18.16.zip bin/dash_daemon
```

### 2. Resolve Addresses
```bash
# Using addr2line (needs debug symbols)
addr2line -e dash_daemon 0x0000007f8a3c2a40

# Or with objdump for full disassembly
objdump -d dash_daemon | grep -A5 -B5 "<address>"
```

### 3. Common Crash Patterns on MT6991 dash

| Pattern | Cause | Location |
|---------|-------|----------|
| `pc` in `libc.so` + `lr` in `libbinder_ndk.so` | Pending binder fault | `vendor_binder::guarded_call` child |
| `pc` in `read_node_text` / `write_verified` | Sysfs node I/O error | Touch/EB DVFS node write |
| `pc` in `std::backtrace` | Recursive fault in crash handler | Crash handler itself |
| `pc` in `libc` malloc/free | Heap corruption / double free | Any alloc path |
| `pc` in `dlopen` / `dlsym` | Missing libbinder_ndk.so | `vendor_binder` init |

### 4. Device-Specific Debugging

#### MT6991 GPU Crashes
- Check `gpu.cappedBy` in health JSON
- `THERMAL_AP` (id 4) → platform ceiling at 1430 MHz
- `THERMAL_EB` (id 5) → vendor EB DVFS policy
- `custom_boost_gpu_freq=51` = floor index (338 MHz), not a bug

#### Touch Node Crashes
- `/proc/xm_htc_report_rate` accepts 1-6 bytes only
- Rejects: "report_rate: 240", "0", "≥7 bytes"
- Returns partial reads: "report_rate: 43", "report_rate: 2"
- Solution: read 8×, require repeated plausible value (60-1000)

#### Display Vendor Calls
- `AIBinder_isAlive()` before `AIBinder_transact`
- Retry with `FLAG_PRIVATE_VENDOR` (0x1000_0000)
- Transaction code 7, flags 0 → fallback flags=0x1000_0000

## Debugging Workflow

### 1. Get Crash Log from Device
```bash
adb shell su -c 'cat /data/adb/dash-essential/crash.log'
# or from export
adb pull /sdcard/Download/Dash-Essential/dash-essential-report-*.txt
```

### 2. Check Health JSON
```bash
python3 /tmp/opencode/dash_query_tcp.py "GET health" | jq '.gpu.vendorEbDvfs, .display.route'
```

### 3. Reproduce in Native Test
```bash
# Add test case in lib.rs #[cfg(test)] module
cargo test --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon <test_name> -- --test-threads=1
```

### 4. Add Debug Logging
```rust
eprintln!("DEBUG: eb_dvfs_policy read: {:?}", raw_text);
eprintln!("DEBUG: touch node returned: {:?}", node_text);
```

## Tools
- `addr2line` (from NDK toolchain or host)
- `objdump -d` for disassembly
- `readelf -s` for symbol table
- `gdb` (if debugging on device with gdbserver)
- `python3 /tmp/opencode/dash_query_tcp.py` for live health

## Crash Prevention Checklist
- [ ] All vendor calls in `guarded_call`
- [ ] `AIBinder_isAlive()` before every transaction
- [ ] Crash log FD opened at startup (before any fault)
- [ ] No alloc in signal handler (use `write()` only)
- [ ] Stack size sufficient for deep call chains (64KB default OK)
- [ ] No unwind across FFI boundary (use `catch_unwind` at FFI edges)