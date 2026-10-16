use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[cfg(target_os = "linux")]
pub use linux::{bring_up, detect_candidates};
#[cfg(not(any(target_os = "linux", windows)))]
pub use unsupported::{bring_up, detect_candidates};
#[cfg(windows)]
pub use windows::{bring_up, detect_candidates};

/// True when an argument is safe to splice into a privileged shell/script string.
#[cfg(any(target_os = "linux", windows))]
fn safe_token(s: &str, extra: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::safe_token;
    use std::path::Path;
    use std::process::Command;

    /// Kernel drivers used by USB-ethernet gadgets (Kindle enumerates as `cdc_ether`).
    const USB_ETH_DRIVERS: &[&str] = &[
        "cdc_ether",
        "cdc_ncm",
        "cdc_subset",
        "rndis_host",
        "cdc_eem",
    ];

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

        let script =
            format!("ip link set dev {iface} up && ip addr replace {host_cidr} dev {iface}");
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

#[cfg(windows)]
mod windows {
    use super::safe_token;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// Keep PowerShell from flashing a console window over the GUI.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Adapter descriptions Windows gives USB-ethernet gadgets: the in-box RNDIS
    /// and NCM drivers, plus vendor CDC/gadget drivers.
    const USB_ETH_PATTERN: &str = "RNDIS|NCM|CDC|Gadget|Kindle";

    /// Run a PowerShell script passed as `-EncodedCommand`, which sidesteps every
    /// layer of command-line quoting between us and PowerShell.
    fn powershell(script: &str) -> std::io::Result<std::process::Output> {
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .args(["-EncodedCommand", &encode_ps(script)])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    /// Base64 of the UTF-16LE script — the format `-EncodedCommand` expects.
    fn encode_ps(script: &str) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// List network adapters that look like a USB-ethernet gadget — the Kindle's
    /// link. Read-only; needs no elevation.
    pub fn detect_candidates() -> Vec<String> {
        let script = format!(
            "Get-NetAdapter | Where-Object {{ $_.InterfaceDescription -match '{USB_ETH_PATTERN}' }} \
             | ForEach-Object {{ $_.Name }}"
        );
        let Ok(output) = powershell(&script) else {
            return Vec::new();
        };
        let mut out: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        out.sort();
        out
    }

    /// Give the adapter a static IPv4 address in one elevated step (one UAC
    /// prompt). Existing IPv4 addresses on it are replaced, so this is safe to
    /// run repeatedly.
    pub fn bring_up(iface: &str, host_cidr: &str) -> Result<(), String> {
        if !safe_token(iface, " .-_()#") {
            return Err(format!("suspicious interface name: {iface:?}"));
        }
        let (ip, prefix) = host_cidr.split_once('/').unwrap_or((host_cidr, "24"));
        if ip.parse::<std::net::Ipv4Addr>().is_err() || !matches!(prefix.parse::<u8>(), Ok(0..=32))
        {
            return Err(format!("expected an IPv4 address/CIDR, got {host_cidr:?}"));
        }

        // Runs elevated. Any failure is a terminating error -> non-zero exit.
        let elevated = format!(
            "$ErrorActionPreference = 'Stop'\n\
             $a = '{iface}'\n\
             Enable-NetAdapter -Name $a -Confirm:$false -ErrorAction SilentlyContinue\n\
             Set-NetIPInterface -InterfaceAlias $a -AddressFamily IPv4 -Dhcp Disabled\n\
             Get-NetIPAddress -InterfaceAlias $a -AddressFamily IPv4 -ErrorAction SilentlyContinue \
               | Remove-NetIPAddress -Confirm:$false\n\
             New-NetIPAddress -InterfaceAlias $a -AddressFamily IPv4 -IPAddress '{ip}' -PrefixLength {prefix} | Out-Null\n"
        );
        // Unelevated launcher: Start-Process -Verb RunAs shows the UAC prompt and
        // throws if it is dismissed.
        let launcher = format!(
            "try {{\n\
               $p = Start-Process -FilePath powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru \
                 -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{}'\n\
               exit $p.ExitCode\n\
             }} catch {{ exit 1223 }}\n",
            encode_ps(&elevated)
        );
        let output =
            powershell(&launcher).map_err(|e| format!("failed to launch PowerShell: {e}"))?;

        match output.status.code() {
            Some(0) => Ok(()),
            Some(1223) => Err("administrator prompt dismissed".into()),
            code => Err(format!(
                "network setup failed (exit code {}) — check the adapter name and that \
                 \"{iface}\" is the Kindle",
                code.unwrap_or(-1)
            )),
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn encodes_utf16le_base64() {
            // "ab" -> 61 00 62 00
            assert_eq!(super::encode_ps("ab"), "YQBiAA==");
            assert_eq!(super::encode_ps("a"), "YQA=");
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported {
    pub fn detect_candidates() -> Vec<String> {
        Vec::new()
    }

    pub fn bring_up(_iface: &str, _host_cidr: &str) -> Result<(), String> {
        Err(
            "automatic network setup is not supported on this OS — configure the \
             USB link by hand"
                .into(),
        )
    }
}
