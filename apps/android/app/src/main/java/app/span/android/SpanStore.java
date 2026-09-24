package app.span.android;

import android.content.Context;
import android.content.SharedPreferences;
import android.os.Build;
import java.util.ArrayList;
import java.util.List;
import org.json.JSONArray;
import org.json.JSONObject;

final class SpanStore {
    private static final String PREFS = "span";
    // Every Activity, Service and discovery callback in the Android process uses
    // a different SpanStore instance. Keep the entire devices read-modify-write
    // transaction under one process-wide lock so a stale discovery snapshot
    // cannot overwrite a trust or revoke decision made on another thread.
    private static final Object DEVICES_LOCK = new Object();
    private final SharedPreferences prefs;

    SpanStore(Context context) {
        this.prefs = context.getApplicationContext().getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    LocalIdentity loadOrCreateIdentity() throws Exception {
        String id = prefs.getString("identity.id", null);
        String name = prefs.getString("identity.name", null);
        String privateKey = prefs.getString("identity.private", null);
        String publicKey = prefs.getString("identity.public", null);
        if (id != null && name != null && privateKey != null && publicKey != null) {
            return new LocalIdentity(id, name, privateKey, publicKey);
        }
        LocalIdentity identity = SpanCrypto.createIdentity(Build.MODEL == null ? "Android" : Build.MODEL);
        prefs.edit()
                .putString("identity.id", identity.id)
                .putString("identity.name", identity.name)
                .putString("identity.private", identity.privateKeyHex)
                .putString("identity.public", identity.publicKeyHex)
                .commit();
        return identity;
    }


    boolean isReceiverEnabled() {
        return prefs.getBoolean("receiver.enabled", true);
    }

    void setReceiverEnabled(boolean enabled) {
        prefs.edit().putBoolean("receiver.enabled", enabled).commit();
    }

    SpanDevice trustedDevice(String id) {
        for (SpanDevice device : loadDevices()) {
            if (device.trusted && device.id.equals(id)) return device;
        }
        return null;
    }

    SpanDevice device(String id) {
        if (id == null) return null;
        for (SpanDevice device : loadDevices()) {
            if (id.equals(device.id)) return device;
        }
        return null;
    }

    List<SpanDevice> loadDevices() {
        synchronized (DEVICES_LOCK) {
            return loadDevicesLocked();
        }
    }

    private List<SpanDevice> loadDevicesLocked() {
        List<SpanDevice> devices = new ArrayList<>();
        String raw = prefs.getString("devices", "[]");
        try {
            JSONArray array = new JSONArray(raw);
            for (int i = 0; i < array.length(); i++) {
                JSONObject o = array.getJSONObject(i);
                devices.add(new SpanDevice(
                        o.optString("id"),
                        o.optString("name"),
                        o.optString("platform"),
                        o.optString("host", null),
                        o.optString("publicKeyHex", null),
                        storedTrustState(o),
                        o.optLong("lastSeenMillis")
                ));
            }
        } catch (Exception ignored) {}
        return devices;
    }

    void saveDevices(List<SpanDevice> devices) {
        synchronized (DEVICES_LOCK) {
            saveDevicesLocked(devices);
        }
    }

    private void saveDevicesLocked(List<SpanDevice> devices) {
        devices = compactDevices(devices);
        JSONArray array = new JSONArray();
        try {
            for (SpanDevice d : devices) {
                JSONObject o = new JSONObject();
                o.put("id", d.id);
                o.put("name", d.name);
                o.put("platform", d.platform);
                o.put("host", d.host);
                o.put("publicKeyHex", d.publicKeyHex);
                o.put("trusted", d.trusted);
                o.put("trustState", d.trustState);
                o.put("lastSeenMillis", d.lastSeenMillis);
                array.put(o);
            }
        } catch (Exception ignored) {}
        // Trust is security state. Persist it before returning so a process kill
        // or reboot immediately after pairing cannot discard the confirmation.
        prefs.edit().putString("devices", array.toString()).commit();
    }

    void upsertDiscovered(SpanDevice discovered) {
        if (discovered == null) return;
        synchronized (DEVICES_LOCK) {
            List<SpanDevice> devices = loadDevicesLocked();
            for (SpanDevice existing : devices) {
                if (sameDevice(existing, discovered)) {
                    boolean keyMatches = existing.publicKeyHex == null
                            || discovered.publicKeyHex == null
                            || existing.publicKeyHex.equalsIgnoreCase(discovered.publicKeyHex);
                    // Never replace a known cryptographic identity from an
                    // unauthenticated discovery datagram, regardless of state.
                    if (!keyMatches) return;
                    existing.id = discovered.id;
                    existing.name = discovered.name;
                    existing.platform = discovered.platform;
                    existing.host = discovered.host;
                    if (existing.publicKeyHex == null) {
                        existing.publicKeyHex = discovered.publicKeyHex;
                    }
                    existing.lastSeenMillis = Math.max(
                            existing.lastSeenMillis, discovered.lastSeenMillis);
                    // Discovery only refreshes identity/address data. In
                    // particular it cannot revive a revoked device.
                    saveDevicesLocked(devices);
                    return;
                }
            }
            discovered.setTrustState(SpanDevice.STATE_DISCOVERED);
            devices.add(discovered);
            saveDevicesLocked(devices);
        }
    }

    private List<SpanDevice> compactDevices(List<SpanDevice> devices) {
        List<SpanDevice> compacted = new ArrayList<>();
        for (SpanDevice device : devices) {
            SpanDevice existing = null;
            for (SpanDevice candidate : compacted) {
                if (sameDevice(candidate, device)) {
                    existing = candidate;
                    break;
                }
            }
            if (existing == null) {
                compacted.add(device);
                continue;
            }
            boolean keyConflict = existing.publicKeyHex != null
                    && device.publicKeyHex != null
                    && !existing.publicKeyHex.equalsIgnoreCase(device.publicKeyHex);
            if (keyConflict) continue;
            existing.setTrustState(strongerTrustState(existing.trustState, device.trustState));
            existing.id = device.id;
            existing.name = device.name;
            existing.platform = device.platform;
            existing.host = device.host == null ? existing.host : device.host;
            if (existing.publicKeyHex == null) existing.publicKeyHex = device.publicKeyHex;
            existing.lastSeenMillis = Math.max(existing.lastSeenMillis, device.lastSeenMillis);
        }
        return compacted;
    }

    private boolean sameDevice(SpanDevice left, SpanDevice right) {
        if (left.id != null && left.id.equals(right.id)) return true;
        if (left.publicKeyHex != null && right.publicKeyHex != null
                && left.publicKeyHex.equalsIgnoreCase(right.publicKeyHex)) return true;
        if (left.publicKeyHex != null && right.publicKeyHex != null) return false;
        return left.name != null && left.name.equals(right.name)
                && left.platform != null && left.platform.equals(right.platform);
    }

    boolean setTrusted(String id, boolean trusted) {
        synchronized (DEVICES_LOCK) {
            List<SpanDevice> devices = loadDevicesLocked();
            boolean changed = false;
            for (SpanDevice device : devices) {
                if (device.id.equals(id)) {
                    device.setTrustState(
                            trusted ? SpanDevice.STATE_TRUSTED : SpanDevice.STATE_REVOKED);
                    changed = true;
                }
            }
            if (changed) saveDevicesLocked(devices);
            return changed;
        }
    }

    boolean acceptPairing(String id, String authenticatedPublicKeyHex) {
        if (id == null || authenticatedPublicKeyHex == null) return false;
        synchronized (DEVICES_LOCK) {
            List<SpanDevice> devices = loadDevicesLocked();
            for (SpanDevice device : devices) {
                if (!id.equals(device.id)) continue;
                if (!device.canAcceptRemotePairing()) return false;
                if (device.publicKeyHex == null
                        || !device.publicKeyHex.equalsIgnoreCase(authenticatedPublicKeyHex)) {
                    return false;
                }
                device.setTrustState(SpanDevice.STATE_TRUSTED);
                saveDevicesLocked(devices);
                return true;
            }
            return false;
        }
    }

    private static String storedTrustState(JSONObject object) {
        String state = object.optString("trustState", "");
        if (!state.isEmpty()) return state;
        // Older APKs only stored a boolean, so `false` could mean either newly
        // discovered or explicitly removed. Migrate that ambiguous state to
        // revoked: requiring one fresh local tap is safer than allowing a
        // delayed remote accept to resurrect a device the user removed.
        return object.optBoolean("trusted")
                ? SpanDevice.STATE_TRUSTED : SpanDevice.STATE_REVOKED;
    }

    private static String strongerTrustState(String left, String right) {
        return trustStateRank(right) > trustStateRank(left) ? right : left;
    }

    private static int trustStateRank(String state) {
        if (SpanDevice.STATE_REVOKED.equals(state)) return 3;
        if (SpanDevice.STATE_TRUSTED.equals(state)) return 2;
        if (SpanDevice.STATE_PENDING.equals(state)) return 1;
        return 0;
    }
}
