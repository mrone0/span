package app.span.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

public final class SpanBootReceiver extends BroadcastReceiver {
    private static final String TAG = "SpanBootReceiver";

    @Override public void onReceive(Context context, Intent intent) {
        String action = intent == null ? null : intent.getAction();
        boolean shouldRestore = Intent.ACTION_BOOT_COMPLETED.equals(action)
                || Intent.ACTION_MY_PACKAGE_REPLACED.equals(action)
                || "android.intent.action.QUICKBOOT_POWERON".equals(action);
        if (!shouldRestore) return;
        SpanStore store = new SpanStore(context);
        if (!store.isReceiverEnabled()) return;
        try {
            SpanReceiveService.start(context);
        } catch (RuntimeException error) {
            // A refused foreground-service start at boot must not crash the
            // receiver process. The accessibility watchdog retries as soon as
            // the user unlocks the phone.
            Log.w(TAG, "Could not restore the LAN receiver after " + action, error);
        }
    }
}
