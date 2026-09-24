package app.span.android;

final class SpanTextPacket {
    enum Kind { TEXT, PAIRING_ACCEPT }

    final Kind kind;
    final String fromDeviceId;
    final byte[] nonce;
    final byte[] ciphertextAndTag;

    SpanTextPacket(String fromDeviceId, byte[] nonce, byte[] ciphertextAndTag) {
        this(Kind.TEXT, fromDeviceId, nonce, ciphertextAndTag);
    }

    SpanTextPacket(Kind kind, String fromDeviceId, byte[] nonce, byte[] ciphertextAndTag) {
        this.kind = kind;
        this.fromDeviceId = fromDeviceId;
        this.nonce = nonce;
        this.ciphertextAndTag = ciphertextAndTag;
    }

    static SpanTextPacket parse(String line) {
        if (line == null) return null;
        String[] parts = line.trim().split("\t", -1);
        if (parts.length != 4) return null;
        Kind kind;
        if (SpanProtocol.TEXT_MAGIC.equals(parts[0])) {
            kind = Kind.TEXT;
        } else if (SpanProtocol.PAIRING_ACCEPT_MAGIC.equals(parts[0])) {
            kind = Kind.PAIRING_ACCEPT;
        } else {
            return null;
        }
        byte[] nonce = Hex.decode(parts[2]);
        byte[] ciphertextAndTag = Hex.decode(parts[3]);
        if (parts[1].trim().isEmpty()) return null;
        if (nonce == null || nonce.length != SpanProtocol.NONCE_BYTES) return null;
        if (ciphertextAndTag == null || ciphertextAndTag.length > SpanProtocol.MAX_TEXT_BYTES + 32) return null;
        return new SpanTextPacket(kind, parts[1], nonce, ciphertextAndTag);
    }
}
