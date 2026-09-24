package app.span.android;

import android.content.Context;
import android.net.wifi.WifiManager;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.InterfaceAddress;
import java.net.NetworkInterface;
import java.net.PortUnreachableException;
import java.net.SocketException;
import java.net.SocketTimeoutException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.atomic.AtomicBoolean;

final class SpanDiscovery {
    private static final String TAG = "SpanDiscovery";
    private static final long ANNOUNCE_INTERVAL_MS = 15_000;
    private static final long RETRY_DELAY_MS = 1_000;

    interface Listener { void onDevice(SpanDevice device); }

    interface PacketSender { void send(DatagramPacket packet) throws IOException; }

    private final Context context;
    private final LocalIdentity identity;
    private final Listener listener;
    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final Object lifecycleLock = new Object();
    private final AtomicBoolean announceRequested = new AtomicBoolean(true);
    private volatile boolean running;
    private volatile long generation;
    private volatile boolean destroyed;
    private volatile DatagramSocket socket;
    private WifiManager.MulticastLock multicastLock;

    SpanDiscovery(Context context, LocalIdentity identity, Listener listener) {
        this.context = context.getApplicationContext();
        this.identity = identity;
        this.listener = listener;
    }

    void start() {
        final long runGeneration;
        synchronized (lifecycleLock) {
            if (running || destroyed) return;
            running = true;
            runGeneration = ++generation;
        }
        try {
            executor.execute(() -> runLoop(runGeneration));
        } catch (RejectedExecutionException error) {
            synchronized (lifecycleLock) {
                if (generation == runGeneration) running = false;
            }
            throw error;
        }
    }

    void stop() {
        DatagramSocket current;
        synchronized (lifecycleLock) {
            if (!running) return;
            running = false;
            generation++;
            current = socket;
        }
        // Closing the socket wakes a blocking receive immediately. The worker
        // owns the multicast lock and releases it after the receive loop exits.
        if (current != null) current.close();
    }

    void destroy() {
        synchronized (lifecycleLock) {
            destroyed = true;
        }
        stop();
        executor.shutdownNow();
    }

    void announceOnce() {
        // A short-lived socket cannot receive the unicast replies to its probe.
        // Queue the request for the bound listener instead; it will send as soon
        // as it is available and receive replies on that same port.
        announceRequested.set(true);
        if (!running) start();
    }

    private void runLoop(long runGeneration) {
        try {
            acquireMulticastLock();
            while (isActive(runGeneration)) {
                DatagramSocket current = null;
                try {
                    current = openListenerSocket();
                    if (!publishSocket(runGeneration, current)) break;
                    receiveLoop(runGeneration, current);
                } catch (Exception error) {
                    if (isActive(runGeneration)) {
                        Log.w(TAG, "Discovery listener failed; retrying", error);
                    }
                } finally {
                    clearSocket(current);
                    if (current != null) current.close();
                }

                if (isActive(runGeneration) && !sleepBeforeRetry()) {
                    break;
                }
            }
        } finally {
            releaseMulticastLock();
            synchronized (lifecycleLock) {
                if (generation == runGeneration) {
                    socket = null;
                    running = false;
                }
            }
        }
    }

    private DatagramSocket openListenerSocket() throws IOException {
        DatagramSocket listenerSocket = new DatagramSocket(null);
        try {
            listenerSocket.setReuseAddress(true);
            listenerSocket.setBroadcast(true);
            listenerSocket.bind(new InetSocketAddress(
                    InetAddress.getByName("0.0.0.0"), SpanProtocol.DISCOVERY_PORT));
            listenerSocket.setSoTimeout(500);
            return listenerSocket;
        } catch (IOException | RuntimeException error) {
            listenerSocket.close();
            throw error;
        }
    }

    private void receiveLoop(long runGeneration, DatagramSocket listenerSocket) throws IOException {
        long nextAnnounce = 0;
        byte[] buffer = new byte[2048];
        while (isActive(runGeneration) && !listenerSocket.isClosed()) {
            long now = System.currentTimeMillis();
            if (announceRequested.getAndSet(false) || now >= nextAnnounce) {
                sendProbe(listenerSocket);
                sendAnnouncement(listenerSocket);
                nextAnnounce = now + ANNOUNCE_INTERVAL_MS;
            }

            try {
                DatagramPacket packet = new DatagramPacket(buffer, buffer.length);
                listenerSocket.receive(packet);
                handlePacket(listenerSocket, packet);
            } catch (SocketTimeoutException | PortUnreachableException ignored) {
                // A timeout is the normal wake-up path. Some Android network
                // stacks report an ICMP response to a previous broadcast as a
                // PortUnreachableException; neither should stop discovery.
            } catch (SocketException error) {
                if (!isActive(runGeneration) || listenerSocket.isClosed()) break;
                if (!isTransientUdpError(error)) throw error;
                Log.d(TAG, "Ignoring transient discovery receive error", error);
                if (!sleepAfterTransientError()) break;
            } catch (RuntimeException error) {
                // A malformed packet or listener callback must not take the UDP
                // listener down. Continue with the next datagram.
                Log.w(TAG, "Ignoring invalid discovery packet", error);
            }
        }
    }

    private boolean isActive(long runGeneration) {
        return running && generation == runGeneration && !Thread.currentThread().isInterrupted();
    }

    private boolean publishSocket(long runGeneration, DatagramSocket current) {
        synchronized (lifecycleLock) {
            if (!isActive(runGeneration)) return false;
            socket = current;
            return true;
        }
    }

    private void clearSocket(DatagramSocket current) {
        synchronized (lifecycleLock) {
            if (socket == current) socket = null;
        }
    }

    private boolean sleepBeforeRetry() {
        try {
            Thread.sleep(RETRY_DELAY_MS);
            return true;
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    private boolean sleepAfterTransientError() {
        try {
            Thread.sleep(50);
            return true;
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    private void handlePacket(DatagramSocket listenerSocket, DatagramPacket packet) {
        String value = new String(packet.getData(), 0, packet.getLength(), StandardCharsets.UTF_8);
        String host = packet.getAddress().getHostAddress();
        if (SpanProtocol.DISCOVERY_PROBE_MAGIC.equals(value.trim())) {
            sendAnnouncementTo(listenerSocket, packet.getAddress(), packet.getPort());
            return;
        }
        SpanDevice device = parsePacket(value, host);
        if (device != null && !identity.id.equals(device.id)) {
            mainHandler.post(() -> {
                try {
                    listener.onDevice(device);
                } catch (RuntimeException error) {
                    Log.w(TAG, "Discovery listener rejected a device", error);
                }
            });
        }
    }

    private void sendProbe(DatagramSocket out) {
        try {
            byte[] data = SpanProtocol.DISCOVERY_PROBE_MAGIC.getBytes(StandardCharsets.UTF_8);
            sendToBroadcasts(out, data);
        } catch (Exception ignored) {
        }
    }

    private void sendAnnouncement(DatagramSocket out) {
        try {
            sendToBroadcasts(out, announcementBytes());
        } catch (Exception ignored) {
        }
    }

    private void sendAnnouncementTo(DatagramSocket out, InetAddress address, int port) {
        try {
            sendUnicast(out, announcementBytes(), address, port);
        } catch (IOException error) {
            Log.d(TAG, "Could not reply to discovery probe", error);
        }
    }

    private void sendToBroadcasts(DatagramSocket out, byte[] data) {
        Set<InetAddress> targets = new LinkedHashSet<>();
        Enumeration<NetworkInterface> interfaces = null;
        try {
            interfaces = NetworkInterface.getNetworkInterfaces();
        } catch (Exception error) {
            Log.d(TAG, "Could not enumerate discovery interfaces", error);
        }

        while (interfaces != null && interfaces.hasMoreElements()) {
            NetworkInterface networkInterface;
            try {
                networkInterface = interfaces.nextElement();
                if (!networkInterface.isUp() || networkInterface.isLoopback()) continue;
            } catch (Exception error) {
                // A broken VPN or virtual adapter must not prevent packets from
                // being sent over the remaining interfaces.
                Log.d(TAG, "Skipping unavailable discovery interface", error);
                continue;
            }

            List<InterfaceAddress> addresses;
            try {
                addresses = new ArrayList<>(networkInterface.getInterfaceAddresses());
            } catch (Exception error) {
                Log.d(TAG, "Skipping unreadable discovery interface", error);
                continue;
            }
            for (InterfaceAddress address : addresses) {
                try {
                    InetAddress broadcast = address.getBroadcast();
                    if (broadcast != null) targets.add(broadcast);
                } catch (Exception error) {
                    // Isolate each address as Android may expose stale entries
                    // while Wi-Fi, VPN or hotspot state is changing.
                    Log.d(TAG, "Skipping invalid broadcast address", error);
                }
            }
        }

        // Some Android devices do not expose InterfaceAddress broadcast values.
        // Keep the global broadcast fallback so discovery still works on simple LANs.
        try {
            targets.add(InetAddress.getByName("255.255.255.255"));
        } catch (Exception error) {
            Log.d(TAG, "Could not create the global broadcast address", error);
        }
        int sent = sendToTargets(out::send, data, targets, SpanProtocol.DISCOVERY_PORT);
        if (sent == 0) Log.d(TAG, "Discovery broadcast did not reach any interface");
    }

    static int sendToTargets(PacketSender sender, byte[] data,
                             Iterable<InetAddress> targets, int port) {
        int sent = 0;
        for (InetAddress target : targets) {
            if (target == null) continue;
            try {
                sender.send(new DatagramPacket(data, data.length, target, port));
                sent++;
            } catch (Exception error) {
                // Keep trying other adapters/targets. It is common for a VPN or
                // a just-disconnected Wi-Fi interface to reject its broadcast.
            }
        }
        return sent;
    }

    static void sendUnicast(DatagramSocket out, byte[] data,
                            InetAddress address, int port) throws IOException {
        out.send(new DatagramPacket(data, data.length, address, port));
    }

    private byte[] announcementBytes() {
        String payload = SpanProtocol.DISCOVERY_MAGIC + "\t" + identity.id + "\t" + sanitize(identity.name) + "\tandroid\t" + identity.publicKeyHex;
        return payload.getBytes(StandardCharsets.UTF_8);
    }

    private SpanDevice parsePacket(String value, String host) {
        String[] parts = value.trim().split("\t", -1);
        if (parts.length < 5 || !SpanProtocol.DISCOVERY_MAGIC.equals(parts[0])) return null;
        byte[] key = Hex.decode(parts[4]);
        if (key == null || key.length != 32) return null;
        return new SpanDevice(parts[1], parts[2], parts[3], host, parts[4], false, System.currentTimeMillis());
    }

    private void acquireMulticastLock() {
        try {
            WifiManager wifi = (WifiManager) context.getSystemService(Context.WIFI_SERVICE);
            if (wifi == null) return;
            multicastLock = wifi.createMulticastLock("span-discovery");
            multicastLock.setReferenceCounted(false);
            multicastLock.acquire();
        } catch (Exception ignored) {
        }
    }

    private void releaseMulticastLock() {
        try {
            if (multicastLock != null && multicastLock.isHeld()) multicastLock.release();
        } catch (Exception ignored) {
        } finally {
            multicastLock = null;
        }
    }

    static boolean isTransientUdpError(SocketException error) {
        String message = error.getMessage();
        if (message == null) return false;
        String normalized = message.toLowerCase(Locale.ROOT);
        return normalized.contains("unreachable")
                || normalized.contains("no route")
                || normalized.contains("reset")
                || normalized.contains("refused")
                || normalized.contains("enetunreach")
                || normalized.contains("ehostunreach");
    }

    private String sanitize(String s) {
        return s.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }
}
