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
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;
import java.util.Locale;

/** Own-layout status notification (no Prism mirror): screen on/off, deep sleep, battery drain,
    live power. Deep sleep and the drain rate are computed by the privileged
    daemon and read through the native bridge; screen buckets and the
    screen-off drain are measured in-process. No privileges required. */
public final class DashStatusNotificationService extends Service implements Runnable {
    static { System.loadLibrary("dash_essential_host"); }
    private static final String PREFS = "dash_status_notification";
    private static final String KEY_ENABLED = "enabled";
    private static final String CHANNEL = "dash_status";
    private static final int ID = 43;

    /** Lifetime deep-sleep total, -1 while the daemon has not answered. */
    private static native long nativeDeepSleepMs();
    private static native int nativeDeepSleepPct();
    private static native int nativeDrainPctPerHour();
    private static native long nativeBatteryPowerMw();
    private static native void nativeRequestRefresh();

    private static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    public static void setEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_ENABLED, enabled).apply();
        Intent intent = new Intent(context, DashStatusNotificationService.class);
        if (enabled) context.startForegroundService(intent);
        else context.stopService(intent);
    }

    static boolean isEnabled(Context context) {
        return prefs(context).getBoolean(KEY_ENABLED, false);
    }

    private boolean screenOn;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final BroadcastReceiver receiver = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            screenOn = Intent.ACTION_SCREEN_ON.equals(intent.getAction());
            accumulate();
            foldScreenOffDrain();
        }
    };

    @Override public void onCreate() {
        super.onCreate();
        NotificationChannel channel = new NotificationChannel(
            CHANNEL, "Dash status", NotificationManager.IMPORTANCE_LOW);
        channel.setShowBadge(false);
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
        screenOn = isInteractive();
        IntentFilter filter = new IntentFilter();
        filter.addAction(Intent.ACTION_SCREEN_ON);
        filter.addAction(Intent.ACTION_SCREEN_OFF);
        registerReceiver(receiver, filter);
    }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (!isEnabled(this)) { stopSelf(); return START_NOT_STICKY; }
        startForeground(ID, notification());
        handler.removeCallbacks(this);
        handler.postDelayed(this, 30_000);
        return START_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public void onDestroy() {
        try { unregisterReceiver(receiver); } catch (IllegalArgumentException gone) {}
        super.onDestroy();
    }

    /** 30 s tick: nudge the backend cache, fold the elapsed interval into the
        on/off bucket, repaint. The refresh lands within one tick, which is
        fine for a 30 s cadence. */
    @Override public void run() {
        try { nativeRequestRefresh(); } catch (UnsatisfiedLinkError unloaded) {}
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.notify(ID, notification());
        handler.postDelayed(this, 30_000);
    }

    private void accumulate() {
        SharedPreferences p = prefs(this);
        long now = SystemClock.elapsedRealtime();
        long mark = p.getLong("mark", now);
        SharedPreferences.Editor e = p.edit().putLong("mark", Math.max(mark, now));
        if (now > mark) e.putLong(screenOn ? "on" : "off", p.getLong(screenOn ? "on" : "off", 0) + now - mark);
        e.apply();
    }

    /** Kernel-manager screen-off drain: a level drop measured between
        ACTION_SCREEN_OFF and the next ACTION_SCREEN_ON, kept as the latest
        value so the notification can show what idle really costs. */
    private void foldScreenOffDrain() {
        SharedPreferences p = prefs(this);
        long now = SystemClock.elapsedRealtime();
        if (!screenOn) {
            p.edit().putInt("off_level", batteryLevel()).putLong("off_ms", now).apply();
            return;
        }
        int offLevel = p.getInt("off_level", -1);
        long span = now - p.getLong("off_ms", 0);
        int level = batteryLevel();
        if (offLevel > 0 && level > 0 && span >= 600_000 && offLevel > level) {
            p.edit().putFloat("off_drain", (offLevel - level) * 3_600_000f / span)
                .putLong("off_drain_ms", now).apply();
        }
        p.edit().remove("off_level").apply();
    }

    private boolean isInteractive() {
        PowerManager power = getSystemService(PowerManager.class);
        return power != null && power.isInteractive();
    }

    private int batteryLevel() {
        BatteryManager battery = getSystemService(BatteryManager.class);
        return battery == null ? 0 : battery.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY);
    }

    /** Charging state from the sticky battery intent; no root, no daemon. */
    private boolean chargingNow() {
        Intent battery = registerReceiver(null, new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
        if (battery == null) return false;
        int status = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1);
        int plugged = battery.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0);
        return status == BatteryManager.BATTERY_STATUS_CHARGING
            || status == BatteryManager.BATTERY_STATUS_FULL
            || plugged != 0;
    }

    private static String duration(long ms) {
        long minutes = ms / 60_000;
        long hours = minutes / 60;
        if (hours <= 0) return minutes + "m";
        return hours + "h " + (minutes % 60) + "m";
    }

    private Notification notification() {
        accumulate();
        SharedPreferences p = prefs(this);
        long onMs = p.getLong("on", 0);
        long offMs = p.getLong("off", 0);

        StringBuilder body = new StringBuilder()
            .append("Screen On: ").append(duration(onMs))
            .append("\nScreen Off: ").append(duration(offMs));

        // Deep sleep comes from the daemon: boottime minus monotonic, the
        // exact suspended time, even for a process that cannot read
        // /proc/uptime. Fall back to nothing rather than a fake zero.
        int deep = nativeDeepSleepPct();
        long deepMs = nativeDeepSleepMs();
        if (deep >= 0) {
            body.append("\nDeep sleep: ").append(deep).append('%');
            if (deepMs > 0) body.append(" · ").append(duration(deepMs)).append(" total");
            body.append("\nAwake: ").append(100 - deep).append('%');
        }

        // Battery line: level, discharge rate over the daemon's sample window,
        // and live |V·I| power, labelled by the framework charging state.
        // The rate uses the daemon's non-rising window; the state label comes
        // from the framework's sticky battery intent. The two can disagree
        // while a charge starts or stops between the daemon's last sample and
        // this paint. The label never overrides the number: a positive rate
        // shown next to "Charging" would be a lie, so the rate is dropped.
        int level = batteryLevel();
        boolean charging = chargingNow();
        StringBuilder battery = new StringBuilder("Battery: ").append(level).append('%');
        int drain = nativeDrainPctPerHour();
        if (drain >= 0 && (!charging || level == 100)) battery.append(" · ").append(drain).append("%/h");
        long powerMw = nativeBatteryPowerMw();
        if (powerMw > 0) {
            battery.append(" · ").append(charging ? "Charging " : "Using ")
                .append(String.format(Locale.US, "%.1fW", powerMw / 1_000.0));
        }
        body.append('\n').append(battery);

        // Last measured screen-off drain, when one exists.
        float offDrain = p.getFloat("off_drain", -1f);
        if (offDrain >= 0f) {
            body.append("\nScreen-off drain: ").append(String.format(Locale.US, "%.1f", offDrain)).append("%/h");
        }

        // Title carries the headline number so the collapsed row answers the
        // only question it can fit: what is the battery doing right now.
        String title = "Dash Essential";
        if (powerMw > 0) {
            title = "Dash Essential · " + String.format(Locale.US, "%.1fW", powerMw / 1_000.0);
        } else if (drain >= 0) {
            title = "Dash Essential · " + drain + "%/h";
        }

        // Compact line: the current state first, then the totals. Deep sleep
        // shows as a duration only, so the compact row stays one glance long.
        String compact = (charging ? "Charging" : "Using")
            + " " + level + "%"
            + " · On " + duration(onMs)
            + " · Off " + duration(offMs)
            + (deep >= 0 ? " · Deep " + duration(deepMs) : "");

        return new Notification.Builder(this, CHANNEL)
            .setSmallIcon(getResources().getIdentifier(
                "ic_fps_overlay", "drawable", getPackageName()))
            .setContentTitle(title)
            .setContentText(compact)
            .setStyle(new Notification.BigTextStyle().bigText(body.toString()))
            .setContentIntent(PendingIntent.getActivity(this, 1,
                new Intent(this, DashActivity.class)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP)
                    .putExtra("dash_open_fps_record", true),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build();
    }
}
