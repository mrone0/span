package app.span.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;

import android.Manifest;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.Collections;
import java.util.concurrent.FutureTask;
import java.util.concurrent.TimeUnit;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(AndroidJUnit4.class)
public final class AndroidToPcClipboardTest {
    private static final String PC_ID = "android-send-test-pc";
    private static final String PC_PRIVATE =
            "20191aa972393494664585001f1e02dd15788626702e771975e4dc55f594cc69";
    private static final String ANDROID_PRIVATE =
            "10a88b9374b635a17482554ff6d5acec34c512e1033655dc8764595c284234af";
    private Context context;
    private String pcPublic;
    private String androidPublic;

    @Before public void setUp() throws Exception {
        pcPublic = Hex.encode(X25519.publicFromPrivate(Hex.decode(PC_PRIVATE)));
        androidPublic = Hex.encode(X25519.publicFromPrivate(Hex.decode(ANDROID_PRIVATE)));
        context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit()
                .clear()
                .putString("identity.id", "android-send-test")
                .putString("identity.name", "Android Send Test")
                .putString("identity.private", ANDROID_PRIVATE)
                .putString("identity.public", androidPublic)
                .commit();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            InstrumentationRegistry.getInstrumentation().getUiAutomation()
                    .grantRuntimePermission(
                            context.getPackageName(), Manifest.permission.POST_NOTIFICATIONS);
        }
        new SpanStore(context).saveDevices(Collections.singletonList(new SpanDevice(
                PC_ID,
                "Loopback PC",
                "windows",
                "127.0.0.1",
                pcPublic,
                true,
                System.currentTimeMillis())));
    }

    @After public void tearDown() {
        context.stopService(new Intent(context, SpanReceiveService.class));
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit().clear().commit();
    }

    @Test public void explicitSendActionSendsCurrentClipboardTwice() throws Exception {
        try (ActivityScenario<MainActivity> activity = ActivityScenario.launch(MainActivity.class)) {
            // MainActivity starts the production receiver on 46793. Stop only that
            // listener so this test's fake PC can own the same production port.
            context.stopService(new Intent(context, SpanReceiveService.class));
            waitUntilTextPortCanBind();

            assertClipboardSentExplicitly(activity, "Android clipboard A ✓");
            assertClipboardSentExplicitly(activity, "Android clipboard B ✓");
        }
    }

    private void assertClipboardSentExplicitly(
            ActivityScenario<MainActivity> activity, String expected) throws Exception {
        activity.onActivity(current -> {
            ClipboardManager clipboard =
                    (ClipboardManager) current.getSystemService(Context.CLIPBOARD_SERVICE);
            assertNotNull(clipboard);
            clipboard.setPrimaryClip(ClipData.newPlainText("test", expected));
        });

        try (ServerSocket server = new ServerSocket()) {
            server.setReuseAddress(true);
            server.bind(new InetSocketAddress(InetAddress.getByName("127.0.0.1"), SpanProtocol.TEXT_PORT));
            server.setSoTimeout(5000);
            FutureTask<String> received = new FutureTask<>(() -> receiveAndDecrypt(server));
            Thread receiver = new Thread(received, "fake-span-pc");
            receiver.start();

            activity.onActivity(MainActivity::sendCurrentClipboard);
            assertEquals(expected, received.get(6, TimeUnit.SECONDS));
        }
    }

    private String receiveAndDecrypt(ServerSocket server) throws Exception {
        try (Socket socket = server.accept();
             BufferedReader reader = new BufferedReader(new InputStreamReader(
                     socket.getInputStream(), StandardCharsets.UTF_8))) {
            SpanTextPacket packet = SpanTextPacket.parse(reader.readLine());
            assertNotNull("Android must send a valid Span text packet", packet);
            assertEquals("android-send-test", packet.fromDeviceId);
            return SpanCrypto.decryptText(packet, PC_PRIVATE, androidPublic);
        }
    }

    private void waitUntilTextPortCanBind() throws Exception {
        Exception last = null;
        long deadline = System.currentTimeMillis() + 5000;
        while (System.currentTimeMillis() < deadline) {
            try (ServerSocket probe = new ServerSocket()) {
                probe.setReuseAddress(true);
                probe.bind(new InetSocketAddress("127.0.0.1", SpanProtocol.TEXT_PORT));
                return;
            } catch (Exception error) {
                last = error;
                Thread.sleep(100);
            }
        }
        if (last != null) throw last;
        throw new AssertionError("text port remained busy");
    }
}
