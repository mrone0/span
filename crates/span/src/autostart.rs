use std::fs;
use std::io;
use std::path::PathBuf;

use crate::config::cli_executable_path;
#[cfg(target_os = "windows")]
use crate::config::gui_executable_path;

pub fn install() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return install_macos_launch_agent();
    }

    #[cfg(target_os = "windows")]
    {
        return install_windows_startup_script();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        install_linux_desktop_entry()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "autostart unsupported on this platform",
        ))
    }
}

pub fn uninstall() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return remove_macos_launch_agent();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("schtasks")
            .args(["/Delete", "/TN", "Span", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        return remove_file_if_exists(windows_startup_script_path()?);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        remove_file_if_exists(linux_desktop_entry_path()?)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "autostart unsupported on this platform",
        ))
    }
}

#[cfg(target_os = "macos")]
fn install_macos_launch_agent() -> io::Result<PathBuf> {
    let path = macos_launch_agent_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = cli_executable_path()?;
    // Keep a stable fingerprint in the plist. The GUI calls `install()` on
    // every launch, so comparing the complete plist lets us leave a healthy
    // daemon alone while still restarting it once after the app is upgraded
    // in place (where the executable path itself does not change).
    let executable_fingerprint = executable_fingerprint(&exe)?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.span.daemon</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>SPAN_EXECUTABLE_FINGERPRINT</key>
    <string>{}</string>
  </dict>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>ThrottleInterval</key>
  <integer>5</integer>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        escape_xml(&exe.display().to_string()),
        escape_xml(&executable_fingerprint),
        escape_xml(&crate::config::daemon_log_path()?.display().to_string()),
        escape_xml(&crate::config::daemon_log_path()?.display().to_string())
    );

    let plist_changed = fs::read_to_string(&path)
        .map(|existing| existing != plist)
        .unwrap_or(true);
    if plist_changed {
        fs::write(&path, &plist)?;
    }

    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let service = format!("{domain}/com.span.daemon");
    let mut state = launchctl_state(&service);

    // Do not tear down a healthy LaunchAgent every time the GUI opens. It only
    // needs to be reloaded when its generated plist changed (including after
    // an in-place app upgrade).
    if plist_changed && state != LaunchAgentState::NotLoaded {
        let status = std::process::Command::new("launchctl")
            .args(["bootout", &service])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        state = launchctl_state(&service);
        if !status.success() && state != LaunchAgentState::NotLoaded {
            return Err(io::Error::other(format!(
                "launchctl bootout failed with {status}"
            )));
        }
    }

    if state == LaunchAgentState::NotLoaded {
        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain, path.to_string_lossy().as_ref()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() && launchctl_state(&service) == LaunchAgentState::NotLoaded {
            return Err(io::Error::other(format!(
                "launchctl bootstrap failed with {status}"
            )));
        }

        // RunAtLoad normally starts the process, but bootstrap only guarantees
        // that the job is loaded. Kick it explicitly so opening Span always
        // leaves a listening daemon behind.
        kickstart_service(&service)?;
    } else if state == LaunchAgentState::Loaded {
        // A loaded job is not necessarily a running job. This is the state we
        // previously mistook for success after a failed/omitted launch.
        kickstart_service(&service)?;
    }

    Ok(path)
}

#[cfg(target_os = "macos")]
fn remove_macos_launch_agent() -> io::Result<PathBuf> {
    let path = macos_launch_agent_path()?;
    let service = launch_agent_service();
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &service])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    remove_file_if_exists(path)
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_path() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    Ok(home.join("Library/LaunchAgents/com.span.daemon.plist"))
}

#[cfg(target_os = "windows")]
fn install_windows_startup_script() -> io::Result<PathBuf> {
    let path = windows_startup_script_path()?;
    let exe = cli_executable_path()?;
    let gui = gui_executable_path()?;
    let task_command = if gui.exists() {
        format!("\"{}\" --hidden", gui.to_string_lossy())
    } else {
        format!("\"{}\" run", exe.to_string_lossy())
    };

    let status = std::process::Command::new("schtasks")
        .args([
            "/Create",
            "/SC",
            "ONLOGON",
            "/TN",
            "Span",
            "/TR",
            &task_command,
            "/RL",
            "LIMITED",
            "/IT",
            "/F",
        ])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "schtasks create failed with {status}"
        )));
    }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn windows_startup_script_path() -> io::Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA not set"))?;
    Ok(appdata.join("Microsoft/Windows/Start Menu/Programs/Startup/span.cmd"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_linux_desktop_entry() -> io::Result<PathBuf> {
    let path = linux_desktop_entry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = cli_executable_path()?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=span\nNoDisplay=true\nTerminal=false\nStartupNotify=false\nExec={} run\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    fs::write(&path, entry)?;
    Ok(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_desktop_entry_path() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory not found"))?;
    Ok(base.join("autostart/span.desktop"))
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LaunchAgentState {
    NotLoaded,
    Loaded,
    Running,
}

#[cfg(target_os = "macos")]
fn launch_agent_service() -> String {
    format!("gui/{}/com.span.daemon", unsafe { libc::getuid() })
}

#[cfg(target_os = "macos")]
fn launchctl_state(service: &str) -> LaunchAgentState {
    let output = match std::process::Command::new("launchctl")
        .args(["print", service])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return LaunchAgentState::NotLoaded,
    };

    if launchctl_output_is_running(&output.stdout) {
        LaunchAgentState::Running
    } else {
        LaunchAgentState::Loaded
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_launch_agent_state() -> LaunchAgentState {
    launchctl_state(&launch_agent_service())
}

#[cfg(target_os = "macos")]
pub(crate) fn kickstart_macos_launch_agent() -> io::Result<()> {
    kickstart_service(&launch_agent_service())
}

#[cfg(target_os = "macos")]
fn kickstart_service(service: &str) -> io::Result<()> {
    let status = std::process::Command::new("launchctl")
        .args(["kickstart", service])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "launchctl kickstart failed with {status}"
        )));
    }

    // launchctl can return just before the child has transitioned to running.
    // Give that transition a short bounded window, while still surfacing a
    // daemon that immediately fails to launch.
    for attempt in 0..20 {
        if launchctl_state(service) == LaunchAgentState::Running {
            return Ok(());
        }
        if attempt < 19 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    Err(io::Error::other(
        "launchctl kickstart completed but span daemon is not running",
    ))
}

#[cfg(target_os = "macos")]
fn launchctl_output_is_running(output: &[u8]) -> bool {
    String::from_utf8_lossy(output).lines().any(|line| {
        let line = line.trim();
        line == "state = running"
            || line
                .strip_prefix("pid = ")
                .is_some_and(|pid| pid.parse::<u32>().is_ok_and(|pid| pid > 0))
    })
}

#[cfg(target_os = "macos")]
fn executable_fingerprint(path: &std::path::Path) -> io::Result<String> {
    use std::time::UNIX_EPOCH;

    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    Ok(format!(
        "{}-{}-{}",
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    ))
}

fn remove_file_if_exists(path: PathBuf) -> io::Result<PathBuf> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::launchctl_output_is_running;

    #[test]
    fn launchctl_running_state_is_recognized() {
        let output = b"gui/501/com.span.daemon = {\n\tstate = running\n\tpid = 123\n}";
        assert!(launchctl_output_is_running(output));
    }

    #[test]
    fn loaded_but_stopped_state_is_not_running() {
        let output = b"gui/501/com.span.daemon = {\n\tstate = not running\n\tlast exit code = 0\n}";
        assert!(!launchctl_output_is_running(output));
    }

    #[test]
    fn zero_pid_is_not_running() {
        let output = b"gui/501/com.span.daemon = {\n\tstate = not running\n\tpid = 0\n}";
        assert!(!launchctl_output_is_running(output));
    }
}
