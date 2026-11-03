package io.github.neeschal.dashessential;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.drawable.Icon;
import android.os.Handler;
import android.os.Looper;
import android.service.quicksettings.Tile;
import android.service.quicksettings.TileService;

/** Quick Settings tile: one tap starts or stops an FPS recording session. */
public final class FpsRecordTileService extends TileService {
    static { System.loadLibrary("dash_essential_host"); }
    private final Handler main = new Handler(Looper.getMainLooper());
    private boolean listening;
    private int lastState = -1;
    private static native int nativeState();
    private static native boolean nativeToggle();

    @Override public void onStartListening() {
        super.onStartListening();
        listening = true;
        lastState = -1;
        publish();
    }
    @Override public void onStopListening() {
        listening = false;
        super.onStopListening();
    }
    @Override public void onClick() {
        super.onClick();
        if (isLocked()) { unlockAndRun(this::toggle); return; }
        toggle();
    }
    private void toggle() {
        if (!nativeToggle()) return;
        publish();
    }
    private void publish() {
        Tile tile = getQsTile();
        if (tile == null) return;
        int state = nativeState();
        if (state == lastState) return;
        boolean recording = (state & 1) != 0;
        boolean auto = (state & 2) != 0;
        tile.setIcon(recordIcon(recording));
        tile.setLabel(recording ? "FPS recording" : "Record FPS");
        tile.setSubtitle(auto ? "Auto-record armed" : "Tap to record the foreground app");
        tile.setState(recording ? Tile.STATE_ACTIVE : Tile.STATE_INACTIVE);
        tile.setContentDescription(recording
            ? "FPS recording active, tap to stop"
            : "Record FPS, tap to start");
        tile.updateTile();
        lastState = state;
    }

    private Icon recordIcon(boolean recording) {
        Bitmap bitmap = Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888);
        Canvas canvas = new Canvas(bitmap);
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        paint.setColor(recording ? Color.parseColor("#E53935") : Color.parseColor("#9E9E9E"));
        canvas.drawCircle(48, 48, 30, paint);
        return Icon.createWithBitmap(bitmap);
    }
}
