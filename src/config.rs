use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted settings. Defaults match the manual workflow the app replaces.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Kindle's IP on the USB network.
    pub kindle_ip: String,
    /// Address/CIDR assigned to *this* machine's USB-ethernet interface.
    pub host_cidr: String,
    /// Where books land on the device.
    pub target_dir: String,
    /// Last interface used, so it can be preselected next launch.
    pub last_iface: Option<String>,
    /// Folder on this PC where downloaded books are saved.
    pub download_dir: Option<String>,
    /// Command run on the Kindle to (re)launch KOReader so it rescans its library.
    pub koreader_cmd: String,
    /// Automatically run `koreader_cmd` after a successful send batch.
    pub koreader_after_send: bool,
}

/// Default: **restart** KOReader so it rescans its library — stop the running
/// instance (SIGTERM, clean save), wait for its launcher to hand back, then
/// relaunch the way KUAL does (`koreader.sh --kual`), daemonized via
/// start-stop-daemon so it survives the SSH session closing. Works whether or
/// not KOReader is currently running. Verified end-to-end (~3.3s) on-device.
pub const DEFAULT_KOREADER_CMD: &str = r#"pids=$(ps | grep reader.lua | grep -v grep | awk '{print $1}'); if [ -n "$pids" ]; then kill $pids 2>/dev/null; i=0; while [ $i -lt 6 ]; do ps | grep koreader.sh | grep -v grep >/dev/null 2>&1 || break; sleep 1; i=$((i+1)); done; sleep 2; fi; start-stop-daemon -S -b -x /mnt/us/koreader/koreader.sh -- --kual; echo restarted"#;

impl Config {
    /// Effective download folder: configured value, else the OS Downloads dir,
    /// else the home directory.
    pub fn download_path(&self) -> std::path::PathBuf {
        self.download_dir
            .as_ref()
            .map(std::path::PathBuf::from)
            .or_else(dirs::download_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            kindle_ip: "192.168.15.244".into(),
            host_cidr: "192.168.15.201/24".into(),
            target_dir: "/mnt/us/documents/".into(),
            last_iface: None,
            download_dir: None,
            koreader_cmd: DEFAULT_KOREADER_CMD.to_string(),
            koreader_after_send: false,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("kindle-drop").join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(p) = Self::path() else {
            return Self::default();
        };
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&p, s);
        }
    }
}
