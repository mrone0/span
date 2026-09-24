mod autostart;
mod clipboard;
mod config;
mod crypto;
mod daemon_control;
mod discovery;
mod gui;
mod transport;
mod trust_store;

use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use span_core::{DeviceId, TrustState, broadcast_targets};

use crate::clipboard::{Clipboard, system_clipboard};
use crate::config::{
    LocalDevice, gui_executable_path, load_or_create_local_device, parse_platform, platform_name,
    trust_store_path,
};
use crate::crypto::decode_hex;
use crate::daemon_control::{start_daemon, stop_daemon};
use crate::discovery::{
    DiscoveryMessage, broadcast_once, discover_once, listen_forever, respond_to_probe,
};
use crate::transport::{
    EncryptedPacketKind, TEXT_PORT, decrypt_text, encrypt_pairing_accept, encrypt_text,
    receive_text_forever, receive_text_once, send_text,
};
use crate::trust_store::TrustStore;

const PAIRING_ACCEPT_TEXT: &str = "\0SPAN_PAIR_ACCEPT_V1\0";
const REMOTE_CLIPBOARD_SUPPRESSION_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct ClipboardEchoGuard {
    pending: Option<RemoteClipboardMarker>,
}

struct RemoteClipboardMarker {
    text: String,
    expires_at: Instant,
}

impl ClipboardEchoGuard {
    fn arm(&mut self, text: &str) {
        self.pending = Some(RemoteClipboardMarker {
            text: text.to_string(),
            expires_at: Instant::now() + REMOTE_CLIPBOARD_SUPPRESSION_TTL,
        });
    }

    fn cancel(&mut self, text: &str) {
        if self.pending.as_ref().map(|marker| marker.text.as_str()) == Some(text) {
            self.pending = None;
        }
    }

    fn should_suppress(&mut self, text: &str) -> bool {
        // Consume the marker even on a mismatch. If the user copies something
        // else before the remote write notification is observed, that local
        // copy must win and a stale marker must never suppress a future copy.
        let Some(marker) = self.pending.take() else {
            return false;
        };
        marker.expires_at >= Instant::now() && marker.text == text
    }
}

pub fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        // The standalone release bundle is GUI-first too: launching `span`
        // directly opens the companion native GUI when available. The npm
        // launcher does the same without briefly attaching a terminal.
        return open_gui_command();
    };

    match command.as_str() {
        // Public commands: keep this surface intentionally small.
        "install" => install_span(),
        "start" => start_daemon(),
        "stop" => stop_daemon(),
        "restart" => restart_daemon(),
        "discover" => discover_devices(),
        "accept" => accept_device(args.next()),
        "send" => send_command(args.collect()),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }

        // Compatibility/debug commands. Supported for scripts and diagnostics,
        // but intentionally hidden from the normal help output.
        "run" | "foreground" => run_daemon(),
        "status" => print_status(),
        "devices" => print_devices(),
        "trust" => trust_device(args.collect()),
        "revoke" => revoke_device(args.next()),
        "reset" => reset_devices(),
        "announce" => announce_device(),
        "clip-read" => read_clipboard(),
        "clip-write" => write_clipboard(args.collect()),
        "send-text" => send_text_command(args.collect()),
        "receive-once" => receive_once(),
        "uninstall" => uninstall_autostart(),
        "gui" | "ui" | "open" => open_gui_command(),
        _ => {
            eprintln!("unknown command: {command}");
            print_help();
            Ok(())
        }
    }
}

fn open_gui_command() -> io::Result<()> {
    let exe = gui_executable_path()?;
    if !exe.exists() {
        return gui::open().or_else(|error| {
            eprintln!("Span GUI unavailable: {error}");
            print_help();
            Ok(())
        });
    }

    let mut command = std::process::Command::new(exe);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    detach_gui_command(&mut command);
    command.spawn()?;
    println!("span gui opened");
    Ok(())
}

fn run_daemon() -> io::Result<()> {
    let _instance_lock = acquire_daemon_instance_lock()?;
    #[cfg(not(target_os = "macos"))]
    std::fs::write(
        crate::config::daemon_pid_path()?,
        format!("{}\n", std::process::id()),
    )?;

    let local = load_or_create_local_device()?;
    let store_path = trust_store_path()?;
    let store = TrustStore::load(&store_path)?;
    let clipboard_echo_guard = Arc::new(Mutex::new(ClipboardEchoGuard::default()));
    let latest_pending_text = Arc::new(Mutex::new(None::<String>));

    println!("span daemon");
    println!("device : {} ({})", local.name, local.id);
    println!("mode   : background-only, no main window");
    println!("policy : broadcast text to trusted devices only");
    println!("targets: {} trusted", store.trusted_devices().len());
    println!("listen : 0.0.0.0:{TEXT_PORT}");

    spawn_receiver(local.clone(), clipboard_echo_guard.clone());
    spawn_discovery_listener(
        store_path.clone(),
        local.clone(),
        latest_pending_text.clone(),
    );

    let mut clipboard = system_clipboard();
    let mut last_change_count = clipboard.change_count().unwrap_or(None);
    let mut last_seen_text = clipboard.read_text().ok().flatten();
    let mut next_forced_clipboard_check = std::time::Instant::now() + Duration::from_secs(2);
    let mut next_announce = std::time::Instant::now();

    loop {
        let now = std::time::Instant::now();
        if std::time::Instant::now() >= next_announce {
            let _ = broadcast_once(&local);
            next_announce = std::time::Instant::now() + Duration::from_secs(15);
        }

        let change_was_reported = match clipboard.wait_for_change(Duration::from_millis(500)) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("clipboard wait error: {error}");
                false
            }
        };

        let forced_clipboard_check = now >= next_forced_clipboard_check;
        if forced_clipboard_check {
            next_forced_clipboard_check = now + Duration::from_secs(2);
        }

        match clipboard.change_count() {
            Ok(Some(change_count)) if last_change_count == Some(change_count) => {
                // Native sequence counters are authoritative. Do not open and
                // read the clipboard on a timer when nothing changed; on busy
                // Windows apps that needless polling can contend with Copy.
                continue;
            }
            Ok(Some(change_count)) => {
                last_change_count = Some(change_count);
            }
            Ok(None) if !change_was_reported && !forced_clipboard_check => continue,
            Ok(None) => {}
            Err(error) => eprintln!("clipboard change count error: {error}"),
        }

        match clipboard.read_text() {
            Ok(Some(text)) => {
                if last_seen_text.as_deref() == Some(text.as_str()) {
                    continue;
                }
                last_seen_text = Some(text.clone());
                if should_suppress(&clipboard_echo_guard, &text) {
                    continue;
                }
                broadcast_clipboard_text(&store_path, &local, latest_pending_text.clone(), text)?;
            }
            Ok(_) => last_seen_text = None,
            Err(error) => eprintln!("clipboard read error: {error}"),
        }
    }
}

fn acquire_daemon_instance_lock() -> io::Result<std::fs::File> {
    let lock_path = crate::config::daemon_pid_path()?.with_file_name("daemon-instance.lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    match FileExt::try_lock_exclusive(&lock) {
        Ok(()) => Ok(lock),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another span daemon is already running",
        )),
        Err(error) => Err(error),
    }
}

fn spawn_discovery_listener(
    store_path: std::path::PathBuf,
    local: LocalDevice,
    latest_pending_text: Arc<Mutex<Option<String>>>,
) {
    thread::spawn(move || {
        loop {
            if let Err(error) = listen_forever(|message, addr, listener| {
                match message {
                    DiscoveryMessage::Probe => {
                        // Reply directly to the scanner's ephemeral port. This
                        // makes `span discover` immediate without creating a
                        // broadcast storm between background daemons. Reuse the
                        // discovery listener so the reply has source port 46792;
                        // this keeps it inside the installed firewall rule.
                        respond_to_probe(listener, &local, addr)?;
                    }
                    DiscoveryMessage::Announcement(packet) => {
                        if packet.id == local.id {
                            return Ok(());
                        }

                        let endpoint = addr.ip().to_string();
                        let mut store = TrustStore::load(&store_path)?;
                        let mut info = packet.into_device_info();
                        info.endpoint = Some(endpoint.clone());
                        let changed = store.record_discovered(info.clone())?;
                        let trusted_now = store.trusted_devices().iter().any(|device| {
                            device.id == info.id
                                || (device.public_key.is_some()
                                    && device.public_key == info.public_key)
                        });
                        if trusted_now
                            && store.update_endpoint_and_key(
                                &info.id,
                                endpoint.clone(),
                                info.public_key.clone().unwrap_or_default(),
                            )?
                        {
                            println!("updated endpoint for {}: {endpoint}", info.name);
                        } else if changed && store.mark_pairing_prompted(&info)? {
                            println!("discovered device: {} ({})", info.name, info.id);
                            notify_gui_pairing_request(&info);
                        }
                        drop(store);

                        if trusted_now {
                            // Repeat the encrypted pairing acknowledgement when a
                            // trusted peer announces. This repairs one-sided trust
                            // created by older Span versions after either app is
                            // upgraded, without overriding an explicit revoke.
                            let _ = send_pairing_accept_to(&local, &info);
                            let pending = latest_pending_text
                                .lock()
                                .ok()
                                .and_then(|text| text.clone());
                            if let Some(text) = pending {
                                broadcast_clipboard_text(
                                    &store_path,
                                    &local,
                                    latest_pending_text.clone(),
                                    text,
                                )?;
                            }
                        }
                    }
                }
                Ok(())
            }) {
                eprintln!("discovery listener stopped: {error}; retrying");
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

fn notify_gui_pairing_request(device: &span_core::DeviceInfo) {
    // The daemon stays headless; GUI is responsible for showing prompts. When
    // the GUI is not running, launch the tiny companion GUI with a one-shot
    // pairing prompt. If the user ignores it, nothing is trusted and clipboard
    // data is not shared.
    let Ok(exe) = gui_executable_path() else {
        return;
    };
    if !exe.exists() {
        return;
    }

    let mut command = std::process::Command::new(exe);
    command
        .arg("pair")
        .arg(device.id.as_str())
        .arg(&device.name)
        .arg(platform_name(device.platform))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    detach_gui_command(&mut command);
    if let Err(error) = command.spawn() {
        eprintln!("could not open pairing prompt: {error}");
    }
}

#[cfg(unix)]
fn detach_gui_command(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(windows)]
fn detach_gui_command(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | DETACHED_PROCESS);
}

fn spawn_receiver(local: LocalDevice, clipboard_echo_guard: Arc<Mutex<ClipboardEchoGuard>>) {
    thread::spawn(move || {
        let trust_path = match trust_store_path() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("receiver trust path error: {error}");
                return;
            }
        };

        loop {
            if let Err(error) = receive_text_forever(|packet| {
                let mut store = TrustStore::load(&trust_path)?;
                let Some(sender) = store.device(&packet.from).cloned() else {
                    eprintln!("rejected packet from unknown device: {}", packet.from);
                    return Ok(());
                };

                let Some(sender_key) = sender.public_key.as_deref() else {
                    eprintln!("rejected text from device without key: {}", packet.from);
                    return Ok(());
                };

                let text = decrypt_text(&packet, &local.private_key, sender_key)?;
                if packet.kind == EncryptedPacketKind::PairingAccept && text == PAIRING_ACCEPT_TEXT
                {
                    if matches!(
                        sender.trust_state,
                        TrustState::Discovered | TrustState::Pending
                    ) {
                        store.trust_existing(&packet.from)?;
                        println!("pairing accepted by {}", sender.name);
                    }
                    return Ok(());
                }

                if packet.kind != EncryptedPacketKind::Text
                    || sender.trust_state != TrustState::Trusted
                {
                    eprintln!("rejected text from untrusted device: {}", packet.from);
                    return Ok(());
                }

                let mut clipboard = system_clipboard();
                write_remote_clipboard(&mut *clipboard, &clipboard_echo_guard, &text)?;
                println!("received text from {}", packet.from);
                Ok(())
            }) {
                eprintln!("receiver stopped: {error}; retrying");
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Confirm an explicit pairing choice to the peer. The proof is encrypted
/// with the same device keys used for clipboard traffic, so the peer can make
/// the trust relationship reciprocal without a second, confusing prompt.
pub(crate) fn notify_pairing_accept(device_id: &DeviceId) -> io::Result<()> {
    let local = load_or_create_local_device()?;
    let store_path = trust_store_path()?;
    refresh_trusted_endpoints(&store_path, &local, Duration::from_millis(500))?;
    let store = TrustStore::load(&store_path)?;
    let device = store
        .trusted_device(device_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "trusted device not found"))?;
    send_pairing_accept_to(&local, device)
}

fn send_pairing_accept_to(local: &LocalDevice, device: &span_core::DeviceInfo) -> io::Result<()> {
    let endpoint = device
        .endpoint
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "device is offline"))?;
    let public_key = device
        .public_key
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "device key is missing"))?;
    let packet = encrypt_pairing_accept(
        &local.id,
        &local.private_key,
        public_key,
        PAIRING_ACCEPT_TEXT,
    )?;
    send_text((endpoint, TEXT_PORT), &packet)
}

fn should_suppress(guard: &Arc<Mutex<ClipboardEchoGuard>>, text: &str) -> bool {
    let Ok(mut guard) = guard.lock() else {
        return false;
    };

    guard.should_suppress(text)
}

fn write_remote_clipboard(
    clipboard: &mut dyn Clipboard,
    guard: &Arc<Mutex<ClipboardEchoGuard>>,
    text: &str,
) -> io::Result<bool> {
    // A reflected packet with the same plain text must not rewrite the local
    // clipboard. On Windows/macOS that would discard rich-text, image and app-
    // specific formats even though the visible text did not change.
    if clipboard.read_text().ok().flatten().as_deref() == Some(text) {
        return Ok(false);
    }

    // Arm before touching the platform clipboard. Windows can deliver
    // WM_CLIPBOARDUPDATE immediately, before write_text returns.
    if let Ok(mut guard) = guard.lock() {
        guard.arm(text);
    }

    if let Err(error) = clipboard.write_text(text) {
        if let Ok(mut guard) = guard.lock() {
            guard.cancel(text);
        }
        return Err(error);
    }

    Ok(true)
}

#[cfg(test)]
mod clipboard_echo_tests {
    use super::*;

    struct TestClipboard {
        text: Option<String>,
        writes: usize,
        fail_write: bool,
        observe_guard_on_write: Option<Arc<Mutex<ClipboardEchoGuard>>>,
        saw_armed_guard: bool,
    }

    impl TestClipboard {
        fn with_text(text: Option<&str>) -> Self {
            Self {
                text: text.map(str::to_string),
                writes: 0,
                fail_write: false,
                observe_guard_on_write: None,
                saw_armed_guard: false,
            }
        }
    }

    impl Clipboard for TestClipboard {
        fn read_text(&mut self) -> io::Result<Option<String>> {
            Ok(self.text.clone())
        }

        fn write_text(&mut self, text: &str) -> io::Result<()> {
            self.writes += 1;
            if let Some(guard) = self.observe_guard_on_write.clone() {
                self.saw_armed_guard = should_suppress(&guard, text);
            }
            if self.fail_write {
                return Err(io::Error::other("simulated clipboard failure"));
            }
            self.text = Some(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn remote_marker_is_armed_before_platform_write_returns() {
        let guard = Arc::new(Mutex::new(ClipboardEchoGuard::default()));
        let mut clipboard = TestClipboard::with_text(Some("local"));
        clipboard.observe_guard_on_write = Some(guard.clone());

        assert!(write_remote_clipboard(&mut clipboard, &guard, "remote").unwrap());
        assert!(clipboard.saw_armed_guard);
        assert!(!should_suppress(&guard, "remote"));
    }

    #[test]
    fn reflected_same_text_does_not_destroy_existing_clipboard_formats() {
        let guard = Arc::new(Mutex::new(ClipboardEchoGuard::default()));
        let mut clipboard = TestClipboard::with_text(Some("same visible text"));

        assert!(!write_remote_clipboard(&mut clipboard, &guard, "same visible text").unwrap());
        assert_eq!(clipboard.writes, 0);
        assert!(!should_suppress(&guard, "same visible text"));
    }

    #[test]
    fn unrelated_local_copy_consumes_stale_remote_marker() {
        let guard = Arc::new(Mutex::new(ClipboardEchoGuard::default()));
        guard.lock().unwrap().arm("remote");

        assert!(!should_suppress(&guard, "new local copy"));
        assert!(!should_suppress(&guard, "remote"));
    }

    #[test]
    fn failed_platform_write_cancels_remote_marker() {
        let guard = Arc::new(Mutex::new(ClipboardEchoGuard::default()));
        let mut clipboard = TestClipboard::with_text(Some("local"));
        clipboard.fail_write = true;

        assert!(write_remote_clipboard(&mut clipboard, &guard, "remote").is_err());
        assert!(!should_suppress(&guard, "remote"));
    }
}

fn broadcast_clipboard_text(
    store_path: &std::path::Path,
    local: &LocalDevice,
    latest_pending_text: Arc<Mutex<Option<String>>>,
    text: String,
) -> io::Result<()> {
    if let Ok(mut pending) = latest_pending_text.lock() {
        *pending = Some(text.clone());
    }

    refresh_trusted_endpoints(store_path, local, Duration::from_millis(350))?;
    let store = TrustStore::load(store_path)?;
    let targets = store
        .trusted_devices()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut failed = false;

    for device in targets {
        let Some(endpoint) = device.endpoint.as_deref() else {
            continue;
        };

        let Some(public_key) = device.public_key.as_deref() else {
            continue;
        };

        let packet = encrypt_text(&local.id, &local.private_key, public_key, &text)?;

        match send_text((endpoint, TEXT_PORT), &packet) {
            Ok(()) => println!("sent text to {}", device.name),
            Err(error) => {
                eprintln!("send to {} at {endpoint} failed: {error}", device.name);
                if retry_send_after_discovery(store_path, local, &device, &packet)? {
                    println!("sent text to {} after discovery refresh", device.name);
                } else {
                    failed = true;
                }
            }
        }
    }

    if !failed {
        if let Ok(mut pending) = latest_pending_text.lock() {
            if pending.as_deref() == Some(text.as_str()) {
                *pending = None;
            }
        }
    }

    Ok(())
}

fn refresh_trusted_endpoints(
    store_path: &std::path::Path,
    local: &LocalDevice,
    timeout: Duration,
) -> io::Result<()> {
    let discovered = discover_once(local, timeout)?;
    if discovered.is_empty() {
        return Ok(());
    }

    let mut store = TrustStore::load(store_path)?;
    for (packet, addr) in discovered {
        let endpoint = addr.ip().to_string();
        for trusted in store
            .trusted_devices()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            let same_identity = trusted.id == packet.id
                || trusted
                    .public_key
                    .as_deref()
                    .map(|key| key == packet.public_key)
                    .unwrap_or(false);
            if !same_identity {
                continue;
            }
            let _ = store.update_endpoint_and_key(
                &trusted.id,
                endpoint.clone(),
                packet.public_key.clone(),
            )?;
        }
    }
    Ok(())
}

fn retry_send_after_discovery(
    store_path: &std::path::Path,
    local: &LocalDevice,
    device: &span_core::DeviceInfo,
    packet: &crate::transport::EncryptedTextPacket,
) -> io::Result<bool> {
    let Some(public_key) = device.public_key.as_deref() else {
        return Ok(false);
    };

    for (discovered, addr) in discover_once(local, Duration::from_secs(2))? {
        if discovered.id != device.id && discovered.public_key != public_key {
            continue;
        }

        let endpoint = addr.ip().to_string();
        let mut store = TrustStore::load(store_path)?;
        let _ =
            store.update_endpoint_and_key(&device.id, endpoint.clone(), public_key.to_string())?;

        match send_text((endpoint.as_str(), TEXT_PORT), packet) {
            Ok(()) => return Ok(true),
            Err(error) => eprintln!(
                "retry send to {} at {endpoint} failed: {error}",
                device.name
            ),
        }
    }

    Ok(false)
}

fn print_status() -> io::Result<()> {
    let local = load_or_create_local_device()?;
    let store = TrustStore::load(trust_store_path()?)?;

    println!("span: ready");
    println!("device: {} ({})", local.name, local.id);
    println!("platform: {}", platform_name(local.platform));
    println!("public key: {}", local.public_key_hex());
    println!("trusted targets: {}", store.trusted_devices().len());
    Ok(())
}

fn print_devices() -> io::Result<()> {
    let store = TrustStore::load(trust_store_path()?)?;
    let devices = store.devices();
    let trusted = broadcast_targets(devices);

    println!("trusted broadcast targets: {}", trusted.len());
    if devices.is_empty() {
        println!("no devices yet");
        return Ok(());
    }

    for device in devices {
        println!(
            "- {}\t{}\t{}\t{:?}\t{}",
            device.id,
            device.name,
            platform_name(device.platform),
            device.trust_state,
            device.endpoint.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn trust_device(args: Vec<String>) -> io::Result<()> {
    if args.len() == 1 {
        let Some(id) = DeviceId::new(args[0].clone()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty device id",
            ));
        };

        let mut store = TrustStore::load(trust_store_path()?)?;
        if store.trust_existing(&id)? {
            let label = store
                .device(&id)
                .map(|device| device.name.as_str())
                .unwrap_or(id.as_str());
            println!("trusted: {label} ({id})");
        } else {
            println!("not discovered: {id}");
            println!("usage: span trust <id> <name> <platform> [host] [public-key]");
        }
        return Ok(());
    }

    if args.len() < 3 || args.len() > 5 {
        println!("usage: span trust <id> <name> <platform> [host] [public-key]");
        return Ok(());
    }

    let id = &args[0];
    let name = &args[1];
    let platform = &args[2];
    let endpoint = args.get(3).cloned();
    let public_key = args.get(4).map(|value| value.to_lowercase());

    let Some(id) = DeviceId::new(id.clone()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty device id",
        ));
    };

    if let Some(public_key) = public_key.as_deref() {
        let Some(bytes) = decode_hex(public_key) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bad public key hex",
            ));
        };
        if bytes.len() != 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bad public key length",
            ));
        }
    }

    let platform = parse_platform(platform);
    let mut store = TrustStore::load(trust_store_path()?)?;
    store.trust(id, name.clone(), platform, endpoint.clone(), public_key)?;
    println!(
        "trusted: {name} ({}){}",
        platform_name(platform),
        endpoint
            .as_deref()
            .map(|value| format!(" at {value}"))
            .unwrap_or_default()
    );
    Ok(())
}

fn revoke_device(id: Option<String>) -> io::Result<()> {
    let Some(id) = id.and_then(DeviceId::new) else {
        println!("usage: span revoke <id>");
        return Ok(());
    };

    let mut store = TrustStore::load(trust_store_path()?)?;
    if store.revoke(&id)? {
        println!("revoked: {id}");
    } else {
        println!("not found: {id}");
    }
    Ok(())
}

fn reset_devices() -> io::Result<()> {
    let mut store = TrustStore::load(trust_store_path()?)?;
    store.reset()?;
    println!("trusted devices reset");
    Ok(())
}

fn discover_devices() -> io::Result<()> {
    let local = load_or_create_local_device()?;
    let devices = scan_devices(&local, Duration::from_secs(3))?;
    if devices.is_empty() {
        println!("No Span devices found on this network.");
        return Ok(());
    }

    println!("Found {} device(s):", devices.len());
    for (index, device) in devices.iter().enumerate() {
        println!(
            "  {}. {} ({}) - {}",
            index + 1,
            device.name,
            platform_name(device.platform),
            state_label(device.trust_state)
        );
    }

    if devices
        .iter()
        .any(|device| device.trust_state != TrustState::Trusted)
    {
        println!();
        println!("Accept a new device with: span accept <number>");
    }
    Ok(())
}

fn scan_devices(local: &LocalDevice, timeout: Duration) -> io::Result<Vec<span_core::DeviceInfo>> {
    // If the background daemon is already running, it owns UDP 46792. In
    // that case use its continuously refreshed store instead of competing for
    // the socket. This keeps the GUI and CLI usable without stopping the daemon.
    let mut store = TrustStore::load(trust_store_path()?)?;
    let mut live_ids = Vec::new();
    match discover_once(local, timeout) {
        Ok(packets) => {
            for (packet, addr) in packets {
                if packet.id == local.id {
                    continue;
                }
                live_ids.push(packet.id.clone());
                let mut info = packet.into_device_info();
                info.endpoint = Some(addr.ip().to_string());
                let _ = store.record_discovered(info)?;
            }
        }
        Err(error) => return Err(error),
    }
    let _ = store.compact()?;

    Ok(store
        .devices()
        .iter()
        .filter(|device| device.id != local.id && live_ids.contains(&device.id))
        .cloned()
        .collect())
}

fn announce_device() -> io::Result<()> {
    let local = load_or_create_local_device()?;
    broadcast_once(&local)?;
    println!("announced: {} ({})", local.name, local.id);
    Ok(())
}

fn install_span() -> io::Result<()> {
    let path = autostart::install()?;
    start_daemon()?;
    println!("Span installed. Background sync and auto-start are enabled.");
    println!("Auto-start: {}", path.display());
    Ok(())
}

fn restart_daemon() -> io::Result<()> {
    stop_daemon()?;
    thread::sleep(Duration::from_millis(200));
    #[cfg(target_os = "macos")]
    {
        // `stop` unloads the LaunchAgent so KeepAlive does not immediately
        // respawn it. Reinstall it before starting again instead of falling
        // back to an unmanaged standalone process.
        autostart::install()?;
    }
    start_daemon()
}

fn accept_device(selection: Option<String>) -> io::Result<()> {
    let local = load_or_create_local_device()?;
    let devices = scan_devices(&local, Duration::from_secs(3))?;
    let available = devices
        .iter()
        .filter(|device| device.trust_state != TrustState::Trusted)
        .collect::<Vec<_>>();

    if available.is_empty() {
        println!("No new devices found.");
        return Ok(());
    }

    if selection.is_none() && available.len() > 1 {
        println!("Devices waiting for acceptance:");
        for (index, device) in available.iter().enumerate() {
            println!(
                "  {}. {} ({})",
                index + 1,
                device.name,
                platform_name(device.platform)
            );
        }
        println!();
        println!("Accept one with: span accept <number>");
        return Ok(());
    }

    let selected = match selection {
        None => available.first().copied(),
        Some(selection) => selection
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| available.get(index).copied())
            .or_else(|| {
                available
                    .iter()
                    .copied()
                    .find(|device| device.id.as_str() == selection)
            }),
    };

    let Some(device) = selected else {
        println!("Device not found. Run `span accept` to see the numbered list.");
        return Ok(());
    };

    let mut store = TrustStore::load(trust_store_path()?)?;
    if store.trust_existing(&device.id)? {
        println!("Accepted: {}", device.name);
        println!("Clipboard sync is now allowed for this device.");
        let _ = notify_pairing_accept(&device.id);
    } else {
        println!("Device is no longer available. Run `span discover` and try again.");
    }
    Ok(())
}

fn send_command(args: Vec<String>) -> io::Result<()> {
    let text = if args.is_empty() {
        let mut clipboard = system_clipboard();
        let Some(text) = clipboard.read_text()? else {
            println!("Clipboard is empty or does not contain text.");
            return Ok(());
        };
        text
    } else {
        args.join(" ")
    };

    let local = load_or_create_local_device()?;
    let store_path = trust_store_path()?;
    let store = TrustStore::load(&store_path)?;
    let targets = store
        .trusted_devices()
        .into_iter()
        .filter(|device| device.endpoint.is_some() && device.public_key.is_some())
        .count();

    if targets == 0 {
        println!("No trusted devices are ready.");
        println!("Run `span discover`, then `span accept`.");
        return Ok(());
    }

    broadcast_clipboard_text(
        &store_path,
        &local,
        Arc::new(Mutex::new(None::<String>)),
        text,
    )?;
    println!("Sent to {targets} trusted device(s).");
    Ok(())
}

fn print_help() {
    println!("Span - cross-device clipboard");
    println!();
    println!("  span                         open device manager");
    println!("  span install                 install and enable auto-start");
    println!("  span start                   start background sync");
    println!("  span stop                    stop background sync");
    println!("  span restart                 restart background sync");
    println!("  span discover                find devices on this network");
    println!("  span accept [number|id]      accept a discovered device");
    println!("  span send [text]             send text, or current clipboard");
}

fn uninstall_autostart() -> io::Result<()> {
    stop_daemon()?;
    let path = autostart::uninstall()?;
    println!("removed autostart: {}", path.display());
    Ok(())
}

fn read_clipboard() -> io::Result<()> {
    let mut clipboard = system_clipboard();
    match clipboard.read_text()? {
        Some(text) => println!("{text}"),
        None => println!("clipboard is empty or not text"),
    }
    Ok(())
}

fn write_clipboard(args: Vec<String>) -> io::Result<()> {
    if args.is_empty() {
        println!("usage: span clip-write <text>");
        return Ok(());
    }

    let text = args.join(" ");
    let mut clipboard = system_clipboard();
    clipboard.write_text(&text)?;
    println!("clipboard updated");
    Ok(())
}

fn send_text_command(args: Vec<String>) -> io::Result<()> {
    let [host, text @ ..] = args.as_slice() else {
        println!("usage: span send-text <host> <text>");
        return Ok(());
    };

    if text.is_empty() {
        println!("usage: span send-text <host> <text>");
        return Ok(());
    }

    let local = load_or_create_local_device()?;
    let store = TrustStore::load(trust_store_path()?)?;
    let Some(peer) = store.devices().iter().find(|device| {
        device.trust_state == TrustState::Trusted
            && device.endpoint.as_deref() == Some(host.as_str())
    }) else {
        println!("no trusted device found for host {host}");
        return Ok(());
    };
    let Some(public_key) = peer.public_key.as_deref() else {
        println!("trusted device missing public key: {}", peer.name);
        return Ok(());
    };

    let packet = encrypt_text(&local.id, &local.private_key, public_key, &text.join(" "))?;
    send_text((host.as_str(), TEXT_PORT), &packet)?;
    println!("sent text to {host}:{TEXT_PORT}");
    Ok(())
}

fn receive_once() -> io::Result<()> {
    let local = load_or_create_local_device()?;
    let trust_path = trust_store_path()?;

    println!("waiting for text on port {TEXT_PORT}...");
    match receive_text_once(Duration::from_secs(30))? {
        Some(packet) => {
            if packet.kind != EncryptedPacketKind::Text {
                println!("received a pairing packet; no clipboard text was changed");
                return Ok(());
            }
            let store = TrustStore::load(&trust_path)?;
            let Some(trusted) = store.trusted_device(&packet.from) else {
                println!("untrusted sender: {}", packet.from);
                return Ok(());
            };
            let Some(sender_key) = trusted.public_key.as_deref() else {
                println!("missing sender key: {}", packet.from);
                return Ok(());
            };

            let text = decrypt_text(&packet, &local.private_key, sender_key)?;
            let mut clipboard = system_clipboard();
            clipboard.write_text(&text)?;
            println!("received text from {} and copied it", packet.from);
        }
        None => println!("no text received"),
    }
    Ok(())
}

fn state_label(state: TrustState) -> &'static str {
    match state {
        TrustState::Trusted => "Trusted",
        TrustState::Revoked => "Removed",
        TrustState::Blocked => "Blocked",
        TrustState::Pending => "Pending",
        TrustState::Discovered => "New",
    }
}

/// Open the native device manager window.
pub fn open_gui() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("pair") => {
            let id = args
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing device id"))?;
            let name = args.next().unwrap_or_else(|| "Span device".to_string());
            let platform = args.next().unwrap_or_else(|| "unknown".to_string());
            gui::prompt_pairing(&id, &name, &platform)
        }
        _ => gui::open(),
    }
}
