package io.github.neeschal.dashessential;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.PixelFormat;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.os.Looper;
import android.provider.Settings;
import android.text.Spannable;
import android.text.SpannableStringBuilder;
import android.text.style.ForegroundColorSpan;
import android.text.style.RelativeSizeSpan;
import android.text.style.StyleSpan;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.widget.TextView;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * One number in a system window, kept alive while the user plays.
 *
 * Dash's Flutter frame callbacks stop when Dash stops drawing, so they cannot
 * measure a game. The daemon reads the vendor frame nodes instead and this
 * service renders that reading over whatever is in front. The window is not
 * focusable and not touchable: game input passes through untouched.
 */
public final class FpsOverlayService extends Service {
    static { System.loadLibrary("dash_essential_host"); }

    private static final String PREFS = "dash_fps_overlay";
    private static final String KEY_ENABLED = "enabled";
    private static final String CHANNEL = "fps_overlay";
    private static final int NOTIFICATION_ID = 42;
    private static final int ACCENT = 0xFF59BCFF;

    private final Handler main = new Handler(Looper.getMainLooper());
    private WindowManager windowManager;
    private TextView chip;
    private boolean attached;
    private HandlerThread worker;
    private Handler workerHandler;

    /**
     * Writes the preference and starts or stops the service. Returns 0 when
     * the user asked for the overlay without granting the window permission,
     * so the switch can stay off instead of lying.
     */
    static int setEnabled(Context context, boolean enabled) {
        if (enabled && !canDraw(context)) return 0;
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putBoolean(KEY_ENABLED, enabled).apply();
        Intent intent = new Intent(context, FpsOverlayService.class);
        if (enabled) context.startForegroundService(intent);
        else context.stopService(intent);
        return 1;
    }

    static boolean isEnabled(Context context) {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getBoolean(KEY_ENABLED, false);
    }

    static boolean canDraw(Context context) {
        return Settings.canDrawOverlays(context);
    }

    /** One daemon snapshot: the frame reading, the sensors, the package. */
    private static native String nativeSnapshot();

    private static final int SPAN = Spannable.SPAN_EXCLUSIVE_EXCLUSIVE;

    private final Runnable sample = new Runnable() {
        @Override public void run() {
            // Revoking the permission in system settings removes the window
            // but leaves the service; drop the notification instead of
            // running a meter nobody can see.
            if (!canDraw(FpsOverlayService.this)) {
                stopSelf();
                return;
            }
            final String payload = nativeSnapshot();
            main.post(() -> {
                if (chip != null) render(chip, payload);
            });
            Handler handler = workerHandler;
            if (handler != null) handler.postDelayed(this, 500);
        }
    };

    @Override public void onCreate() {
        super.onCreate();
        windowManager = (WindowManager) getSystemService(WINDOW_SERVICE);
        NotificationChannel channel = new NotificationChannel(
            CHANNEL, "FPS overlay", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Shows the foreground app's frame rate.");
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        // A sticky restart carries no intent. The preference decides whether
        // the overlay comes back, so a disabled overlay never resurrects.
        if (!isEnabled(this)) {
            stopSelf();
            return START_NOT_STICKY;
        }
        startForeground(NOTIFICATION_ID, notification());
        show();
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) {
        return null;
    }

    @Override public void onDestroy() {
        hide();
        main.removeCallbacksAndMessages(null);
        super.onDestroy();
    }

    private Notification notification() {
        Intent open = new Intent(this, DashActivity.class)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent tap = PendingIntent.getActivity(
            this, 0, open, PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, CHANNEL)
            .setSmallIcon(icon())
            .setContentTitle("FPS overlay")
            .setContentText("Foreground app frame rate")
            .setContentIntent(tap)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build();
    }

    // The platform dex is compiled without an R class (aapt2 owns resource
    // ids), so the icon is resolved by name. The framework icon is a last
    // resort that still posts a valid notification.
    private int icon() {
        int id = getResources().getIdentifier(
            "ic_fps_overlay", "drawable", getPackageName());
        return id != 0 ? id : android.R.drawable.ic_dialog_info;
    }

    private void show() {
        // A permission revoked in settings must not leave a dead service
        // holding a notification.
        if (!canDraw(this) || windowManager == null) {
            stopSelf();
            return;
        }
        if (chip == null) chip = buildChip();
        if (!attached) {
            try {
                windowManager.addView(chip, params());
                attached = true;
            } catch (RuntimeException refused) {
                stopSelf();
                return;
            }
        }
        if (worker == null) {
            worker = new HandlerThread("fps-overlay");
            worker.start();
            workerHandler = new Handler(worker.getLooper());
            workerHandler.post(sample);
        }
    }

    private void hide() {
        if (worker != null) {
            worker.quitSafely();
            worker = null;
            workerHandler = null;
        }
        if (attached && chip != null && windowManager != null) {
            try {
                windowManager.removeView(chip);
            } catch (IllegalArgumentException alreadyGone) {
                // The window manager can drop the view first on process death.
            }
            attached = false;
        }
    }

    private TextView buildChip() {
        TextView view = new TextView(this);
        view.setText("FPS \u2014");
        view.setTextColor(ACCENT);
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        view.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        view.setPadding(dp(10), dp(6), dp(10), dp(6));
        view.setLineSpacing(0, 1.05f);
        view.setMaxWidth(dp(260));
        view.setContentDescription("Foreground app frame rate");
        GradientDrawable background = new GradientDrawable();
        background.setColor(0xE6000000);
        background.setCornerRadius(dp(10));
        background.setStroke(dp(1), Color.argb(128, 89, 188, 255));
        view.setBackground(background);
        return view;
    }

    private WindowManager.LayoutParams params() {
        WindowManager.LayoutParams params = new WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                | WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT);
        params.gravity = Gravity.TOP | Gravity.START;
        Insets bars = windowManager.getCurrentWindowMetrics().getWindowInsets()
            .getInsetsIgnoringVisibility(
                WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
        params.x = bars.left + dp(8);
        params.y = bars.top + dp(8);
        return params;
    }

    /**
     * Draws one snapshot into the chip. The reading is the headline; the
     * sensors and the package are smaller supporting lines. A field the ROM
     * does not expose is omitted rather than printed as a fake zero.
     */
    private static void render(TextView view, String payload) {
        long scaled = 0;
        String pkg = "";
        int gpu = 0;
        int gpuMhz = 0;
        int cpuMhz = 0;
        int tempDeciC = 0;
        boolean hasGpu = false;
        boolean hasGpuMhz = false;
        boolean hasCpuMhz = false;
        boolean hasTemp = false;
        try {
            JSONObject snapshot = new JSONObject(payload == null ? "{}" : payload);
            scaled = Math.round(snapshot.optDouble("fps", 0.0) * 100.0);
            pkg = snapshot.optString("package", "");
            hasGpu = snapshot.has("gpuUtil");
            hasGpuMhz = snapshot.has("gpuMHz");
            hasCpuMhz = snapshot.has("cpuMHz");
            hasTemp = snapshot.has("tempDeciC");
            gpu = snapshot.optInt("gpuUtil", 0);
            gpuMhz = snapshot.optInt("gpuMHz", 0);
            cpuMhz = snapshot.optInt("cpuMHz", 0);
            tempDeciC = snapshot.optInt("tempDeciC", 0);
        } catch (JSONException malformed) {
            // A daemon that is away answers "{}"; the chip shows a dash.
        }

        SpannableStringBuilder text = new SpannableStringBuilder();
        int start = text.length();
        text.append(scaled > 0
            ? Math.round(scaled / 100.0) + " FPS" : "FPS \u2014");
        text.setSpan(new RelativeSizeSpan(1.35f), start, text.length(), SPAN);
        text.setSpan(new StyleSpan(Typeface.BOLD), start, text.length(), SPAN);
        if (hasTemp && tempDeciC > 0) {
            text.append("  ");
            int tempStart = text.length();
            text.append(Math.round(tempDeciC / 10.0) + "\u00b0C");
            text.setSpan(new RelativeSizeSpan(0.8f), tempStart, text.length(), SPAN);
        }
        if ((hasGpu && gpu > 0) || (hasGpuMhz && gpuMhz > 0)
                || (hasCpuMhz && cpuMhz > 0)) {
            text.append('\n');
            int detailStart = text.length();
            if (hasGpu) text.append("GPU ").append(Integer.toString(gpu)).append('%');
            if (hasGpuMhz && gpuMhz > 0) {
                if (text.length() > detailStart) text.append(' ');
                text.append(Integer.toString(gpuMhz)).append(" MHz");
            }
            if (hasCpuMhz && cpuMhz > 0) {
                if (text.length() > detailStart) text.append(" \u00b7");
                text.append(" CPU ").append(Integer.toString(cpuMhz)).append(" MHz");
            }
            text.setSpan(new RelativeSizeSpan(0.68f), detailStart, text.length(), SPAN);
            text.setSpan(
                new ForegroundColorSpan(0xFFA8B8C4), detailStart, text.length(), SPAN);
        }
        if (!pkg.isEmpty()) {
            text.append('\n');
            int pkgStart = text.length();
            text.append(pkg);
            text.setSpan(new RelativeSizeSpan(0.62f), pkgStart, text.length(), SPAN);
            text.setSpan(
                new ForegroundColorSpan(0xFF7E8E9A), pkgStart, text.length(), SPAN);
        }
        view.setText(text);
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
