package app.span.android;

import android.accessibilityservice.AccessibilityButtonController;
import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.AccessibilityServiceInfo;
import android.content.Context;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.widget.FrameLayout;
import android.widget.Toast;
import java.lang.ref.WeakReference;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;

/**
 * Optional system-bound watchdog for vendors that kill normal foreground services.
 *
 * It reads no screen content and performs no gestures. Android keeps the
 * service binding after the user enables it; that binding is used to restore
 * Span's tiny LAN receiver and retry a deferred PC clipboard write when the
 * user switches into another app to paste.
 */
public final class SpanKeepAliveService extends AccessibilityService {
    private static final long HEARTBEAT_MILLIS = 60_000;
    private static final long EVENT_RETRY_DEBOUNCE_MILLIS = 500;
    private static final long CLIPBOARD_FOCUS_TIMEOUT_MILLIS = 1_200;
    private enum ClipboardFocusOperation { NONE, SEND, WRITE_PENDING }

    private static WeakReference<SpanKeepAliveService> activeService = new WeakReference<>(null);
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService clipboardWorker = Executors.newSingleThreadExecutor();
    // These fields are only touched on the accessibility service's main thread.
    // A single invisible focus window serializes reads and writes so two quick
    // events cannot steal focus from each other or launch an Activity.
    private ClipboardFocusOperation clipboardFocusOperation = ClipboardFocusOperation.NONE;
    private boolean clipboardFocusStarted;
    private boolean clipboardSendQueued;
    private boolean clipboardWriteQueued;
    private long lastEventRetryMillis;
    private AccessibilityButtonController accessibilityButtonController;
    private WindowManager windowManager;
    private View clipboardFocusWindow;
    private final AccessibilityButtonController.AccessibilityButtonCallback accessibilityButtonCallback =
            new AccessibilityButtonController.AccessibilityButtonCallback() {
                @Override public void onClicked(AccessibilityButtonController controller) {
                    sendClipboardWithoutActivity();
                }
            };
    private final Runnable clipboardFocusTimeout = () -> {
        if (clipboardFocusOperation == ClipboardFocusOperation.NONE || clipboardFocusStarted) return;
        ClipboardFocusOperation failed = clipboardFocusOperation;
        completeClipboardFocusOperation();
        if (failed == ClipboardFocusOperation.SEND) {
            Toast.makeText(this, "系统未允许读取剪贴板，请使用分享菜单发送", Toast.LENGTH_SHORT).show();
        }
    };
    private final Runnable heartbeat = new Runnable() {
        @Override public void run() {
            ensureReceiver();
            writePendingClipboardWithoutActivity();
            handler.postDelayed(this, HEARTBEAT_MILLIS);
        }
    };

    @Override protected void onServiceConnected() {
        super.onServiceConnected();
        activeService = new WeakReference<>(this);
        accessibilityButtonController = getAccessibilityButtonController();
        accessibilityButtonController.registerAccessibilityButtonCallback(
                accessibilityButtonCallback, handler);
        handler.removeCallbacks(heartbeat);
        ensureReceiver();
        writePendingClipboardWithoutActivity();
        if (SpanReceiveService.isRunning()) SpanReceiveService.start(this);
        handler.postDelayed(heartbeat, HEARTBEAT_MILLIS);
    }

    @Override public void onAccessibilityEvent(AccessibilityEvent event) {
        // Do not inspect event contents. A foreground app switch is enough to
        // retry a pending PC clipboard write before the user long-presses Paste.
        int type = event == null ? 0 : event.getEventType();
        if (type != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED
                && type != AccessibilityEvent.TYPE_WINDOWS_CHANGED) {
            return;
        }
        long now = System.currentTimeMillis();
        if (now - lastEventRetryMillis < EVENT_RETRY_DEBOUNCE_MILLIS) return;
        lastEventRetryMillis = now;
        ensureReceiver();
        writePendingClipboardWithoutActivity();
    }

    @Override public void onInterrupt() {
        ensureReceiver();
    }

    @Override public void onDestroy() {
        handler.removeCallbacks(heartbeat);
        handler.removeCallbacks(clipboardFocusTimeout);
        removeClipboardFocusWindow();
        clipboardWorker.shutdownNow();
        clipboardFocusOperation = ClipboardFocusOperation.NONE;
        clipboardFocusStarted = false;
        clipboardSendQueued = false;
        clipboardWriteQueued = false;
        if (accessibilityButtonController != null) {
            accessibilityButtonController.unregisterAccessibilityButtonCallback(
                    accessibilityButtonCallback);
            accessibilityButtonController = null;
        }
        SpanKeepAliveService current = activeService.get();
        if (current == this) activeService = new WeakReference<>(null);
        if (SpanReceiveService.isRunning()) SpanReceiveService.start(this);
        super.onDestroy();
    }

    /**
     * Runs a user-requested clipboard send without bringing Span's Activity to
     * the foreground. Android 10+ only exposes clipboard contents to the UID
     * owning the focused window, so the accessibility service briefly owns a
     * transparent 1x1 accessibility overlay while it reads the clipboard.
     */
    private void sendClipboardWithoutActivity() {
        clipboardSendQueued = true;
        runNextClipboardFocusOperation();
    }

    private void writePendingClipboardWithoutActivity() {
        if (!SpanClipboardSync.hasPendingRemoteClipboard(this)) return;
        clipboardWriteQueued = true;
        runNextClipboardFocusOperation();
    }

    private void runNextClipboardFocusOperation() {
        if (clipboardFocusOperation != ClipboardFocusOperation.NONE) return;
        ClipboardFocusOperation next;
        if (clipboardSendQueued) {
            clipboardSendQueued = false;
            next = ClipboardFocusOperation.SEND;
        } else if (clipboardWriteQueued) {
            clipboardWriteQueued = false;
            next = ClipboardFocusOperation.WRITE_PENDING;
        } else {
            return;
        }
        beginClipboardFocusOperation(next);
    }

    private void beginClipboardFocusOperation(ClipboardFocusOperation operation) {
        ensureReceiver();

        windowManager = (WindowManager) getSystemService(WINDOW_SERVICE);
        if (windowManager == null) {
            if (operation == ClipboardFocusOperation.SEND) {
                Toast.makeText(this, "系统未提供无障碍浮层，请使用分享菜单发送", Toast.LENGTH_SHORT).show();
            }
            runNextClipboardFocusOperation();
            return;
        }

        clipboardFocusOperation = operation;
        clipboardFocusStarted = false;

        FrameLayout focusWindow = new FrameLayout(this) {
            @Override public void onWindowFocusChanged(boolean hasFocus) {
                super.onWindowFocusChanged(hasFocus);
                if (hasFocus) beginFocusedClipboardOperation();
            }
        };
        focusWindow.setFocusable(true);
        focusWindow.setFocusableInTouchMode(true);
        WindowManager.LayoutParams params = new WindowManager.LayoutParams(
                1,
                1,
                WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
                        | WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                        | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                PixelFormat.TRANSLUCENT);
        params.gravity = Gravity.TOP | Gravity.START;
        params.alpha = 0.01f;
        params.setTitle("Span clipboard send");

        try {
            clipboardFocusWindow = focusWindow;
            windowManager.addView(focusWindow, params);
            focusWindow.requestFocus();
            handler.postDelayed(clipboardFocusTimeout, CLIPBOARD_FOCUS_TIMEOUT_MILLIS);
        } catch (RuntimeException error) {
            removeClipboardFocusWindow();
            clipboardFocusOperation = ClipboardFocusOperation.NONE;
            clipboardFocusStarted = false;
            if (operation == ClipboardFocusOperation.SEND) {
                Toast.makeText(this, "系统阻止了无障碍浮层，请使用分享菜单发送", Toast.LENGTH_SHORT).show();
            }
            runNextClipboardFocusOperation();
        }
    }

    private void beginFocusedClipboardOperation() {
        if (clipboardFocusOperation == ClipboardFocusOperation.NONE || clipboardFocusStarted) return;
        clipboardFocusStarted = true;
        handler.removeCallbacks(clipboardFocusTimeout);
        if (clipboardFocusOperation == ClipboardFocusOperation.WRITE_PENDING) {
            SpanClipboardSync.writePendingRemoteClipboard(this);
            // Never detach a WindowManager view from inside its own
            // onWindowFocusChanged callback. API 35's ViewRootImpl continues to
            // inspect the view after returning and otherwise crashes the process.
            handler.post(this::completeClipboardFocusOperation);
            return;
        }

        String text;
        try {
            // Capture while this UID owns window focus, then immediately return
            // focus to the previous app before doing any network I/O.
            text = SpanClipboardSync.captureCurrentClipboard(this);
        } catch (RuntimeException error) {
            handler.post(() -> {
                completeClipboardFocusOperation();
                Toast.makeText(this, "系统未允许读取剪贴板，请使用分享菜单发送",
                        Toast.LENGTH_SHORT).show();
            });
            return;
        }
        handler.post(() -> {
            completeClipboardFocusOperation();
            try {
                clipboardWorker.execute(() -> {
                    try {
                        int sent = SpanClipboardSync.sendCapturedClipboard(this, text);
                        finishClipboardSend(sent == 0
                                ? "没有可信设备或剪贴板为空"
                                : "已发送到 " + sent + " 台设备");
                    } catch (Exception error) {
                        finishClipboardSend("发送失败，请检查设备是否在线");
                    }
                });
            } catch (RejectedExecutionException ignored) {
                // The accessibility service was destroyed after focus was
                // released; there is no active user request left to complete.
            }
        });
    }

    private void finishClipboardSend(String message) {
        handler.post(() -> Toast.makeText(this, message, Toast.LENGTH_SHORT).show());
    }

    private void completeClipboardFocusOperation() {
        removeClipboardFocusWindow();
        clipboardFocusOperation = ClipboardFocusOperation.NONE;
        clipboardFocusStarted = false;
        runNextClipboardFocusOperation();
    }

    private void removeClipboardFocusWindow() {
        handler.removeCallbacks(clipboardFocusTimeout);
        View window = clipboardFocusWindow;
        clipboardFocusWindow = null;
        if (window == null || windowManager == null) return;
        try {
            windowManager.removeViewImmediate(window);
        } catch (RuntimeException ignored) {
            // The system may already have detached the accessibility window.
        }
    }

    private void ensureReceiver() {
        if (!new SpanStore(this).isReceiverEnabled()) return;
        if (!SpanReceiveService.isRunning()) SpanReceiveService.start(this);
    }

    static boolean requestClipboardRetry() {
        SpanKeepAliveService service = activeService.get();
        if (service == null) return false;
        service.handler.post(service::writePendingClipboardWithoutActivity);
        return true;
    }

    static boolean requestClipboardSend() {
        SpanKeepAliveService service = activeService.get();
        if (service == null) return false;
        service.handler.post(service::sendClipboardWithoutActivity);
        return true;
    }

    static boolean isConnected() {
        return activeService.get() != null;
    }

    static boolean isEnabled(Context context) {
        AccessibilityManager manager =
                (AccessibilityManager) context.getSystemService(Context.ACCESSIBILITY_SERVICE);
        if (manager == null) return false;
        List<android.accessibilityservice.AccessibilityServiceInfo> enabled =
                manager.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK);
        String packageName = context.getPackageName();
        String className = SpanKeepAliveService.class.getName();
        for (android.accessibilityservice.AccessibilityServiceInfo info : enabled) {
            if (info.getResolveInfo() == null || info.getResolveInfo().serviceInfo == null) continue;
            android.content.pm.ServiceInfo service = info.getResolveInfo().serviceInfo;
            if (packageName.equals(service.packageName) && className.equals(service.name)) return true;
        }
        return false;
    }
}
