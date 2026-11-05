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

import java.io.BufferedReader;
import java.io.FileReader;

/** Prism-style status notification: screen on/off, deep sleep, battery drain.
    Tapping opens the FPS recording screen. Measured in-process, no privileges. */
public final class DashStatusNotificationService extends Service implements Runnable {
    private static final String PREFS = "dash_status_notification";
    private static final String KEY_ENABLED = "enabled";
    private static final String CHANNEL = "dash_status";
    private static final int ID = 43;

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

    /** 30 s tick: fold the elapsed interval into the on/off bucket, repaint. */
    @Override public void run() {
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.notify(ID, notification());
        handler.postDelayed(this, 30_000);
    }

    /** Adds the time since the last mark to the bucket that matches. */
    private void accumulate() {
        SharedPreferences p = prefs(this);
        long now = SystemClock.elapsedRealtime();
        long mark = p.getLong("mark", now);
        SharedPreferences.Editor e = p.edit().putLong("mark", Math.max(mark, now));
        if (now > mark) e.putLong(screenOn ? "on" : "off", p.getLong(screenOn ? "on" : "off", 0) + now - mark);
        e.apply();
    }

    private boolean isInteractive() {
        PowerManager power = getSystemService(PowerManager.class);
        return power != null && power.isInteractive();
    }

    private int batteryLevel() {
        BatteryManager battery = getSystemService(BatteryManager.class);
        return battery == null ? 0 : battery.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY);
    }

    private static int deepSleepPercent() {
        try (BufferedReader reader = new BufferedReader(new FileReader("/proc/uptime"))) {
            String line = reader.readLine();
            long elapsed = SystemClock.elapsedRealtime();
            if (line == null || elapsed <= 0) return 0;
            int space = line.indexOf(' ');
            long uptime = (long) (Double.parseDouble(
                space > 0 ? line.substring(0, space) : line) * 1000);
            if (uptime <= 0 || uptime >= elapsed) return 0;
            return (int) Math.round((elapsed - uptime) * 100.0 / elapsed);
        } catch (Exception unreadable) {
            return 0;
        }
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
        int level = batteryLevel();
        int levelMark = p.getInt("lmark", level);
        long levelMarkMs = p.getLong("lmark_ms", 0);
        long span = SystemClock.elapsedRealtime() - levelMarkMs;
        String drain = "";
        if (levelMark != level && levelMark > 0 && span > 600_000) {
            drain = String.format(" · %.1f%%/h", (levelMark - level) * 3_600_000f / span);
        }
        if (level != levelMark || span > 3_600_000) {
            p.edit().putInt("lmark", level).putLong("lmark_ms", SystemClock.elapsedRealtime()).apply();
        }
        int deep = deepSleepPercent();
        String body = String.format(
            "Screen On: %s\nScreen Off: %s\nDeep sleep: %d%%\nAwake: %d%%",
            duration(onMs), duration(offMs), deep, 100 - deep);
        if (!drain.isEmpty()) body += String.format("\nBattery: %d%%%s", level, drain);
        return new Notification.Builder(this, CHANNEL)
            .setSmallIcon(getResources().getIdentifier(
                "ic_fps_overlay", "drawable", getPackageName()))
            .setContentTitle("Dash Essential")
            .setContentText(body.replace("\n", " · "))
            .setStyle(new Notification.BigTextStyle().bigText(body))
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
