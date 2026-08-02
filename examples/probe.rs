// Live timing probe for the SSH/SCP path against a real Kindle.
// Run:  cargo run --example probe -- 192.168.15.244
use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Instant;

struct EmptyPrompt;
impl ssh2::KeyboardInteractivePrompt for EmptyPrompt {
    fn prompt(&mut self, _u: &str, _i: &str, prompts: &[ssh2::Prompt]) -> Vec<String> {
        prompts.iter().map(|_| String::new()).collect()
    }
}

fn main() {
    let ip = std::env::args().nth(1).unwrap_or_else(|| "192.168.15.244".into());
    let t0 = Instant::now();
    let tcp = TcpStream::connect((ip.as_str(), 22)).expect("tcp");
    // Replicate the app's (suspect) socket timeouts on a blocking session:
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();
    println!("tcp connect       : {:?}", t0.elapsed());

    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    let t = Instant::now();
    sess.handshake().expect("handshake");
    println!("handshake         : {:?}", t.elapsed());

    let t = Instant::now();
    let _ = sess.auth_methods("root");
    println!("auth_methods/none : {:?} (authed={})", t.elapsed(), sess.authenticated());
    if !sess.authenticated() {
        let t = Instant::now();
        let _ = sess.userauth_password("root", "");
        println!("password auth     : {:?} (authed={})", t.elapsed(), sess.authenticated());
    }
    if !sess.authenticated() {
        let t = Instant::now();
        let _ = sess.userauth_keyboard_interactive("root", &mut EmptyPrompt);
        println!("kbd-interactive   : {:?} (authed={})", t.elapsed(), sess.authenticated());
    }
    assert!(sess.authenticated(), "AUTH FAILED");

    let mb: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let size = mb * 1024 * 1024usize;
    let data = vec![7u8; size];
    let remote = "/mnt/us/documents/_probe (Author Name) (z-library.sk, 1lib.sk).epub";
    let t = Instant::now();
    let mut ch = sess
        .scp_send(Path::new(remote), 0o644, size as u64, None)
        .expect("scp_send open");
    println!("scp_send open     : {:?}", t.elapsed());
    let tw = Instant::now();
    let mut off = 0;
    while off < size {
        let end = (off + 65536).min(size);
        ch.write_all(&data[off..end]).expect("write");
        off = end;
    }
    println!("write 2MB         : {:?}", tw.elapsed());
    let tc = Instant::now();
    ch.send_eof().unwrap();
    ch.wait_eof().unwrap();
    ch.close().unwrap();
    ch.wait_close().unwrap();
    println!("eof+close         : {:?}", tc.elapsed());
    println!(">>> full 2MB upload: {:?}", t.elapsed());

    // cleanup
    let mut c = sess.channel_session().unwrap();
    c.exec(&format!("rm -f -- '{remote}'")).unwrap();
    let mut s = String::new();
    c.read_to_string(&mut s).ok();
    c.wait_close().ok();
    println!("(cleaned up probe file)  total {:?}", t0.elapsed());
}
