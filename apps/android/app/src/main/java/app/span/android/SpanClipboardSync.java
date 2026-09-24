package app.span.android;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.util.Log;
import java.nio.charset.StandardCharsets;

/** Coordinates local clipboard sends across the activity and foreground service. */
final class SpanClipboardSync {
    private static final String TAG = "SpanClipboardSync";
    private static final String PREFS = "span";
    private static final String PENDING_REMOTE_TEXT = "clipboard.pending_remote_text";
    private static final Object LOCK = new Object();
    private static final Object WRITE_LOCK = new Object();
    private static final long DUPLICATE_WINDOW_MILLIS = 1500;
    private static final long REMOTE_ECHO_WINDOW_MILLIS = 5000;

    private static String inFlightText;
    private static String lastSentText;
    private static long lastSentAtMillis;
    private static String remoteText;
    private static long remoteTextUntilMillis;
    private static String pendingRemoteText;

    private SpanClipboardSync() {}

    static int sendCurrentClipboard(Context context) throws Exception {
        return sendCapturedClipboard(context, captureCurrentClipboard(context));
    }

    static String captureCurrentClipboard(Context context) {
        return readClipboardText(context);
    }

    static int sendCapturedClipboard(Context context, String text) throws Exception {
        return sendText(context, text, false);
    }

    static int sendSharedText(Context context, String text) throws Exception {
        return sendText(context, text, true);
    }

    static void markRemoteClipboard(Context context, String text) {
        Context app = context.getApplicationContext();
        synchronized (LOCK) {
            remoteText = text;
            pendingRemoteText = text;
            remoteTextUntilMillis = System.currentTimeMillis() + REMOTE_ECHO_WINDOW_MILLIS;
            // Keep the in-memory and persisted pending value in the same order.
            // Otherwise a successful older write can race a newly received value
            // and remove that newer value from SharedPreferences.
            app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .edit()
                    .putString(PENDING_REMOTE_TEXT, text)
                    .apply();
        }
    }

    static boolean writePendingRemoteClipboard(Context context) {
        // The receiver service, accessibility watchdog and Activity can all try
        // a deferred write. Serialize them so a second caller cannot replay an
        // item after the first caller has already delivered and cleared it.
        synchronized (WRITE_LOCK) {
            Context app = context.getApplicationContext();
            String text;
            synchronized (LOCK) {
                text = pendingRemoteText;
                if (text == null) {
                    text = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                            .getString(PENDING_REMOTE_TEXT, null);
                    if (text != null) pendingRemoteText = text;
                }
            }
            if (text == null || text.isEmpty()) return false;

            ClipboardManager clipboard =
                    (ClipboardManager) context.getSystemService(Context.CLIPBOARD_SERVICE);
            if (clipboard == null) return false;
            try {
                clipboard.setPrimaryClip(ClipData.newPlainText("Span", text));
                // Android 10+ can silently ignore a background clipboard write:
                // setPrimaryClip may return normally even though the value did
                // not change. Only clear the durable pending item after a focused
                // Activity/accessibility overlay can read the same value back.
                if (!clipboard.hasPrimaryClip()) return false;
                ClipData written = clipboard.getPrimaryClip();
                if (written == null || written.getItemCount() == 0) return false;
                CharSequence verified = written.getItemAt(0).coerceToText(context);
                if (verified == null || !text.contentEquals(verified)) return false;
            } catch (RuntimeException error) {
                // Keep the persisted value so a focused Activity or accessibility
                // overlay can retry when Android permits the call.
                Log.w(TAG, "System clipboard write deferred from "
                        + context.getClass().getSimpleName(), error);
                return false;
            }

            boolean cleared = false;
            synchronized (LOCK) {
                if (text.equals(pendingRemoteText)) {
                    pendingRemoteText = null;
                    app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                            .edit()
                            .remove(PENDING_REMOTE_TEXT)
                            .apply();
                    // Renew echo suppression when a deferred write finally
                    // succeeds; otherwise foreground wake-up would immediately
                    // send it back. Do not replace the marker for a newer item
                    // that arrived while this platform call was in flight.
                    remoteText = text;
                    remoteTextUntilMillis =
                            System.currentTimeMillis() + REMOTE_ECHO_WINDOW_MILLIS;
                    cleared = true;
                }
            }
            return cleared;
        }
    }

    static boolean hasPendingRemoteClipboard(Context context) {
        synchronized (LOCK) {
            if (pendingRemoteText != null && !pendingRemoteText.isEmpty()) return true;
            String persisted = context.getApplicationContext()
                    .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .getString(PENDING_REMOTE_TEXT, null);
            if (persisted == null || persisted.isEmpty()) return false;
            pendingRemoteText = persisted;
            return true;
        }
    }


    private static int sendText(Context context, String text, boolean explicitShare) throws Exception {
        if (text == null || text.trim().isEmpty()) return 0;
        if (text.getBytes(StandardCharsets.UTF_8).length > SpanProtocol.MAX_TEXT_BYTES) {
            throw new IllegalArgumentException("text too large");
        }
        long now = System.currentTimeMillis();
        synchronized (LOCK) {
            if (!explicitShare && remoteText != null && remoteText.equals(text)
                    && now <= remoteTextUntilMillis) {
                Log.d(TAG, "Skipped clipboard text received from a remote device");
                return 0;
            }
            if (inFlightText != null && inFlightText.equals(text)) return 0;
            if (lastSentText != null && lastSentText.equals(text)
                    && now - lastSentAtMillis <= DUPLICATE_WINDOW_MILLIS) {
                return 0;
            }
            inFlightText = text;
        }

        int sent = 0;
        try {
            sent = new SpanDispatcher(context).sendText(text);
            return sent;
        } finally {
            synchronized (LOCK) {
                if (text.equals(inFlightText)) inFlightText = null;
                if (sent > 0) {
                    lastSentText = text;
                    lastSentAtMillis = System.currentTimeMillis();
                }
            }
        }
    }

    private static String readClipboardText(Context context) {
        Context app = context.getApplicationContext();
        ClipboardManager clipboard =
                (ClipboardManager) app.getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard == null || !clipboard.hasPrimaryClip()) return null;
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) return null;
        CharSequence text = clip.getItemAt(0).coerceToText(app);
        return text == null ? null : text.toString();
    }
}
