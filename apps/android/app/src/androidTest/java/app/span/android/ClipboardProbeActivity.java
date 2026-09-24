package app.span.android;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ResultReceiver;
import android.util.Log;

/** Foreground activity in the separate test APK that simulates a paste target. */
public final class ClipboardProbeActivity extends Activity {
    private static final String TAG = "SpanClipboardProbe";
    static final String EXTRA_RESULT_RECEIVER = "clipboard_result_receiver";
    static final String EXTRA_VALUE = "value";
    static final String EXTRA_FINISH_ON_VALUE = "finish_on_value";
    static final String EXTRA_FINISH_NOW = "finish_now";
    static final String EXTRA_SET_VALUE = "set_value";
    static final int RESULT_CLIPBOARD = 1;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private ResultReceiver resultReceiver;
    private String finishOnValue;
    private String lastReportedValue;
    private final Runnable poll = new Runnable() {
        @Override public void run() {
            readClipboard();
            handler.postDelayed(this, 50);
        }
    };

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        applyIntent(getIntent());
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        applyIntent(intent);
    }

    private void applyIntent(Intent intent) {
        if (intent.getBooleanExtra(EXTRA_FINISH_NOW, false)) {
            finish();
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            resultReceiver = intent.getParcelableExtra(
                    EXTRA_RESULT_RECEIVER, ResultReceiver.class);
        } else {
            //noinspection deprecation
            resultReceiver = intent.getParcelableExtra(EXTRA_RESULT_RECEIVER);
        }
        finishOnValue = intent.getStringExtra(EXTRA_FINISH_ON_VALUE);
        String setValue = intent.getStringExtra(EXTRA_SET_VALUE);
        if (setValue != null) {
            ClipboardManager clipboard =
                    (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
            if (clipboard != null) {
                clipboard.setPrimaryClip(ClipData.newPlainText("clipboard test", setValue));
            }
        }
    }

    @Override protected void onResume() {
        super.onResume();
        handler.removeCallbacks(poll);
        handler.post(poll);
    }

    @Override protected void onPause() {
        handler.removeCallbacks(poll);
        super.onPause();
    }

    private void readClipboard() {
        ClipboardManager clipboard =
                (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard == null || !clipboard.hasPrimaryClip()) return;
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) return;
        CharSequence value = clip.getItemAt(0).coerceToText(this);
        if (value == null) return;
        String current = value.toString();
        if (!current.equals(lastReportedValue)) {
            // This Activity only exists in the test APK. The log lets an
            // external adb driver verify the production accessibility service
            // without instrumenting (and therefore force-stopping) that service.
            Log.i(TAG, "clipboard=" + current);
            lastReportedValue = current;
        }
        if (resultReceiver == null) return;
        Bundle result = new Bundle();
        result.putString(EXTRA_VALUE, current);
        try {
            resultReceiver.send(RESULT_CLIPBOARD, result);
            if (current.equals(finishOnValue)) finish();
        } catch (RuntimeException ignored) {
            // The instrumentation process may finish before this foreground
            // probe is paused. That does not affect the assertion it reported.
        }
    }
}
