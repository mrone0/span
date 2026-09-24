package app.span.android;

import static org.junit.Assert.assertNotNull;

import android.content.Context;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import java.util.Collections;
import org.junit.Test;
import org.junit.runner.RunWith;

/** Prepares deterministic state for the external accessibility smoke test. */
@RunWith(AndroidJUnit4.class)
public final class AccessibilityClipboardFixtureTest {
    private static final String PC_ID = "rust-test-pc";
    private static final String PC_PUBLIC =
            "59716dbff7b07fe4ae16c18e7712a4a9d6036e9737f621f6f9e7144e9651241d";
    private static final String ANDROID_PRIVATE =
            "b34293c9e4883a86feb43c65fc1bfa806ab709e2465965727555301b5e0db991";
    private static final String ANDROID_PUBLIC =
            "bc8be9171b797e0f82222994e58411ab111db7832c2d0755f1208f4252d60066";

    @Test public void configureTrustedRustPeer() {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit()
                .clear()
                .putString("identity.id", "android-test")
                .putString("identity.name", "Android Test")
                .putString("identity.private", ANDROID_PRIVATE)
                .putString("identity.public", ANDROID_PUBLIC)
                .putBoolean("receiver.enabled", true)
                .commit();
        new SpanStore(context).saveDevices(Collections.singletonList(new SpanDevice(
                PC_ID,
                "Rust Test PC",
                "windows",
                // Android Emulator's host gateway. The external smoke driver
                // listens on the host's production TCP port.
                "10.0.2.2",
                PC_PUBLIC,
                true,
                System.currentTimeMillis())));
        assertNotNull(new SpanStore(context).trustedDevice(PC_ID));
    }
}
