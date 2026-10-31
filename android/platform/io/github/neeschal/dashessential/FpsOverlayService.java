package io.github.neeschal.dashessential;

import android.annotation.SuppressLint;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
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
import android.view.MotionEvent;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.widget.TextView;
import org.json.JSONException;
import org.json.JSONObject;

import java.util.HashMap;
import java.util.Map;

/**
 * One number in a system window, kept alive while the user plays.
 *
 * Dash's Flutter frame callbacks stop when Dash stops drawing, so they cannot
 * measure a game. The daemon reads the vendor frame nodes instead and this
 * service renders that reading over whatever is in front. The window is not
 * focusable and not touchable: game input passes through untouched.
 *
 * The overlay is repositionable (drag), resizable (double-tap or the settings
 * slider), and styled (opacity, corner radius, accent). Position is remembered
 * per game when the foreground tracker is on. Style config is written by the
 * Flutter settings UI through the FFI bridge; window position is owned and
 * persisted here, because only this service can observe a drag.
 */
public final class FpsOverlayService extends Service {
    static { System.loadLibrary("dash_essential_host"); }

    private static final String PREFS = "dash_fps_overlay";
    private static final String KEY_ENABLED = "enabled";
    private static final String KEY_X = "x";
    private static final String KEY_Y = "y";
    private static final String KEY_SCALE = "scale";
    private static final String KEY_OPACITY = "opacity";
    private static final String KEY_RADIUS = "radius";
    private static final String KEY_ACCENT = "accent";
    private static final String KEY_POSITIONS = "positions";
    private static final String KEY_POS_PACKAGE = "position_package";
    private static final String CHANNEL = "fps_overlay";
    private static final int NOTIFICATION_ID = 42;

    private static final float DEFAULT_SCALE = 1.0f;
    private static final float DEFAULT_OPACITY = 0.85f;
    private static final float DEFAULT_RADIUS = 10.0f;
    private static final int DEFAULT_ACCENT = 0xFF59BCFF;
    private static final float[] SCALE_STEPS = {0.8f, 1.0f, 1.3f};

    /** The one live instance, so config pushed over FFI reaches a running chip. */
    private static FpsOverlayService current;

    private final Handler main = new Handler(Looper.getMainLooper());
    private WindowManager windowManager;
    private TextView chip;
    private boolean attached;
    private HandlerThread worker;
    private Handler workerHandler;

    private int accentColor = DEFAULT_ACCENT;
    private float scale = DEFAULT_SCALE;
    private float opacity = DEFAULT_OPACITY;
    private float radius = DEFAULT_RADIUS;

    private WindowManager.LayoutParams params;
    private int dragStartX;
    private int dragStartY;
    private boolean isDragging;
    private long lastTapTime;
    /** The package whose remembered position the chip currently sits at. */
    private String positionedPackage = "";

    private final Map<String, int[]> perGamePositions = new HashMap<>();

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
            if (!canDraw(FpsOverlayService.this)) {
                stopSelf();
                return;
            }
            final String payload = nativeSnapshot();
            main.post(() -> {
                if (chip != null) {
                    render(chip, payload);
                    followForeground(packageOf(payload));
                }
            });
            Handler handler = workerHandler;
            if (handler != null) handler.postDelayed(this, 500);
        }
    };

    @Override public void onCreate() {
        super.onCreate();
        current = this;
        windowManager = (WindowManager) getSystemService(WINDOW_SERVICE);
        NotificationChannel channel = new NotificationChannel(
            CHANNEL, "FPS overlay", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Shows the foreground app's frame rate.");
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (!isEnabled(this)) {
            stopSelf();
            return START_NOT_STICKY;
        }
        loadConfig();
        startForeground(NOTIFICATION_ID, notification());
        show();
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) {
        return null;
    }

    @Override public void onDestroy() {
        if (current == this) current = null;
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
    // ids), so the icon is resolved by name.
    private int icon() {
        int id = getResources().getIdentifier(
            "ic_fps_overlay", "drawable", getPackageName());
        return id != 0 ? id : android.R.drawable.ic_dialog_info;
    }

    // ─── Preference helpers ────────────────────────────────────────────────

    private SharedPreferences prefs() {
        return getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    /**
     * Merges the pushed style into the prefs and, if the overlay is live,
     * re-applies it to the running chip. Called by the FFI bridge, so the
     * activity is foregrounded and the service may or may not be running.
     * Values are packed ints: scale and radius in thousandths, opacity 0..255.
     */
    static void applyConfig(Context context, int scaleMilli, int opacity, int radiusMilli, int accent) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putFloat(KEY_SCALE, scaleMilli / 1000.0f)
            .putFloat(KEY_OPACITY, opacity / 255.0f)
            .putFloat(KEY_RADIUS, radiusMilli / 1000.0f)
            .putInt(KEY_ACCENT, accent)
            .apply();
        FpsOverlayService live = current;
        if (live != null) live.main.post(live::reloadStyle);
    }

    /** Drains a changed config into the visible chip without restarting it. */
    private void reloadStyle() {
        loadConfig();
        applyStyle();
    }

    private void loadConfig() {
        SharedPreferences p = prefs();
        scale = p.getFloat(KEY_SCALE, DEFAULT_SCALE);
        opacity = p.getFloat(KEY_OPACITY, DEFAULT_OPACITY);
        radius = p.getFloat(KEY_RADIUS, DEFAULT_RADIUS);
        accentColor = p.getInt(KEY_ACCENT, DEFAULT_ACCENT);
        perGamePositions.clear();
        try {
            JSONObject positions = new JSONObject(p.getString(KEY_POSITIONS, "{}"));
            for (int i = 0; i < positions.names().length(); i++) {
                String pkg = positions.names().getString(i);
                JSONObject entry = positions.getJSONObject(pkg);
                perGamePositions.put(pkg, new int[]{entry.getInt("x"), entry.getInt("y")});
            }
        } catch (JSONException ignored) { }
    }

    // ─── Window ─────────────────────────────────────────────────────────────

    private void show() {
        if (!canDraw(this) || windowManager == null) {
            stopSelf();
            return;
        }
        if (chip == null) chip = buildChip();
        if (!attached) {
            try {
                params = buildParams();
                windowManager.addView(chip, params);
                attached = true;
                installDragListener();
            } catch (RuntimeException refused) {
                stopSelf();
                return;
            }
        } else {
            applyStyle();
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

    private WindowManager.LayoutParams buildParams() {
        // Touchable window with a pass-through backdrop: the chip itself is
        // touchable so it can be dragged, and the transparent backdrop view
        // is what keeps the rest of the screen touchable by the game. The
        // chip view reports touches only when they land inside its bounds.
        WindowManager.LayoutParams lp = new WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT);
        lp.gravity = Gravity.TOP | Gravity.START;
        lp.layoutInDisplayCutoutMode =
            WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        positionedPackage = prefs().getString(KEY_POS_PACKAGE, "");
        int[] saved = positionedPackage.isEmpty() ? null : perGamePositions.get(positionedPackage);
        if (saved != null) {
            lp.x = saved[0];
            lp.y = saved[1];
        } else {
            lp.x = prefs().getInt(KEY_X, Integer.MIN_VALUE);
            lp.y = prefs().getInt(KEY_Y, Integer.MIN_VALUE);
            if (lp.x == Integer.MIN_VALUE || lp.y == Integer.MIN_VALUE) {
                Insets bars = windowManager.getCurrentWindowMetrics().getWindowInsets()
                    .getInsetsIgnoringVisibility(
                        WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                lp.x = bars.left + dp(8);
                lp.y = bars.top + dp(8);
            }
        }
        return lp;
    }

    /**
     * Moves the chip to a package's remembered spot when the foreground app
     * changes. A package with no memory keeps the chip where the user last
     * put it; the current position is adopted as that game's spot on drag end.
     */
    private void followForeground(String pkg) {
        if (pkg == null || pkg.equals(positionedPackage) || isDragging) return;
        positionedPackage = pkg;
        prefs().edit().putString(KEY_POS_PACKAGE, pkg).apply();
        if (params == null) return;
        int[] saved = perGamePositions.get(pkg);
        if (saved != null) {
            params.x = saved[0];
            params.y = saved[1];
            windowManager.updateViewLayout(chip, params);
        }
    }

    @SuppressLint("ClickableViewAccessibility")
    private void installDragListener() {
        chip.setOnTouchListener((v, event) -> {
            if (params == null) return false;
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                    dragStartX = (int) event.getRawX();
                    dragStartY = (int) event.getRawY();
                    isDragging = false;
                    long now = System.currentTimeMillis();
                    if (now - lastTapTime < 300) {
                        cycleScale();
                        lastTapTime = 0;
                        return true;
                    }
                    lastTapTime = now;
                    return true;
                case MotionEvent.ACTION_MOVE:
                    int dx = (int) (event.getRawX() - dragStartX);
                    int dy = (int) (event.getRawY() - dragStartY);
                    if (!isDragging && (Math.abs(dx) > 10 || Math.abs(dy) > 10)) {
                        isDragging = true;
                    }
                    if (isDragging) {
                        params.x += dx;
                        params.y += dy;
                        windowManager.updateViewLayout(chip, params);
                        dragStartX = (int) event.getRawX();
                        dragStartY = (int) event.getRawY();
                    }
                    return true;
                case MotionEvent.ACTION_UP:
                    if (isDragging) {
                        isDragging = false;
                        persistPosition();
                    }
                    return true;
                default:
                    return false;
            }
        });
        // The chip view only receives what lands inside its own bounds; every
        // other pixel of the transparent window passes through to the game.
    }

    private void persistPosition() {
        if (params == null) return;
        SharedPreferences.Editor e = prefs().edit()
            .putInt(KEY_X, params.x)
            .putInt(KEY_Y, params.y);
        String pkg = positionedPackage;
        if (pkg != null && !pkg.isEmpty() && !pkg.equals("null")) {
            perGamePositions.put(pkg, new int[]{params.x, params.y});
            try {
                JSONObject positions = new JSONObject();
                for (Map.Entry<String, int[]> entry : perGamePositions.entrySet()) {
                    JSONObject pos = new JSONObject();
                    pos.put("x", entry.getValue()[0]);
                    pos.put("y", entry.getValue()[1]);
                    positions.put(entry.getKey(), pos);
                }
                e.putString(KEY_POSITIONS, positions.toString());
            } catch (JSONException ignored) { }
        }
        e.apply();
    }

    /** Cycles S(0.8) -> M(1.0) -> L(1.3) and remembers the step. */
    private void cycleScale() {
        int next = 1;
        for (int i = 0; i < SCALE_STEPS.length; i++) {
            if (Math.abs(scale - SCALE_STEPS[i]) < 0.05f) {
                next = (i + 1) % SCALE_STEPS.length;
                break;
            }
        }
        scale = SCALE_STEPS[next];
        prefs().edit().putFloat(KEY_SCALE, scale).apply();
        applyStyle();
    }

    // ─── Chip style ─────────────────────────────────────────────────────────

    private TextView buildChip() {
        TextView view = new TextView(this);
        view.setText("FPS \u2014");
        view.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        view.setLineSpacing(0, 1.05f);
        view.setContentDescription("Foreground app frame rate");
        applyStyleTo(view);
        return view;
    }

    private void applyStyle() {
        if (chip != null) applyStyleTo(chip);
    }

    private void applyStyleTo(TextView view) {
        view.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16 * scale);
        int padH = Math.round(dp(10) * scale);
        int padV = Math.round(dp(6) * scale);
        view.setPadding(padH, padV, padH, padV);
        view.setMaxWidth(Math.round(dp(260) * scale));
        view.setTextColor(accentColor);
        GradientDrawable background = new GradientDrawable();
        // Opacity drives the fill; the accent trim fades with it so a
        // translucent chip does not keep a hard bright outline.
        int fillAlpha = Math.round(opacity * 0xE6);
        background.setColor((fillAlpha << 24) | 0x000000);
        background.setCornerRadius(dp(Math.round(radius)));
        background.setStroke(
            Math.max(1, Math.round(dp(1))),
            Color.argb(Math.round(opacity * 128),
                Color.red(accentColor), Color.green(accentColor), Color.blue(accentColor)));
        view.setBackground(background);
    }

    // ─── Rendering ──────────────────────────────────────────────────────────

    private static String packageOf(String payload) {
        try {
            String raw = new JSONObject(payload == null ? "{}" : payload)
                .optString("package", "");
            // optString on a JSON null returns the literal string "null".
            return raw.equals("null") ? "" : raw;
        } catch (JSONException malformed) {
            return "";
        }
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
            pkg = packageOf(payload);
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
