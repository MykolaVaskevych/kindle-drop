use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Kernel drivers used by USB-ethernet gadgets (Kindle enumerates as `cdc_ether`).
const USB_ETH_DRIVERS: &[&str] = &["cdc_ether", "cdc_ncm", "cdc_subset", "rndis_host", "cdc_eem"];

/// Scan `/sys/class/net` for USB-ethernet interfaces — the Kindle's gadget link.
/// Read-only; needs no privileges. On the dev machine this returns `enp14s0u3`.
pub fn detect_candidates() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let base = Path::new("/sys/class/net").join(&name);

        // Must be a real ethernet link (ARPHRD_ETHER == 1).
        let is_ether = std::fs::read_to_string(base.join("type"))
            .map(|s| s.trim() == "1")
            .unwrap_or(false);
        if !is_ether {
            continue;
        }

        // Driver behind the device must be a USB-ethernet gadget driver.
        let driver = std::fs::canonicalize(base.join("device").join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .unwrap_or_default();
        if USB_ETH_DRIVERS.contains(&driver.as_str()) {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// True when an argument is safe to splice into the `pkexec sh -c` string.
fn safe_token(s: &str, extra: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// Bring the interface up and assign our host address, in a single privileged
/// call (one polkit password prompt). `ip addr replace` is idempotent, so this
/// is safe to run repeatedly.
pub fn bring_up(iface: &str, host_cidr: &str) -> Result<(), String> {
    if !safe_token(iface, ".-_") {
        return Err(format!("suspicious interface name: {iface:?}"));
    }
    if !safe_token(host_cidr, "./:") {
        return Err(format!("suspicious address/CIDR: {host_cidr:?}"));
    }

    let script = format!("ip link set dev {iface} up && ip addr replace {host_cidr} dev {iface}");
    let output = Command::new("pkexec")
        .args(["sh", "-c", &script])
        .output()
        .map_err(|e| format!("failed to launch pkexec: {e}"))?;

    if output.status.success() {
        return Ok(());
    }
    let code = output.status.code().unwrap_or(-1);
    let msg = match code {
        126 => "authorization dialog dismissed".to_string(),
        127 => "authentication failed".to_string(),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            if stderr.is_empty() {
                format!("exit code {code}")
            } else {
                stderr.to_string()
            }
        }
    };
    Err(format!("network setup failed: {msg}"))
}

/// Can we open a TCP connection to `ip:port`? Used to confirm the Kindle's SSH
/// server is reachable after the link is up.
pub fn reachable(ip: &str, port: u16, timeout: Duration) -> bool {
    format!("{ip}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .map(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
        .unwrap_or(false)
}
