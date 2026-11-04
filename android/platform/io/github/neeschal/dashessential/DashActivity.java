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

    @Override protected void onCreate(Bundle state) {
        current = this;
        super.onCreate(state);
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
                "fps_export.csv", "text/csv", "RedFox/FpsRecord/",
                csv.getBytes(StandardCharsets.UTF_8));
        } catch (Exception ignored) {
            // The PNG may still save even if the CSV does not.
        }
        try {
            if (png != null && png.length > 0) {
                long now = System.currentTimeMillis();
                pngPath = writeMedia(resolver,
                    MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                    "fps_graph_" + now + ".png", "image/png", "RedFox/FPSRecord/",
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
        try (OutputStream out = resolver.openOutputStream(uri)) {
            if (out == null) return null;
            out.write(data);
        }
        ContentValues release = new ContentValues();
        release.put(MediaStore.MediaColumns.IS_PENDING, 0);
        resolver.update(uri, release, null, null);
        return uri.toString();
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
