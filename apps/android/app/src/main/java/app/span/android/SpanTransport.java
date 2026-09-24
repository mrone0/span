package app.span.android;

import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;

final class SpanTransport {
    void sendText(String text, LocalIdentity identity, SpanDevice device) throws Exception {
        if (text == null || text.isEmpty()) return;
        if (text.getBytes(StandardCharsets.UTF_8).length > SpanProtocol.MAX_TEXT_BYTES) {
            throw new IllegalArgumentException("text too large");
        }
        sendEncrypted(SpanProtocol.TEXT_MAGIC, text, identity, device);
    }

    void sendPairingAccept(LocalIdentity identity, SpanDevice device) throws Exception {
        sendEncrypted(
                SpanProtocol.PAIRING_ACCEPT_MAGIC,
                SpanProtocol.PAIRING_ACCEPT_PROOF,
                identity,
                device);
    }

    private void sendEncrypted(
            String magic, String plaintext, LocalIdentity identity, SpanDevice device)
            throws Exception {
        if (device.host == null || device.host.trim().isEmpty()) throw new IllegalArgumentException("missing host");
        if (device.publicKeyHex == null || device.publicKeyHex.trim().isEmpty()) throw new IllegalArgumentException("missing key");
        SpanCrypto.Encrypted encrypted = SpanCrypto.encryptText(
                plaintext, identity.privateKeyHex, device.publicKeyHex);
        String line = magic + "\t" + identity.id + "\t" + encrypted.nonceHex + "\t"
                + encrypted.ciphertextHex + "\n";
        byte[] data = line.getBytes(StandardCharsets.UTF_8);
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress(device.host, SpanProtocol.TEXT_PORT), 2500);
            socket.setTcpNoDelay(true);
            OutputStream out = socket.getOutputStream();
            out.write(data);
            out.flush();
        }
    }
}
