package io.github.neeschal.dashessential;

import android.Manifest;
import android.app.NativeActivity;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.MediaStore;
import android.provider.Settings;
import android.widget.Toast;
import android.window.BackEvent;
import android.window.OnBackAnimationCallback;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;

/** NEESCHAL: a platform adapter, not a second UI toolkit. */
public final class DashActivity extends NativeActivity {
    static { System.loadLibrary("dash_essential_host"); }
    private static DashActivity current;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final Runnable refresh = this::refreshBack;
    private OnBackInvokedCallback callback;
    private boolean registered;
    private boolean resumed;
    private static native boolean nativeInterceptBack();
    private static native void nativeBack(int phase, float progress, int edge);
    private static native void nativeOpenBypassSettings();
    private static native void nativeBackupResult(int code, byte[] data);
    // Activities that call FFI methods must capture the JNI activity reference
    // through the host bridge. Without this the export/overlay bridges see a
    // null activity and return 0 before any Java code runs (the "export could
    // not be saved" path).
    private static native void nativeInitActivity();
    private static native void nativeClearActivity();

    private static final int BACKUP_REQUEST = 0x100;
    private static final int BACKUP_LIMIT = 4 * 1024 * 1024;

    @Override protected void onCreate(Bundle state) {
        current = this;
        super.onCreate(state);
        // The host bridge needs the JNI activity reference before any exported
        // path (FPS export, overlay toggle, status notification) can reach Java.
        nativeInitActivity();
        if (Build.VERSION.SDK_INT >= 34) callback = new ProgressCallback();
        else if (Build.VERSION.SDK_INT >= 33) callback = () -> nativeBack(4, 1, 0);
        handleTileIntent(getIntent());
    }
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent); setIntent(intent); handleTileIntent(intent);
    }
    private void handleTileIntent(Intent intent) {
        if (intent != null && "android.service.quicksettings.action.QS_TILE_PREFERENCES".equals(intent.getAction()))
            nativeOpenBypassSettings();
        if (intent != null && intent.getBooleanExtra("dash_open_fps_record", false)) {
            // Consumed by the Dart side through the host bridge, which routes
            // to the FPS recording screen.
            consumeFpsRecordOpenRequest();
        }
    }
    /** Signals the Flutter UI to open the FPS recording screen. */
    public void consumeFpsRecordOpenRequest() {
        openFpsRecordFromNotification();
    }
    private static native void openFpsRecordFromNotification();
    // System overlay control. The service owns the window; the activity only
    // forwards the intent, so it can keep running after Dash is backgrounded.
    public int setFpsOverlay(boolean enabled) {
        int result = FpsOverlayService.setEnabled(this, enabled);
        if (result == 1 && enabled) requestOverlayNotificationPermission();
        return result;
    }
    /** Starts or stops the Prism-style status notification service. */
    public int setStatusNotification(boolean enabled) {
        DashStatusNotificationService.setEnabled(this, enabled);
        if (enabled) requestOverlayNotificationPermission();
        return 1;
    }
    // Android 13 hides foreground-service notifications when the user has not
    // allowed them. The overlay still runs, but the prompt keeps its stop path
    // visible in the shade instead of only in Task Manager.
    private void requestOverlayNotificationPermission() {
        if (Build.VERSION.SDK_INT < 33) return;
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                == PackageManager.PERMISSION_GRANTED) return;
        requestPermissions(new String[] { Manifest.permission.POST_NOTIFICATIONS }, 0);
    }
    /** Pushes overlay style config from native FFI. */
    public boolean configFpsOverlay(int scaleMilli, int opacity, int radiusMilli, int accent,
            int perGame) {
        FpsOverlayService.applyConfig(this, scaleMilli, opacity, radiusMilli, accent, perGame != 0);
        return true;
    }
    public boolean hasFpsOverlayPermission() {
        return FpsOverlayService.canDraw(this);
    }
    public void requestFpsOverlayPermission() {
        startActivity(new Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
            Uri.parse("package:" + getPackageName())));
    }
    /** Native export bridge: writes one FPS session's CSV and graph PNG to
        public storage via MediaStore, then shows a Prism-style toast. */
    public void saveFpsExport(String csv, byte[] png) {
        ContentResolver resolver = getContentResolver();
        String csvPath = null;
        String pngPath = null;
        try {
            csvPath = writeMedia(resolver,
                MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                "fps_export.csv", "text/csv", "Download/",
                csv.getBytes(StandardCharsets.UTF_8));
        } catch (Exception ignored) {
            // The PNG may still save even if the CSV does not.
        }
        try {
            if (png != null && png.length > 0) {
                long now = System.currentTimeMillis();
                pngPath = writeMedia(resolver,
                    MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                    "fps_graph_" + now + ".png", "image/png", "Download/",
                    png);
            }
        } catch (Exception ignored) {
        }
        String message;
        if (csvPath != null && pngPath != null) {
            message = "FPS record saved at " + pngPath + " and CSV at " + csvPath;
        } else if (csvPath != null) {
            message = "FPS data exported to CSV";
        } else if (pngPath != null) {
            message = "FPS record saved at " + pngPath;
        } else {
            message = "The export could not be saved.";
        }
        main.post(() -> Toast.makeText(this, message, Toast.LENGTH_LONG).show());
    }

    private String writeMedia(ContentResolver resolver, Uri collection,
            String displayName, String mimeType, String relativePath,
            byte[] data) throws Exception {
        ContentValues values = new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME, displayName);
        values.put(MediaStore.MediaColumns.MIME_TYPE, mimeType);
        values.put(MediaStore.MediaColumns.RELATIVE_PATH, relativePath);
        values.put(MediaStore.MediaColumns.IS_PENDING, 1);
        Uri uri = resolver.insert(collection, values);
        if (uri == null) return null;
        try {
            try (OutputStream out = resolver.openOutputStream(uri)) {
                if (out == null) return null;
                out.write(data);
            }
        } finally {
            // Always clear IS_PENDING so a failed write never leaves an
            // orphaned pending media entry visible in Downloads/Images.
            try {
                ContentValues release = new ContentValues();
                release.put(MediaStore.MediaColumns.IS_PENDING, 0);
                resolver.update(uri, release, null, null);
            } catch (Exception ignored) {}
        }
        return uri.toString();
    }

    /** Settings backup, written to Downloads the same way an FPS export is;
        the URI it produced comes back so the UI can offer a share action. */
    public void saveBackup(byte[] data) {
        try {
            String name = "dash-essential-backup-" + System.currentTimeMillis() + ".json";
            String uri = writeMedia(getContentResolver(),
                    MediaStore.Downloads.EXTERNAL_CONTENT_URI, name,
                    "application/json", "Download/", data);
            if (uri != null) {
                nativeBackupResult(1, uri.getBytes(StandardCharsets.UTF_8));
            } else {
                nativeBackupResult(2, null);
            }
        } catch (Exception error) {
            nativeBackupResult(2, null);
        }
    }

    /** Settings restore: the user picks the backup, the activity reads it. */
    public void openBackup() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("application/json");
            startActivityForResult(intent, BACKUP_REQUEST);
        });
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != BACKUP_REQUEST) return;
        if (result != RESULT_OK || data == null || data.getData() == null) {
            nativeBackupResult(5, null);
            return;
        }
        try (InputStream in = getContentResolver().openInputStream(data.getData());
                ByteArrayOutputStream buffer = new ByteArrayOutputStream()) {
            if (in == null) {
                nativeBackupResult(4, null);
                return;
            }
            byte[] block = new byte[8192];
            int read;
            while ((read = in.read(block)) > 0) {
                buffer.write(block, 0, read);
                // A real backup is a few kilobytes; anything past the limit is
                // not a bundle this UI should hand to the daemon.
                if (buffer.size() > BACKUP_LIMIT) {
                    nativeBackupResult(4, null);
                    return;
                }
            }
            nativeBackupResult(3, buffer.toByteArray());
        } catch (Exception error) {
            nativeBackupResult(4, null);
        }
    }

    /** Hands the exported backup to Drive or any other transport. */
    public void shareBackup(String uri) {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_SEND);
            intent.setType("application/json");
            intent.putExtra(Intent.EXTRA_STREAM, Uri.parse(uri));
            startActivity(Intent.createChooser(intent, "Share backup"));
        });
    }

    /** Whether an imported custom background still resolves on this device. */
    public boolean backupPathExists(String path) {
        if (path.startsWith("content://")) {
            return getContentResolver().getType(Uri.parse(path)) != null;
        }
        return new File(path).exists();
    }

    /** Called only when native navigation ownership changes; no polling loop. */
    public static void refreshBackFromNative() {
        DashActivity activity = current;
        if (activity != null) {
            activity.main.removeCallbacks(activity.refresh);
            activity.main.post(activity.refresh);
        }
    }
    private void refreshBack() {
        if (Build.VERSION.SDK_INT < 33 || callback == null) return;
        boolean wanted = resumed && nativeInterceptBack();
        if (wanted == registered) return;
        if (wanted) getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
            OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback);
        else getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(callback);
        registered = wanted;
    }
    @Override protected void onResume() { super.onResume(); resumed = true; refreshBack(); }
    @Override protected void onPause() {
        resumed = false; refreshBack(); nativeBack(3, 0, 0); super.onPause();
    }
    @Override protected void onDestroy() {
        main.removeCallbacks(refresh);
        if (current == this) current = null;
        nativeClearActivity();
        super.onDestroy();
    }
    // Isolated API-34 class: older devices never instantiate progress callbacks.
    private static final class ProgressCallback implements OnBackAnimationCallback {
        private int edge;
        @Override public void onBackStarted(BackEvent event) {
            edge = event.getSwipeEdge();
            nativeBack(1, event.getProgress(), event.getSwipeEdge());
        }
        @Override public void onBackProgressed(BackEvent event) {
            nativeBack(2, event.getProgress(), event.getSwipeEdge());
        }
        @Override public void onBackCancelled() { nativeBack(3, 0, edge); }
        @Override public void onBackInvoked() { nativeBack(4, 1, edge); }
    }
}
