use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn start(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_tinyland"))
        .args(["--remote", "--width", "2", "--height", "1", "--socket", "wayland-test", "--", "/bin/sh", "-c", "echo $$ > \"$REMOTE_TEST_PID\"; exec sleep 60"])
        .env("XDG_RUNTIME_DIR", dir)
        .env("REMOTE_TEST_PID", dir.join("child.pid"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap()
}

fn child_pid(dir: &std::path::Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(pid) = std::fs::read_to_string(dir.join("child.pid")) { return pid.trim().parse().unwrap(); }
        assert!(Instant::now() < deadline, "child did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() { return status; }
        if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); panic!("compositor did not exit"); }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn remote_eof_reaps_the_command_and_reserves_stdout_for_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = start(dir.path());
    let message = tinyland_remote::read_compositor(child.stdout.as_mut().unwrap(), None).unwrap().unwrap();
    assert_eq!(message, tinyland_remote::CompositorMessage::Ready { size: tinyland_remote::Size::new(2, 1).unwrap() });
    let pid = child_pid(dir.path());
    drop(child.stdin.take());
    assert!(!wait(&mut child).success());
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    let mut rest = Vec::new(); child.stdout.take().unwrap().read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}

#[test]
fn remote_signal_reaps_the_command() {
    use std::os::unix::process::ExitStatusExt;
    let dir = tempfile::tempdir().unwrap();
    let mut child = start(dir.path());
    let _ = tinyland_remote::read_compositor(child.stdout.as_mut().unwrap(), None).unwrap().unwrap();
    let pid = child_pid(dir.path());
    assert!(Command::new("kill").args(["-TERM", &child.id().to_string()]).status().unwrap().success());
    assert_eq!(wait(&mut child).signal(), Some(15));
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}

#[test]
fn remote_rejects_conflicting_modes_before_opening_devices() {
    let bin = env!("CARGO_BIN_EXE_tinyland");
    for args in [&["--remote", "--seat", "--", "true"][..], &["--remote", "--drm-proof", "--", "true"], &["--remote", "--width", "4097", "--", "true"], &["--remote"]] {
        let out = Command::new(bin).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn stalled_partial_host_message_ends_the_session_and_reaps_the_command() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let mut child = start(dir.path());
    let _ = tinyland_remote::read_compositor(child.stdout.as_mut().unwrap(), None).unwrap().unwrap();
    let pid = child_pid(dir.path());
    child.stdin.as_mut().unwrap().write_all(b"T").unwrap();
    assert!(!wait(&mut child).success());
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}
