# Backup "says done but nothing backed up" — diagnosis (2026-11-21)

Symptom (user): pressing Backup reports success, but no original config was
backed up (a later restore would give defaults).

## Evidence gathered on the device (Redmi dash, Android 17, app v1.23.16)

- `io.github.neeschal.dashessential` v1.23.16 (12316) is installed; the packed
  `classes.dex` matches the repo's `DashActivity.java` (jadx decompile ==
  source, only javac/jadx cosmetic differences). `libapp.so` contains the
  export strings ("Backup saved", "GET save.bundle") and version 1.23.16.
- `GET save.bundle` answered live on `@dash_essentiald_v15` with a real bundle:
  48 state lines (perf=2, gpu=powersave 338–598, zram 12288, display_color=1,
  expert_gamut=2 …) and 1 per-app profile (`com.kurogame.wutheringwaves.global`).
  It matches `/data/adb/dash-essential/state.conf` exactly. The daemon's export
  data is NOT empty.
- No `dash-essential-backup-*.json` exists anywhere on `/sdcard` or
  `/data/media/0` (find by name and by content grep).
- MediaProvider `external.db` (copied + read): no row named
  `dash-essential-backup-%` ever existed; `deleted_media` has no matching
  recent deletion; the `log` table is empty. No trace of an insert.
- The report file `dash-essential-report-*.txt` (written by the daemon with a
  direct root `fs::write` to `/sdcard/Download/Dash-Essential`) DID land, and
  root writes to `/sdcard/Download` work.
- Controlled MediaStore probe (insert pending → write bytes at the
  `.pending-<epoch>-<name>` path → `IS_PENDING=0` update by item URI):
  MediaProvider renamed the file to `Download/dashprobe3.json` and it appeared
  on disk. The `writeMedia` path works on this device, so a completed export
  would have left a visible file and a DB row.
- The UI's success sheet only shows when `_pollBackupState` returns `1`, which
  is set only by `nativeBackupResult(1, uri)` from `DashActivity.saveBackup`
  after a successful MediaStore insert. So the app really did complete the
  write at least once.

## Conclusion

The export pipeline works and really did run to completion; the produced
`dash-essential-backup-*.json` is now missing from Downloads. Most likely the
file was created once and later removed (MIUI cleanmaster is installed and
aggressively prunes Downloads; manual cleaning also possible), or the user
looked in a different directory/app. There is no code path that can report
"Backup saved" without a successful MediaStore insert, and there is no DB trace
of a deletion by MediaProvider itself.

## Hardening options (not yet implemented)

- Write the backup under `Download/Dash-Essential/` (like the diagnostics
  report) so folder-based cleaners and the user's own searches find it.
- Show the produced file path in the success sheet (it is already available in
  `uri`), so "done" is verifiable.
- Consider a `no_backup`/`exclude_from_cleaner` hint via `.nomedia` is wrong
  here (it hides from media scans); instead rely on the stable subfolder.

## Probe leftovers

MediaProvider DB rows 28366/28367/28368/28376 are pending probe rows owned by
no package (inserted via root `content insert`); root cannot delete them
("UID 0 has no associated package"). Harmless: pending rows with no file. They
age out with MediaProvider's pending-timeout sweep.
