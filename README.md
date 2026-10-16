# Kindle Drop

A tiny desktop app for sideloading books onto a **jailbroken Kindle** over USB
networking. Drag books onto the window (or pick them with a file dialog) and they
stream straight to `/mnt/us/documents/`. It also brings up the USB network link
for you and lets you browse, download, and delete books already on the device.

Built in Rust with [egui](https://github.com/emilk/egui) + native SSH
([`ssh2`](https://crates.io/crates/ssh2) / libssh2).

<p align="center">
  <img src="docs/screenshot-send.png" width="420" alt="Send tab — drag-and-drop queue">
  &nbsp;
  <img src="docs/screenshot-library.png" width="420" alt="Library tab — manage books on the device">
</p>

## Features

- **Drag & drop** books onto the window, or use **Add files…** (native KDE/portal picker).
- **One-click connect** — auto-detects the Kindle's USB-ethernet interface, brings the
  link up, and assigns your host address via a single polkit (Linux) or UAC (Windows) prompt.
- **Live per-file progress bars** while sending (uses the legacy SCP protocol, the same
  thing `scp -O` forces, so it works with the Kindle's old dropbear SSH server).
- **Library tab** to manage what's already on the device: **download** books back to your
  PC and **delete** them (with a confirmation; also removes the `.sdr` reading-data sidecar).
  A **Books only** toggle hides non-book files (e.g. the KUAL launcher's `.azw2`).
- **Live connection status** — the pill flips to *Disconnected* when you unplug the cable, and
  **Connect** skips the privilege prompt when the Kindle is already reachable.
- **Restart KOReader** button (and an optional "after sending" auto-trigger) so newly sent books
  show up without touching the device — it stops the running instance and relaunches it the way
  KUAL does (`koreader.sh --kual`), daemonized so it survives the SSH session closing. The command
  is editable in Settings if your setup differs.
- Settings (Kindle IP, host address/CIDR, target dir, download folder) persist to a config file.

## What it replaces

The old manual dance:

```sh
# 1. figure out the interface, then assign an address (needs sudo)
ip link
sudo ip addr add 192.168.15.201/24 dev enp14s0u3

# 2. copy everything in the current dir to the Kindle (empty root password)
scp -O * root@192.168.15.244:/mnt/us/documents/
```

Both steps are now buttons.

## Install

### Windows

Download `kindle-drop-<version>-x86_64-windows.msi` from
[Releases](https://github.com/MykolaVaskevych/kindle-drop/releases) and run it. It installs to
`Program Files\Kindle Drop`, adds a Start-menu shortcut, and puts the folder on `PATH` so
`kindle-drop --send <file>` works from a terminal. Uninstall from *Settings → Apps*. A bare
`.exe` is attached too if you'd rather not install anything.

Windows only ships drivers for USB-ethernet gadgets that speak **RNDIS** or **NCM**. If the
Kindle shows up in Device Manager without a driver (it's in plain CDC-ECM mode), switch its USB
networking to RNDIS or install an ECM driver. When it works, the adapter appears in **⚙ Settings
→ Interface**. You can also set the address by hand (adapter → IPv4 → static
`192.168.15.201/24`). Connect then sees the Kindle straight away and skips the UAC prompt.

Windows' unsigned-app warning (SmartScreen) shows on first run, because the installer isn't
code-signed. Click *More info → Run anyway*.

### Linux

Grab the latest tarball from [Releases](https://github.com/MykolaVaskevych/kindle-drop/releases)
and run the installer — it drops the binary, icon, and desktop entry under `~/.local`, no root
needed:

```sh
tar xzf kindle-drop-*-x86_64-linux.tar.gz
cd kindle-drop-*-x86_64-linux
./install.sh
```

The prebuilt binary is x86_64 Linux with OpenSSL statically linked, so it runs on any
reasonably current distro. `./uninstall.sh` removes it again. Building from source works too —
see below.

There's no macOS build. The app compiles there, but you have to set up the USB link
yourself.

## Requirements

- Rust (2021+) and a jailbroken Kindle with **USB networking** enabled.
- System libraries (already present on most Arch installs): `libssh2`, `openssl`, `zlib`.
- Linux network setup: `polkit` (a running polkit agent, e.g.
  `polkit-kde-authentication-agent-1`), `pkexec`, and `iproute2` (`ip`).
- Assumes the Kindle's `root` account has **no password** (default on jailbroken units).

## Build & run

```sh
cargo build --release
./target/release/kindle-drop
```

On Windows, `packaging\windows\build-msi.ps1` builds the `.msi` and `.exe` into `dist\`. It
needs the MSVC Rust toolchain, Strawberry Perl, and the WiX v5 CLI (`dotnet tool install
--global wix --version 5.0.2`).

To reproduce the Linux release artifacts (portable binary + tarball in `dist/`), run
`packaging/build-release.sh --container` — it builds inside Debian bullseye via docker so
the result isn't tied to your distro's glibc. CI runs the same script on tag pushes; see
`.github/workflows/release.yml`.

## Usage

1. Plug the Kindle in with USB networking on.
2. Click **Connect** — approve the polkit / UAC prompt. The pill turns green when the Kindle's
   SSH server is reachable (`192.168.15.244:22`).
3. **Send tab:** drop books (or Add files…) → **Send to Kindle**.
4. **Library tab:** **Refresh** to list books on the device, then **Download** or **Delete**.

Adjust the Kindle IP, your host address/CIDR, the on-device target directory, and the
download folder under **⚙ Settings**. Config is stored at
`~/.config/kindle-drop/config.toml` (Windows: `%APPDATA%\kindle-drop\config.toml`).

### Headless / demo modes

- **Send from the terminal:** `kindle-drop --send <file>...` connects and uploads without opening
  the GUI (uses the Kindle IP / target dir from your config). Handy for scripts.
- **Demo:** `kindle-drop --demo` seeds the UI with sample data (no Kindle needed) — handy for
  screenshots or poking at the interface. Add `library` to start on the Library tab.

## How it works

- **Interface detection** scans `/sys/class/net/*` for a USB-ethernet gadget
  (`cdc_ether`/`cdc_ncm`/…), which is how the Kindle enumerates. On Windows it asks
  `Get-NetAdapter` for adapters whose description mentions RNDIS/NCM/CDC.
- **Network bring-up** runs `pkexec sh -c 'ip link set dev <iface> up && ip addr replace
  <cidr> dev <iface>'` — one prompt, and idempotent so it's safe to re-run. On Windows an
  elevated PowerShell (one UAC prompt) disables DHCP on the adapter and replaces its IPv4
  address with the configured one.
- **Transfers** open one authenticated SSH session per batch (empty-password auth with
  keyboard-interactive fallback) and stream each file with `scp_send` / `scp_recv`.

## Notes

- Deleting a book also removes its `<name>.sdr` sidecar directory.
- If a transfer fails, the row shows the error and the rest of the queue continues; press
  **Send** again to retry the failed items.

## License

[MIT](LICENSE).
