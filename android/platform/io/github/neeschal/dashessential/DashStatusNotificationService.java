package io.github.neeschal.dashessential;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.os.BatteryManager;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.os.Looper;
import android.os.SystemClock;

import java.io.BufferedReader;
import java.io.FileReader;

/**
 * Prism-style status notification: screen on/off time, deep sleep share and
 * battery drain, updated every 30 s while Dash runs its foreground services.
 * Tapping it opens Dash's FPS recording screen.
 *
 * Every value is measured here, in the app process, from sources that need no
 * privilege: ACTION_SCREEN_ON/OFF broadcasts for the screen times, the gap
 * between elapsedRealtime (pauses in suspend) and /proc/uptime (does not) for
 * deep sleep, and BatteryManager properties for the drain.
 */
public final class DashStatusNotificationService extends Service {
    private static final String PREFS = "dash_status_notification";
    private static final String KEY_ENABLED = "enabled";
    private static final String KEY_ON_MS = "screen_on_ms";
    private static final String KEY_OFF_MS = "screen_off_ms";
    private static final String KEY_MARK_MS = "elapsed_mark_ms";
    private static final String KEY_UPTIME_MARK = "uptime_mark_s";
    private static final String KEY_LEVEL_MARK = "level_mark";
    private static final String KEY_LEVEL_MARK_MS = "level_mark_ms";
    private static final String CHANNEL = "dash_status";
    private static final int NOTIFICATION_ID = 43;
    private static final long TICK_MS = 30_000;

    /** The one live instance, so the toggle helpers reach a running service. */
    private static DashStatusNotificationService current;

    private final Handler main = new Handler(Looper.getMainLooper());
    private HandlerThread worker;
    private Handler workerHandler;
    private boolean screenOn;

    private final BroadcastReceiver screenReceiver = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            boolean on = Intent.ACTION_SCREEN_ON.equals(intent.getAction());
            if (on == screenOn) return;
            accumulate();
            screenOn = on;
            prefs().edit().putBoolean("screen_on", on).apply();
        }
    };

    public static void setEnabled(Context context, boolean enabled) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putBoolean(KEY_ENABLED, enabled).apply();
        Intent intent = new Intent(context, DashStatusNotificationService.class);
        if (enabled) context.startForegroundService(intent);
        else context.stopService(intent);
    }

    static boolean isEnabled(Context context) {
        return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getBoolean(KEY_ENABLED, false);
    }

    @Override public void onCreate() {
        super.onCreate();
        current = this;
        NotificationChannel channel = new NotificationChannel(
            CHANNEL, "Dash status", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Screen time, deep sleep and battery drain.");
        channel.setShowBadge(false);
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
        screenOn = prefs().getBoolean("screen_on", isInteractive());
        IntentFilter filter = new IntentFilter();
        filter.addAction(Intent.ACTION_SCREEN_ON);
        filter.addAction(Intent.ACTION_SCREEN_OFF);
        registerReceiver(screenReceiver, filter);
        markBattery();
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (!isEnabled(this)) {
            stopSelf();
            return START_NOT_STICKY;
        }
        startForeground(NOTIFICATION_ID, notification());
        if (worker == null) {
            worker = new HandlerThread("dash-status");
            worker.start();
            workerHandler = new Handler(worker.getLooper());
            workerHandler.post(tick);
        }
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) {
        return null;
    }

    @Override public void onDestroy() {
        if (current == this) current = null;
        accumulate();
        try {
            unregisterReceiver(screenReceiver);
        } catch (IllegalArgumentException alreadyGone) {
        }
        if (worker != null) {
            worker.quitSafely();
            worker = null;
            workerHandler = null;
        }
        super.onDestroy();
    }

    private final Runnable tick = new Runnable() {
        @Override public void run() {
            accumulate();
            main.post(() -> {
                if (current != null) {
                    NotificationManager manager = getSystemService(NotificationManager.class);
                    manager.notify(NOTIFICATION_ID, notification());
                }
            });
            Handler handler = workerHandler;
            if (handler != null) handler.postDelayed(this, TICK_MS);
        }
    };

    private boolean isInteractive() {
        android.os.PowerManager power = getSystemService(android.os.PowerManager.class);
        return power != null && power.isInteractive();
    }

    private SharedPreferences prefs() {
        return getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    /** Adds the time since the last mark to the on/off bucket that matches. */
    private void accumulate() {
        SharedPreferences p = prefs();
        long now = SystemClock.elapsedRealtime();
        long mark = p.getLong(KEY_MARK_MS, now);
        if (now > mark) {
            long delta = now - mark;
            String key = screenOn ? KEY_ON_MS : KEY_OFF_MS;
            p.edit().putLong(key, p.getLong(key, 0) + delta)
                .putLong(KEY_MARK_MS, now).apply();
        } else {
            p.edit().putLong(KEY_MARK_MS, now).apply();
        }
    }

    /** Anchors the drain measurement to the current level. */
    private void markBattery() {
        SharedPreferences p = prefs();
        p.edit().putInt(KEY_LEVEL_MARK, batteryLevel())
            .putLong(KEY_LEVEL_MARK_MS, SystemClock.elapsedRealtime())
            .apply();
    }

    private int batteryLevel() {
        BatteryManager battery = getSystemService(BatteryManager.class);
        if (battery == null) return 0;
        return battery.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY);
    }

    /** Deep sleep share over the whole uptime window, 0..100. */
    private static int deepSleepPercent() {
        long elapsed = SystemClock.elapsedRealtime();
        long uptime = uptimeSeconds() * 1000;
        if (elapsed <= 0 || uptime <= 0 || uptime >= elapsed) return 0;
        return Math.round((elapsed - uptime) * 100f / elapsed);
    }

    private static long uptimeSeconds() {
        try (BufferedReader reader = new BufferedReader(new FileReader("/proc/uptime"))) {
            String line = reader.readLine();
            if (line == null) return 0;
            int space = line.indexOf(' ');
            String seconds = space > 0 ? line.substring(0, space) : line;
            return (long) Double.parseDouble(seconds.trim());
        } catch (Exception unreadable) {
            return 0;
        }
    }

    private String formatDuration(long ms) {
        long minutes = ms / 60_000;
        long hours = minutes / 60;
        minutes %= 60;
        if (hours <= 0) return minutes + "m";
        return hours + "h " + String.format("%02dm", minutes);
    }

    private Notification notification() {
        Intent open = new Intent(this, DashActivity.class)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra("dash_open_fps_record", true);
        PendingIntent tap = PendingIntent.getActivity(
            this, 1, open,
            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);

        SharedPreferences p = prefs();
        long onMs = p.getLong(KEY_ON_MS, 0);
        long offMs = p.getLong(KEY_OFF_MS, 0);
        int level = batteryLevel();
        int levelMark = p.getInt(KEY_LEVEL_MARK, level);
        long levelMarkMs = p.getLong(KEY_LEVEL_MARK_MS, 0);
        String drain = "";
        long spanMs = SystemClock.elapsedRealtime() - levelMarkMs;
        // Charging resets the reference so the number means "on battery".
        if (levelMark != level && levelMark > 0 && spanMs > 10 * 60_000) {
            float perHour = (levelMark - level) * 3_600_000f / spanMs;
            drain = String.format(" · %.1f%%/h", perHour);
        }
        if (level != levelMark || spanMs > 60 * 60_000) markBattery();

        return new Notification.Builder(this, CHANNEL)
            .setSmallIcon(icon())
            .setContentTitle("Dash Essential")
            .setContentText(String.format(
                "Screen on %s · off %s · deep sleep %d%%",
                formatDuration(onMs), formatDuration(offMs), deepSleepPercent()))
            .setSubText("Battery " + level + "%" + drain)
            .setContentIntent(tap)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build();
    }

    private int icon() {
        int id = getResources().getIdentifier(
            "ic_fps_overlay", "drawable", getPackageName());
        return id != 0 ? id : android.R.drawable.ic_dialog_info;
    }
}
