package app.span.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import android.content.Context;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import java.util.Collections;
import java.util.concurrent.CountDownLatch;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(AndroidJUnit4.class)
public final class SpanPairingStoreTest {
    private static final String DEVICE_ID = "desktop-test";
    private static final String PUBLIC_KEY =
            "59716dbff7b07fe4ae16c18e7712a4a9d6036e9737f621f6f9e7144e9651241d";

    private Context context;

    @Before public void setUp() {
        context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit().clear().commit();
    }

    @After public void tearDown() {
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit().clear().commit();
    }

    @Test public void authenticatedAcceptTrustsDiscoveredAndPendingDevices() {
        SpanStore store = new SpanStore(context);
        store.saveDevices(Collections.singletonList(device(SpanDevice.STATE_DISCOVERED)));
        assertTrue(store.acceptPairing(DEVICE_ID, PUBLIC_KEY));
        assertTrue(store.trustedDevice(DEVICE_ID).trusted);

        store.saveDevices(Collections.singletonList(device(SpanDevice.STATE_PENDING)));
        assertTrue(store.acceptPairing(DEVICE_ID, PUBLIC_KEY));
        assertTrue(store.trustedDevice(DEVICE_ID).trusted);
    }

    @Test public void revokedDeviceCannotBeRevivedByDiscoveryOrDelayedAccept() {
        SpanStore store = new SpanStore(context);
        store.saveDevices(Collections.singletonList(device(SpanDevice.STATE_DISCOVERED)));
        assertTrue(store.setTrusted(DEVICE_ID, true));
        assertTrue(store.setTrusted(DEVICE_ID, false));

        store.upsertDiscovered(device(SpanDevice.STATE_DISCOVERED));
        assertFalse(store.acceptPairing(DEVICE_ID, PUBLIC_KEY));

        SpanDevice stored = store.device(DEVICE_ID);
        assertNotNull(stored);
        assertFalse(stored.trusted);
        assertTrue(stored.isRevoked());
    }

    @Test public void processWideLockPreventsDiscoveryFromOverwritingRevoke() throws Exception {
        SpanStore uiStore = new SpanStore(context);
        SpanStore discoveryStore = new SpanStore(context);
        uiStore.saveDevices(Collections.singletonList(device(SpanDevice.STATE_TRUSTED)));
        CountDownLatch start = new CountDownLatch(1);

        Thread revoke = new Thread(() -> {
            await(start);
            for (int i = 0; i < 100; i++) uiStore.setTrusted(DEVICE_ID, false);
        });
        Thread discover = new Thread(() -> {
            await(start);
            for (int i = 0; i < 100; i++) {
                discoveryStore.upsertDiscovered(device(SpanDevice.STATE_DISCOVERED));
            }
        });
        revoke.start();
        discover.start();
        start.countDown();
        revoke.join();
        discover.join();

        SpanDevice stored = uiStore.device(DEVICE_ID);
        assertNotNull(stored);
        assertEquals(SpanDevice.STATE_REVOKED, stored.trustState);
        assertFalse(uiStore.acceptPairing(DEVICE_ID, PUBLIC_KEY));
    }

    @Test public void pairingAcceptRequiresThePinnedDiscoveryKey() {
        SpanStore store = new SpanStore(context);
        store.saveDevices(Collections.singletonList(device(SpanDevice.STATE_DISCOVERED)));

        assertFalse(store.acceptPairing(DEVICE_ID, "00" + PUBLIC_KEY.substring(2)));
        assertEquals(SpanDevice.STATE_DISCOVERED, store.device(DEVICE_ID).trustState);
    }

    @Test public void legacyUntrustedStateMigratesConservativelyToRevoked() {
        String legacy = "[{\"id\":\"" + DEVICE_ID
                + "\",\"name\":\"Test desktop\",\"platform\":\"windows\""
                + ",\"host\":\"127.0.0.1\",\"publicKeyHex\":\"" + PUBLIC_KEY
                + "\",\"trusted\":false,\"lastSeenMillis\":1}]";
        context.getSharedPreferences("span", Context.MODE_PRIVATE).edit()
                .putString("devices", legacy)
                .commit();

        SpanStore store = new SpanStore(context);
        assertEquals(SpanDevice.STATE_REVOKED, store.device(DEVICE_ID).trustState);
        assertFalse(store.acceptPairing(DEVICE_ID, PUBLIC_KEY));
    }

    private static SpanDevice device(String state) {
        return new SpanDevice(
                DEVICE_ID,
                "Test desktop",
                "windows",
                "127.0.0.1",
                PUBLIC_KEY,
                state,
                System.currentTimeMillis());
    }

    private static void await(CountDownLatch latch) {
        try {
            latch.await();
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new AssertionError(error);
        }
    }
}
