package app.span.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.SocketException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;

public final class SpanDiscoveryTest {
    @Test public void unicastReplyReusesListenerSourcePort() throws Exception {
        InetAddress loopback = InetAddress.getByName("127.0.0.1");
        try (DatagramSocket listener = new DatagramSocket(new InetSocketAddress(loopback, 0));
             DatagramSocket scanner = new DatagramSocket(new InetSocketAddress(loopback, 0))) {
            scanner.setSoTimeout(1_000);
            byte[] payload = "announcement".getBytes(StandardCharsets.UTF_8);

            SpanDiscovery.sendUnicast(
                    listener, payload, loopback, scanner.getLocalPort());

            byte[] buffer = new byte[64];
            DatagramPacket received = new DatagramPacket(buffer, buffer.length);
            scanner.receive(received);
            assertEquals(listener.getLocalPort(), received.getPort());
            assertEquals("announcement", new String(
                    received.getData(), 0, received.getLength(), StandardCharsets.UTF_8));
        }
    }

    @Test public void failedBroadcastTargetDoesNotBlockRemainingTargets() throws Exception {
        List<InetAddress> targets = Arrays.asList(
                InetAddress.getByName("192.0.2.1"),
                InetAddress.getByName("192.0.2.2"),
                InetAddress.getByName("192.0.2.3"));
        List<InetAddress> attempted = new ArrayList<>();

        int sent = SpanDiscovery.sendToTargets(packet -> {
            attempted.add(packet.getAddress());
            if (attempted.size() == 1) throw new IOException("stale VPN adapter");
        }, new byte[] {1}, targets, SpanProtocol.DISCOVERY_PORT);

        assertEquals(targets, attempted);
        assertEquals(2, sent);
    }

    @Test public void classifiesCommonTransientUdpErrors() {
        assertTrue(SpanDiscovery.isTransientUdpError(
                new SocketException("sendto failed: EHOSTUNREACH (No route to host)")));
        assertTrue(SpanDiscovery.isTransientUdpError(
                new SocketException("recvfrom failed: ECONNRESET (Connection reset)")));
        assertFalse(SpanDiscovery.isTransientUdpError(
                new SocketException("Socket is closed")));
    }
}
