use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ssh2::Session;

use crate::model::{FileJob, JobStatus, RemoteFile};

/// Answers every keyboard-interactive challenge with an empty string — the
/// Kindle's root account has no password.
struct EmptyPrompt;
impl ssh2::KeyboardInteractivePrompt for EmptyPrompt {
    fn prompt(&mut self, _u: &str, _i: &str, prompts: &[ssh2::Prompt]) -> Vec<String> {
        prompts.iter().map(|_| String::new()).collect()
    }
}

/// Open an authenticated SSH session, trying every empty-credential method the
/// Kindle's dropbear might expose: "none", empty password, then empty
/// keyboard-interactive.
fn connect(ip: &str, user: &str) -> Result<Session, String> {
    let tcp = TcpStream::connect((ip, 22)).map_err(|e| format!("connect {ip}:22 failed: {e}"))?;

    let mut sess = Session::new().map_err(|e| format!("ssh session: {e}"))?;
    // libssh2's own blocking timeout — do NOT set SO_RCVTIMEO/SO_SNDTIMEO on a
    // blocking session (that can stall or spuriously error mid-transfer). This
    // makes a dropped USB link surface as an error instead of hanging forever.
    sess.set_timeout(20_000);
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| format!("ssh handshake: {e}"))?;

    // "none" auth is attempted as a side effect of listing auth methods.
    let _ = sess.auth_methods(user);
    if !sess.authenticated() {
        let _ = sess.userauth_password(user, "");
    }
    if !sess.authenticated() {
        let _ = sess.userauth_keyboard_interactive(user, &mut EmptyPrompt);
    }
    if !sess.authenticated() {
        return Err("authentication failed (empty password rejected)".into());
    }
    Ok(sess)
}

/// Upload one file over the legacy SCP protocol (what `scp -O` forces), updating
/// the job's byte counter as it streams.
fn upload_one(sess: &Session, job: &FileJob, target_dir: &str) -> Result<(), String> {
    let remote = format!("{}/{}", target_dir.trim_end_matches('/'), job.name);
    let mut file =
        std::fs::File::open(&job.path).map_err(|e| format!("open {}: {e}", job.name))?;
    let mut chan = sess
        .scp_send(Path::new(&remote), 0o644, job.size, None)
        .map_err(|e| format!("scp_send: {e}"))?;

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        chan.write_all(&buf[..n]).map_err(|e| format!("send: {e}"))?;
        job.sent.fetch_add(n as u64, Ordering::Relaxed);
    }

    chan.send_eof().map_err(|e| format!("eof: {e}"))?;
    chan.wait_eof().map_err(|e| format!("eof ack: {e}"))?;
    chan.close().map_err(|e| format!("close: {e}"))?;
    chan.wait_close().map_err(|e| format!("close ack: {e}"))?;
    Ok(())
}

/// Worker entry point: connect once, then upload each job in turn. Runs on a
/// background thread; `active` gates the UI's "in progress" state and `ctx`
/// drives repaints so progress bars animate.
pub fn run_batch(
    ip: String,
    user: String,
    target_dir: String,
    jobs: Vec<Arc<FileJob>>,
    ctx: egui::Context,
    active: Arc<AtomicBool>,
) {
    active.store(true, Ordering::SeqCst);

    let sess = match connect(&ip, &user) {
        Ok(s) => s,
        Err(e) => {
            for j in &jobs {
                j.set_status(JobStatus::Error(e.clone()));
            }
            active.store(false, Ordering::SeqCst);
            ctx.request_repaint();
            return;
        }
    };

    for j in &jobs {
        j.set_status(JobStatus::Sending);
        ctx.request_repaint();
        match upload_one(&sess, j, &target_dir) {
            Ok(()) => {
                j.sent.store(j.size, Ordering::Relaxed);
                j.set_status(JobStatus::Done);
            }
            Err(e) => j.set_status(JobStatus::Error(e)),
        }
        ctx.request_repaint();
    }

    active.store(false, Ordering::SeqCst);
    ctx.request_repaint();
}

/// Run an arbitrary command on the Kindle (used to relaunch KOReader). Returns
/// the command's stdout on success.
pub fn run_remote_cmd(ip: &str, user: &str, cmd: &str) -> Result<String, String> {
    let sess = connect(ip, user)?;
    let (out, code) = exec(&sess, cmd)?;
    if code == 0 {
        Ok(out)
    } else if out.trim().is_empty() {
        Err(format!("command exited {code}"))
    } else {
        Err(out.trim().to_string())
    }
}

/// Headless upload used by the `--send` CLI mode. Exercises the exact same
/// connect + upload path as the GUI. Returns a process exit code.
pub fn cli_upload(ip: &str, user: &str, dir: &str, paths: &[std::path::PathBuf]) -> i32 {
    let sess = match connect(ip, user) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect to {ip} failed: {e}");
            return 1;
        }
    };
    let mut rc = 0;
    for p in paths {
        let Some(job) = FileJob::new(p.clone()) else {
            eprintln!("skip (not a readable file): {}", p.display());
            rc = 1;
            continue;
        };
        print!("sending {} ({} bytes) … ", job.name, job.size);
        let _ = std::io::stdout().flush();
        let t = std::time::Instant::now();
        match upload_one(&sess, &job, dir) {
            Ok(()) => println!("ok ({:?})", t.elapsed()),
            Err(e) => {
                println!("FAILED: {e}");
                rc = 1;
            }
        }
    }
    rc
}

// ---- remote management (list / delete / download) ------------------------

/// Wrap a string in single quotes, safe to splice into a remote shell command.
fn shq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Run a command on the Kindle, returning (combined stdout, exit code).
fn exec(sess: &Session, cmd: &str) -> Result<(String, i32), String> {
    let mut chan = sess.channel_session().map_err(|e| format!("channel: {e}"))?;
    chan.exec(cmd).map_err(|e| format!("exec: {e}"))?;
    let mut out = String::new();
    chan.read_to_string(&mut out).map_err(|e| format!("read: {e}"))?;
    chan.send_eof().ok();
    chan.wait_close().ok();
    let code = chan.exit_status().unwrap_or(-1);
    Ok((out, code))
}

/// List regular files (name + size) in the Kindle's target directory. Uses a
/// busybox-safe shell loop so it works on the device's minimal `sh`.
pub fn list_remote(ip: &str, user: &str, dir: &str) -> Result<Vec<RemoteFile>, String> {
    let sess = connect(ip, user)?;
    // Sizes via `stat` (a metadata read) — NOT `wc -c`, which would read every
    // file end-to-end. Falls back to `wc -c` only if `stat` is unavailable.
    let cmd = format!(
        "cd {d} 2>/dev/null && for f in *; do [ -f \"$f\" ] && printf '%s\\t%s\\n' \"$(stat -c %s \"$f\" 2>/dev/null || wc -c < \"$f\")\" \"$f\"; done",
        d = shq(dir)
    );
    let (out, _code) = exec(&sess, &cmd)?;
    let mut files = Vec::new();
    for line in out.lines() {
        if let Some((size, name)) = line.split_once('\t') {
            // Tolerate an unparseable size (e.g. no `stat`): still list the file.
            let sz = size.trim().parse::<u64>().unwrap_or(0);
            files.push(RemoteFile::new(name.to_string(), sz));
        }
    }
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(files)
}

/// Delete a book from the Kindle, along with its `<stem>.sdr` sidecar directory
/// (Kindle's per-book reading data) if present.
pub fn delete_remote(ip: &str, user: &str, dir: &str, name: &str) -> Result<(), String> {
    let sess = connect(ip, user)?;
    let dir = dir.trim_end_matches('/');
    let path = format!("{dir}/{name}");
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let sdr = format!("{dir}/{stem}.sdr");
    let cmd = format!("rm -f -- {} ; rm -rf -- {}", shq(&path), shq(&sdr));
    let (out, code) = exec(&sess, &cmd)?;
    if code == 0 {
        Ok(())
    } else if out.trim().is_empty() {
        Err(format!("delete failed (exit {code})"))
    } else {
        Err(out.trim().to_string())
    }
}

/// Download a file from the Kindle to `dest_dir`, updating `file.got` as it
/// streams. Uses the legacy SCP protocol (matches `scp -O`).
pub fn download_remote(
    ip: &str,
    user: &str,
    dir: &str,
    file: &RemoteFile,
    dest_dir: &Path,
) -> Result<std::path::PathBuf, String> {
    let sess = connect(ip, user)?;
    let remote = format!("{}/{}", dir.trim_end_matches('/'), file.name);
    let (mut chan, _stat) = sess
        .scp_recv(Path::new(&remote))
        .map_err(|e| format!("scp_recv: {e}"))?;

    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
    let dest = dest_dir.join(&file.name);
    let mut out =
        std::fs::File::create(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = chan.read(&mut buf).map_err(|e| format!("recv: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
        file.got.fetch_add(n as u64, Ordering::Relaxed);
    }

    chan.send_eof().ok();
    chan.wait_eof().ok();
    chan.close().ok();
    chan.wait_close().ok();
    Ok(dest)
}
