package app.span.android;

final class SpanProtocol {
    private SpanProtocol() {}

    static final int DISCOVERY_PORT = 46792;
    static final int TEXT_PORT = 46793;
    static final String DISCOVERY_MAGIC = "SPAN_DISCOVERY_V2";
    static final String DISCOVERY_PROBE_MAGIC = "SPAN_DISCOVERY_PROBE_V1";
    static final String TEXT_MAGIC = "SPAN_TEXT_V3";
    static final String PAIRING_ACCEPT_MAGIC = "SPAN_PAIR_ACCEPT_V1";
    static final String PAIRING_ACCEPT_PROOF = "\0SPAN_PAIR_ACCEPT_V1\0";
    static final byte[] TEXT_KEY_INFO = "span-text-v3".getBytes(java.nio.charset.StandardCharsets.UTF_8);
    static final int NONCE_BYTES = 12;
    static final int MAX_TEXT_BYTES = 64 * 1024;
}
