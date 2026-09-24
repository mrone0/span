package app.span.android;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.provider.Settings;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Span Android connection hub: one primary action, explicit trust, quiet setup guidance. */
public final class MainActivity extends Activity {
    private static final int BLUE = Color.rgb(35, 108, 238);
    private static final int BLUE_SOFT = Color.rgb(235, 243, 255);
    private static final int GREEN = Color.rgb(28, 151, 86);
    private static final int GREEN_SOFT = Color.rgb(232, 247, 239);
    private static final int ORANGE = Color.rgb(197, 112, 16);
    private static final int ORANGE_SOFT = Color.rgb(255, 246, 229);
    private static final int TEXT = Color.rgb(27, 31, 38);
    private static final int TEXT_MUTED = Color.rgb(105, 113, 126);
    private static final int SURFACE = Color.WHITE;
    private static final int PAGE = Color.rgb(247, 248, 251);
    private static final int DIVIDER = Color.rgb(231, 234, 239);

    private SpanStore store;
    private LocalIdentity identity;
    private final ExecutorService worker = Executors.newCachedThreadPool();
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final Runnable deviceRefresh = new Runnable() {
        @Override public void run() {
            if (isFinishing()) return;
            refreshDevices();
            mainHandler.postDelayed(this, 1000);
        }
    };

    private TextView connectionTitle;
    private TextView connectionDetail;
    private TextView connectionCount;
    private TextView remoteDeviceName;
    private TextView remoteDeviceKind;
    private TextView activityMessage;
    private LinearLayout nearbySection;
    private LinearLayout nearbyList;
    private LinearLayout trustedList;
    private LinearLayout setupCard;
    private TextView setupTitle;
    private TextView setupDetail;
    private Button setupButton;
    private Button sendButton;
    private Button discoverButton;

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        store = new SpanStore(this);
        try {
            identity = store.loadOrCreateIdentity();
        } catch (Exception error) {
            throw new RuntimeException(error);
        }
        buildUi();
        store.setReceiverEnabled(true);
        SpanReceiveService.start(this);
        refreshDevices();
        handleLaunchIntent(getIntent());
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleLaunchIntent(intent);
    }

    @Override protected void onResume() {
        super.onResume();
        updateBackgroundSetup();
        refreshDevices();
        mainHandler.removeCallbacks(deviceRefresh);
        mainHandler.postDelayed(deviceRefresh, 500);
        if (!isFinishing()) SpanClipboardSync.writePendingRemoteClipboard(this);
    }

    @Override protected void onPause() {
        mainHandler.removeCallbacks(deviceRefresh);
        super.onPause();
    }

    @Override public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus || isFinishing()) return;
        // A focused Activity may read back and verify a deferred remote write.
        // Never treat merely opening Span as permission to transmit the user's
        // current clipboard; sending remains an explicit button/share/tile action.
        SpanClipboardSync.writePendingRemoteClipboard(this);
    }

    @Override protected void onDestroy() {
        mainHandler.removeCallbacksAndMessages(null);
        worker.shutdownNow();
        super.onDestroy();
    }

    private void buildUi() {
        getWindow().setStatusBarColor(PAGE);
        getWindow().setNavigationBarColor(PAGE);
        getWindow().getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR);

        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setClipToPadding(false);
        scroll.setBackgroundColor(PAGE);
        LinearLayout root = column();
        root.setPadding(dp(20), dp(16), dp(20), dp(36));
        scroll.addView(root, matchWrap());

        root.addView(buildHeader());
        root.addView(buildConnectionHero(), topMargin(dp(20)));
        root.addView(buildPrimaryAction(), topMargin(dp(16)));

        activityMessage = text("内容仅在可信设备间加密传输", 12, TEXT_MUTED, Typeface.NORMAL);
        activityMessage.setGravity(Gravity.CENTER);
        root.addView(activityMessage, topMargin(dp(12)));

        nearbySection = column();
        TextView nearbyTitle = sectionTitle("附近的新设备");
        nearbySection.addView(nearbyTitle);
        nearbyList = column();
        nearbySection.addView(nearbyList);
        root.addView(nearbySection, topMargin(dp(24)));

        LinearLayout trustedHead = row();
        TextView trustedTitle = sectionTitle("已连接设备");
        trustedHead.addView(trustedTitle, new LinearLayout.LayoutParams(0, dp(28), 1));
        discoverButton = quietButton("重新发现");
        discoverButton.setOnClickListener(v -> discoverNearby());
        trustedHead.addView(discoverButton, new LinearLayout.LayoutParams(dp(92), dp(34)));
        root.addView(trustedHead, topMargin(dp(22)));
        trustedList = column();
        root.addView(trustedList, topMargin(dp(8)));

        setupCard = buildSetupCard();
        root.addView(setupCard, topMargin(dp(22)));

        TextView privacy = text("局域网加密传输 · 不经过云端", 11, TEXT_MUTED, Typeface.NORMAL);
        privacy.setGravity(Gravity.CENTER);
        root.addView(privacy, topMargin(dp(24)));

        setContentView(scroll);
    }

    private View buildHeader() {
        LinearLayout header = row();
        ImageView logo = new ImageView(this);
        logo.setImageResource(R.drawable.ic_span);
        logo.setScaleType(ImageView.ScaleType.CENTER_CROP);
        header.addView(logo, new LinearLayout.LayoutParams(dp(32), dp(32)));

        LinearLayout copy = column();
        copy.setPadding(dp(12), 0, 0, 0);
        copy.addView(text("Span", 18, TEXT, Typeface.BOLD));
        copy.addView(text("跨设备剪贴板", 12, TEXT_MUTED, Typeface.NORMAL));
        header.addView(copy, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        TextView live = pill("●  接收中", GREEN, GREEN_SOFT);
        header.addView(live);
        return header;
    }

    private View buildConnectionHero() {
        LinearLayout hero = card(SURFACE, 24, dp(20));

        LinearLayout heading = row();
        LinearLayout copy = column();
        connectionTitle = text("正在查找设备", 21, TEXT, Typeface.BOLD);
        connectionDetail = text("确保电脑与手机连接同一 Wi-Fi", 13, TEXT_MUTED, Typeface.NORMAL);
        connectionDetail.setPadding(0, dp(5), 0, 0);
        copy.addView(connectionTitle);
        copy.addView(connectionDetail);
        heading.addView(copy, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        connectionCount = text("0", 34, BLUE, Typeface.BOLD);
        connectionCount.setGravity(Gravity.CENTER);
        heading.addView(connectionCount, new LinearLayout.LayoutParams(dp(52), dp(52)));
        hero.addView(heading);

        LinearLayout route = row();
        route.setPadding(0, dp(22), 0, dp(2));
        route.addView(deviceNode("手机", safeName(identity.name), true),
                new LinearLayout.LayoutParams(0, dp(94), 1));
        TextView arrows = text("⇄", 26, BLUE, Typeface.NORMAL);
        arrows.setGravity(Gravity.CENTER);
        route.addView(arrows, new LinearLayout.LayoutParams(dp(54), dp(94)));
        View remoteNode = deviceNode("其他设备", "等待连接", false);
        route.addView(remoteNode, new LinearLayout.LayoutParams(0, dp(94), 1));
        hero.addView(route);
        return hero;
    }

    private View deviceNode(String kind, String name, boolean local) {
        LinearLayout node = column();
        node.setGravity(Gravity.CENTER);
        node.setPadding(dp(8), dp(12), dp(8), dp(12));
        node.setBackground(rounded(local ? BLUE_SOFT : PAGE, local ? BLUE_SOFT : DIVIDER, 18));
        TextView icon = text(local ? "▯" : "+", 25, local ? BLUE : TEXT_MUTED, Typeface.BOLD);
        icon.setGravity(Gravity.CENTER);
        node.addView(icon, new LinearLayout.LayoutParams(dp(34), dp(34)));
        TextView nameView = text(name, 13, TEXT, Typeface.BOLD);
        nameView.setGravity(Gravity.CENTER);
        nameView.setSingleLine(true);
        node.addView(nameView);
        TextView kindView = text(kind, 10, TEXT_MUTED, Typeface.NORMAL);
        kindView.setGravity(Gravity.CENTER);
        node.addView(kindView);
        if (!local) {
            remoteDeviceName = nameView;
            remoteDeviceKind = kindView;
        }
        return node;
    }

    private View buildPrimaryAction() {
        LinearLayout actions = column();
        sendButton = primaryButton("发送当前剪贴板");
        sendButton.setOnClickListener(v -> sendCurrentClipboard());
        actions.addView(sendButton, matchHeight(dp(54)));
        TextView hint = text("也可以从系统分享菜单或快捷设置发送", 11, TEXT_MUTED, Typeface.NORMAL);
        hint.setGravity(Gravity.CENTER);
        actions.addView(hint, topMargin(dp(8)));
        return actions;
    }

    private LinearLayout buildSetupCard() {
        LinearLayout card = card(ORANGE_SOFT, 18, dp(16));
        LinearLayout head = row();
        TextView icon = text("!", 15, ORANGE, Typeface.BOLD);
        icon.setGravity(Gravity.CENTER);
        icon.setBackground(rounded(Color.WHITE, Color.WHITE, 20));
        head.addView(icon, new LinearLayout.LayoutParams(dp(32), dp(32)));
        LinearLayout copy = column();
        copy.setPadding(dp(12), 0, dp(8), 0);
        setupTitle = text("完善后台接收", 15, TEXT, Typeface.BOLD);
        setupDetail = text("避免锁屏后被系统中断", 12, TEXT_MUTED, Typeface.NORMAL);
        copy.addView(setupTitle);
        copy.addView(setupDetail);
        head.addView(copy, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        setupButton = quietButton("设置");
        setupButton.setOnClickListener(v -> configureReliableBackground());
        head.addView(setupButton, new LinearLayout.LayoutParams(dp(72), dp(38)));
        card.addView(head);
        return card;
    }

    private void discoverNearby() {
        discoverButton.setEnabled(false);
        discoverButton.setText("查找中…");
        SpanReceiveService.discover(this);
        showActivity("正在查找同一局域网内的设备…");
        mainHandler.postDelayed(() -> {
            if (isFinishing()) return;
            discoverButton.setEnabled(true);
            discoverButton.setText("重新发现");
            refreshDevices();
        }, 1800);
    }

    private void refreshDevices() {
        if (trustedList == null || nearbyList == null) return;
        List<SpanDevice> devices = store.loadDevices();
        List<SpanDevice> trusted = new ArrayList<>();
        List<SpanDevice> nearby = new ArrayList<>();
        for (SpanDevice device : devices) {
            if (device.trusted) trusted.add(device);
            else if (System.currentTimeMillis() - device.lastSeenMillis < 30_000) nearby.add(device);
        }

        int count = trusted.size();
        connectionCount.setText(String.valueOf(count));
        connectionTitle.setText(count == 0 ? "连接你的电脑" : count == 1 ? "已连接 1 台设备" : "已连接 " + count + " 台设备");
        connectionDetail.setText(count == 0 ? "发现并信任设备后即可开始同步" : "复制的文本可以在这些设备间流转");
        if (remoteDeviceName != null && remoteDeviceKind != null) {
            remoteDeviceName.setText(count == 0 ? "等待连接" : safeName(trusted.get(0).name));
            remoteDeviceKind.setText(count <= 1 ? "其他设备" : "其他设备 · 另有 " + (count - 1) + " 台");
        }

        trustedList.removeAllViews();
        if (trusted.isEmpty()) {
            trustedList.addView(emptyRow("还没有可信设备", "点击“重新发现”，或在电脑端打开 Span"));
        } else {
            for (SpanDevice device : trusted) trustedList.addView(trustedRow(device), bottomMargin(dp(8)));
        }

        nearbyList.removeAllViews();
        nearbySection.setVisibility(nearby.isEmpty() ? View.GONE : View.VISIBLE);
        for (SpanDevice device : nearby) nearbyList.addView(nearbyRow(device), bottomMargin(dp(8)));
    }

    private View nearbyRow(SpanDevice device) {
        LinearLayout row = card(BLUE_SOFT, 18, dp(14));
        LinearLayout content = row();
        LinearLayout copy = column();
        copy.addView(text(safeName(device.name), 15, TEXT, Typeface.BOLD));
        copy.addView(text(platformName(device.platform) + " · 请求连接", 11, TEXT_MUTED, Typeface.NORMAL));
        content.addView(copy, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        Button trust = smallPrimaryButton("信任");
        trust.setOnClickListener(v -> {
            if (!store.setTrusted(device.id, true)) {
                showActivity("设备状态已变化，请重新发现");
                refreshDevices();
                return;
            }
            showActivity("已连接 " + safeName(device.name));
            refreshDevices();
            worker.execute(() -> notifyPeerPairingAccepted(device));
        });
        content.addView(trust, new LinearLayout.LayoutParams(dp(74), dp(40)));
        row.addView(content);
        return row;
    }

    private void notifyPeerPairingAccepted(SpanDevice device) {
        for (int attempt = 0; attempt < 3; attempt++) {
            try {
                new SpanTransport().sendPairingAccept(identity, device);
                runOnUiThread(() -> showActivity(
                        "已与 " + safeName(device.name) + " 完成双向信任"));
                return;
            } catch (Exception error) {
                if (attempt < 2) {
                    try {
                        Thread.sleep(250L * (attempt + 1));
                    } catch (InterruptedException interrupted) {
                        Thread.currentThread().interrupt();
                        return;
                    }
                }
            }
        }
        // Local trust remains explicit and valid even when the reciprocal
        // confirmation cannot reach a desktop that just went offline.
        runOnUiThread(() -> showActivity("已在手机信任；请确认电脑端 Span 在线"));
    }

    private View trustedRow(SpanDevice device) {
        LinearLayout card = card(SURFACE, 18, dp(14));
        LinearLayout row = row();
        TextView icon = text(platformIcon(device.platform), 20, BLUE, Typeface.BOLD);
        icon.setGravity(Gravity.CENTER);
        icon.setBackground(rounded(BLUE_SOFT, BLUE_SOFT, 14));
        row.addView(icon, new LinearLayout.LayoutParams(dp(44), dp(44)));

        LinearLayout copy = column();
        copy.setPadding(dp(12), 0, 0, 0);
        copy.addView(text(safeName(device.name), 15, TEXT, Typeface.BOLD));
        String state = device.host == null || device.host.isEmpty() ? "已信任 · 等待上线" : "已信任 · " + device.host;
        copy.addView(text(state, 11, TEXT_MUTED, Typeface.NORMAL));
        row.addView(copy, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        Button more = quietButton("移除");
        more.setTextColor(TEXT_MUTED);
        more.setOnClickListener(v -> confirmRemove(device));
        row.addView(more, new LinearLayout.LayoutParams(dp(66), dp(38)));
        card.addView(row);
        return card;
    }

    private void confirmRemove(SpanDevice device) {
        new AlertDialog.Builder(this)
                .setTitle("断开 " + safeName(device.name) + "？")
                .setMessage("断开后，这台设备将无法收发你的剪贴板内容。")
                .setNegativeButton("取消", null)
                .setPositiveButton("断开", (dialog, which) -> {
                    store.setTrusted(device.id, false);
                    showActivity("已断开 " + safeName(device.name));
                    refreshDevices();
                })
                .show();
    }

    private View emptyRow(String title, String detail) {
        LinearLayout empty = card(SURFACE, 18, dp(16));
        empty.addView(text(title, 14, TEXT, Typeface.BOLD));
        TextView hint = text(detail, 12, TEXT_MUTED, Typeface.NORMAL);
        hint.setPadding(0, dp(4), 0, 0);
        empty.addView(hint);
        return empty;
    }

    private void configureReliableBackground() {
        if (android.os.Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 1001);
            showActivity("允许通知后，Span 才能持续显示接收状态");
            return;
        }
        if (!SpanKeepAliveService.isEnabled(this)) {
            showActivity("请在“已下载的应用”中开启 Span");
            startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS));
            return;
        }
        PowerManager power = (PowerManager) getSystemService(POWER_SERVICE);
        if (power != null && !power.isIgnoringBatteryOptimizations(getPackageName())) {
            showActivity("请允许 Span 在后台持续运行");
            startActivity(new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS)
                    .setData(Uri.parse("package:" + getPackageName())));
            return;
        }
        SpanReceiveService.start(this);
        showActivity("后台接收已就绪");
        updateBackgroundSetup();
    }

    private void updateBackgroundSetup() {
        if (setupCard == null) return;
        boolean notificationReady = android.os.Build.VERSION.SDK_INT < 33
                || checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED;
        boolean accessibility = SpanKeepAliveService.isEnabled(this);
        PowerManager power = (PowerManager) getSystemService(POWER_SERVICE);
        boolean battery = power != null && power.isIgnoringBatteryOptimizations(getPackageName());
        boolean ready = notificationReady && accessibility && battery;
        setupCard.setVisibility(ready ? View.GONE : View.VISIBLE);
        if (!notificationReady) {
            setupTitle.setText("允许后台通知");
            setupDetail.setText("用于显示接收服务状态");
            setupButton.setText("允许");
        } else if (!accessibility) {
            setupTitle.setText("开启可靠后台");
            setupDetail.setText("锁屏和切换应用后继续接收");
            setupButton.setText("开启");
        } else {
            setupTitle.setText("允许后台运行");
            setupDetail.setText("避免省电策略中断同步");
            setupButton.setText("允许");
        }
    }

    void sendCurrentClipboard() {
        sendButton.setEnabled(false);
        sendButton.setText("正在发送…");
        worker.execute(() -> {
            try {
                int sent = SpanClipboardSync.sendCurrentClipboard(this);
                runOnUiThread(() -> {
                    restoreSendButton();
                    if (sent > 0) {
                        showActivity("已发送到 " + sent + " 台设备");
                    } else {
                        showActivity("请先复制文本，并连接至少一台设备");
                    }
                });
            } catch (SecurityException error) {
                runOnUiThread(() -> sendFailed("系统暂时不允许读取剪贴板"));
            } catch (Exception error) {
                runOnUiThread(() -> sendFailed("发送失败，请检查设备是否在线"));
            }
        });
    }

    private void restoreSendButton() {
        sendButton.setEnabled(true);
        sendButton.setText("发送当前剪贴板");
    }

    private void sendFailed(String detail) {
        restoreSendButton();
        showActivity(detail);
        Toast.makeText(this, detail, Toast.LENGTH_SHORT).show();
    }

    private void handleLaunchIntent(Intent intent) {
        if (intent == null) return;
        if (Intent.ACTION_SEND.equals(intent.getAction()) && "text/plain".equals(intent.getType())) {
            String shared = intent.getStringExtra(Intent.EXTRA_TEXT);
            if (shared != null && !shared.trim().isEmpty()) sendText(shared, true);
        }
    }

    private void sendText(String value, boolean finishAfter) {
        worker.execute(() -> {
            try {
                int sent = SpanClipboardSync.sendSharedText(this, value);
                runOnUiThread(() -> {
                    showActivity(sent == 0 ? "请先连接可信设备" : "已发送到 " + sent + " 台设备");
                    if (finishAfter) {
                        Toast.makeText(this, sent == 0 ? "没有可信设备" : "已通过 Span 发送", Toast.LENGTH_SHORT).show();
                        finish();
                    }
                });
            } catch (Exception error) {
                runOnUiThread(() -> {
                    showActivity("发送失败，请检查网络和设备状态");
                    if (finishAfter) Toast.makeText(this, "发送失败", Toast.LENGTH_SHORT).show();
                });
            }
        });
    }

    private void showActivity(String message) {
        if (activityMessage != null) activityMessage.setText(message);
    }

    private LinearLayout card(int color, int radius, int padding) {
        LinearLayout view = column();
        view.setPadding(padding, padding, padding, padding);
        view.setBackground(rounded(color, color == SURFACE ? DIVIDER : color, radius));
        if (color == SURFACE) view.setElevation(dp(1));
        return view;
    }

    private TextView sectionTitle(String value) {
        return text(value, 16, TEXT, Typeface.BOLD);
    }

    private TextView pill(String value, int foreground, int background) {
        TextView view = text(value, 11, foreground, Typeface.BOLD);
        view.setGravity(Gravity.CENTER);
        view.setPadding(dp(10), dp(6), dp(10), dp(6));
        view.setBackground(rounded(background, background, 20));
        return view;
    }

    private Button primaryButton(String label) {
        Button button = button(label);
        tintButton(button, BLUE, Color.WHITE, BLUE);
        return button;
    }

    private Button smallPrimaryButton(String label) {
        Button button = button(label);
        button.setTextSize(13);
        tintButton(button, BLUE, Color.WHITE, BLUE);
        return button;
    }

    private Button quietButton(String label) {
        Button button = button(label);
        button.setTextSize(12);
        tintButton(button, Color.TRANSPARENT, BLUE, DIVIDER);
        return button;
    }

    private Button button(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(15);
        button.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        button.setAllCaps(false);
        button.setGravity(Gravity.CENTER);
        button.setPadding(dp(10), 0, dp(10), 0);
        button.setStateListAnimator(null);
        return button;
    }

    private void tintButton(Button button, int background, int foreground, int border) {
        button.setTextColor(foreground);
        button.setBackground(rounded(background, border, 14));
    }

    private GradientDrawable rounded(int fill, int stroke, int radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(radiusDp));
        drawable.setStroke(dp(1), stroke);
        return drawable;
    }

    private TextView text(String value, float size, int color, int style) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextSize(size);
        view.setTextColor(color);
        view.setTypeface(Typeface.DEFAULT, style);
        view.setLineSpacing(0, 1.08f);
        return view;
    }

    private LinearLayout column() {
        LinearLayout view = new LinearLayout(this);
        view.setOrientation(LinearLayout.VERTICAL);
        return view;
    }

    private LinearLayout row() {
        LinearLayout view = new LinearLayout(this);
        view.setOrientation(LinearLayout.HORIZONTAL);
        view.setGravity(Gravity.CENTER_VERTICAL);
        return view;
    }

    private LinearLayout.LayoutParams matchWrap() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    private LinearLayout.LayoutParams matchHeight(int height) {
        return new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, height);
    }

    private LinearLayout.LayoutParams topMargin(int margin) {
        LinearLayout.LayoutParams params = matchWrap();
        params.topMargin = margin;
        return params;
    }

    private LinearLayout.LayoutParams bottomMargin(int margin) {
        LinearLayout.LayoutParams params = matchWrap();
        params.bottomMargin = margin;
        return params;
    }

    private String safeName(String value) {
        return value == null || value.trim().isEmpty() ? "未命名设备" : value;
    }

    private String platformName(String platform) {
        if (platform == null) return "未知平台";
        if ("android".equalsIgnoreCase(platform)) return "Android";
        if ("macos".equalsIgnoreCase(platform)) return "macOS";
        if ("windows".equalsIgnoreCase(platform)) return "Windows";
        if ("linux".equalsIgnoreCase(platform)) return "Linux";
        return platform;
    }

    private String platformIcon(String platform) {
        if (platform == null) return "•";
        if ("android".equalsIgnoreCase(platform)) return "A";
        if ("macos".equalsIgnoreCase(platform)) return "M";
        if ("windows".equalsIgnoreCase(platform)) return "W";
        if ("linux".equalsIgnoreCase(platform)) return "L";
        return "•";
    }

    private int dp(int value) {
        return (int) (value * getResources().getDisplayMetrics().density + 0.5f);
    }
}
