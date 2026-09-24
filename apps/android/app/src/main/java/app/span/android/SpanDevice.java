package app.span.android;

final class SpanDevice {
    static final String STATE_DISCOVERED = "discovered";
    static final String STATE_PENDING = "pending";
    static final String STATE_TRUSTED = "trusted";
    static final String STATE_REVOKED = "revoked";

    String id;
    String name;
    String platform;
    String host;
    String publicKeyHex;
    boolean trusted;
    String trustState;
    long lastSeenMillis;

    SpanDevice(String id, String name, String platform, String host, String publicKeyHex, boolean trusted, long lastSeenMillis) {
        this(id, name, platform, host, publicKeyHex,
                trusted ? STATE_TRUSTED : STATE_DISCOVERED, lastSeenMillis);
    }

    SpanDevice(String id, String name, String platform, String host, String publicKeyHex,
               String trustState, long lastSeenMillis) {
        this.id = id;
        this.name = name;
        this.platform = platform;
        this.host = host;
        this.publicKeyHex = publicKeyHex;
        setTrustState(trustState);
        this.lastSeenMillis = lastSeenMillis;
    }

    void setTrustState(String state) {
        if (!STATE_DISCOVERED.equals(state)
                && !STATE_PENDING.equals(state)
                && !STATE_TRUSTED.equals(state)
                && !STATE_REVOKED.equals(state)) {
            state = STATE_DISCOVERED;
        }
        trustState = state;
        trusted = STATE_TRUSTED.equals(state);
    }

    boolean canAcceptRemotePairing() {
        return STATE_DISCOVERED.equals(trustState) || STATE_PENDING.equals(trustState);
    }

    boolean isRevoked() {
        return STATE_REVOKED.equals(trustState);
    }
}
