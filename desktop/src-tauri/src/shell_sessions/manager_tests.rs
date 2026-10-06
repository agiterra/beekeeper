use super::*;

fn state_with(bytes: &[u8], total: u64) -> SharedState {
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(bytes);
    SharedState {
        scrollback: bytes.to_vec(),
        total,
        last_output_at: None,
        parser,
    }
}

#[test]
fn default_shell_is_never_empty() {
    assert!(!default_shell().is_empty());
}

#[test]
fn missing_session_errors_are_descriptive() {
    let err = write("nope", b"x").unwrap_err();
    assert!(err.contains("not found"), "unexpected error: {err}");
    let err = snapshot("nope", false).unwrap_err();
    assert!(err.contains("not found"), "unexpected error: {err}");
    let err = cursor("nope").unwrap_err();
    assert!(err.contains("not found"), "unexpected error: {err}");
}

#[test]
fn output_since_returns_only_new_bytes() {
    let st = state_with(b"hello world", 11);
    let (text, cursor, truncated) = output_since(&st, 6);
    assert_eq!(text, "world");
    assert_eq!(cursor, 11);
    assert!(!truncated);
}

#[test]
fn output_since_flags_truncation_past_dropped_front() {
    // total 100 but only the last 11 bytes retained → offsets < 89 dropped.
    let st = state_with(b"hello world", 100);
    let (text, cursor, truncated) = output_since(&st, 10);
    assert!(
        truncated,
        "offset before retained buffer must flag truncation"
    );
    assert_eq!(text, "hello world");
    assert_eq!(cursor, 100);
}

#[test]
fn rendered_output_strips_ansi_and_keeps_line_breaks() {
    // Colored `ls`-style output: color codes gone, rows intact. A PTY's
    // output discipline (ONLCR) emits CR-LF between rows, which vt100
    // renders as a real column-0 line break; it trims the trailing one.
    let raw = b"\x1b[34mDesktop\x1b[0m\r\n\x1b[34mtankloop\x1b[0m\r\n";
    let st = state_with(raw, raw.len() as u64);
    let (text, _, _) = output_since(&st, 0);
    assert_eq!(text, "Desktop\ntankloop");
}

#[test]
fn columns_spaced_with_cursor_forward_keep_their_gaps() {
    // Regression: `ls` lays out columns with cursor-forward escapes, not
    // literal spaces. Naive ANSI-stripping collapsed the gap
    // ("Agiterralearninggodot01"); the emulator preserves it.
    let raw = b"Agiterra\x1b[4Clearninggodot01\n";
    let st = state_with(raw, raw.len() as u64);
    let (text, _, _) = output_since(&st, 0);
    assert_eq!(text, "Agiterra    learninggodot01");
}

#[test]
fn line_editor_redraw_does_not_duplicate_first_char() {
    // Regression: zsh's ZLE echoes the first char, then repaints the line
    // from column 1, which naive stripping rendered as "ccd tmp". The
    // emulator resolves the repaint to the final "cd tmp".
    let raw = b"c\x1b[1Gcd tmp\n";
    let st = state_with(raw, raw.len() as u64);
    let (text, _, _) = output_since(&st, 0);
    assert_eq!(text, "cd tmp");
}

#[test]
fn input_line_reflects_pending_typed_text() {
    // A prompt with leftover typed input, no newline entered yet.
    let st = state_with(b"$ ttrs t testing", 16);
    let (line, col) = input_state(&st);
    assert_eq!(line, "$ ttrs t testing");
    assert_eq!(col, 16);
}

#[test]
fn dormant_session_is_listed_readable_and_restorable() {
    // A restored (dormant) session must appear in the list, resolve for
    // reads (history), and be flagged restorable — all without a live PTY.
    let id = "test-dormant-abc123";
    let history = b"$ echo restored\r\nrestored\r\n$ ";
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(history);
    dormant().lock().unwrap().insert(
        id.to_string(),
        DormantSession {
            info: ShellSessionInfo {
                session_id: id.to_string(),
                title: "restored".to_string(),
                current_directory: "/tmp/restored".to_string(),
                shell: "/bin/zsh".to_string(),
                created_at: 1,
                rows: 24,
                cols: 80,
                running: false,
                restorable: true,
                project_ref: None,
                shared: true,
                roster: Vec::new(),
                coding_session: None,
            },
            state: Arc::new(Mutex::new(SharedState {
                scrollback: history.to_vec(),
                total: history.len() as u64,
                last_output_at: None,
                parser,
            })),
            cwd: "/tmp/restored".to_string(),
        },
    );

    let listed = list();
    let entry = listed
        .iter()
        .find(|s| s.session_id == id)
        .expect("dormant session listed");
    assert!(entry.restorable);
    assert!(!entry.running);

    // Reads resolve against the dormant state and show the history.
    let read = read(id, false, true, None).expect("read dormant");
    assert!(read.text.contains("restored"), "history: {:?}", read.text);

    // Cleanup so the shared static doesn't leak into other tests.
    dormant().lock().unwrap().remove(id);
}

#[test]
fn drain_replay_rebuilds_state_from_host_frames() {
    use std::os::unix::net::UnixStream;

    // A fake host: send scrollback history as Output, then Synced with a
    // total that exceeds the retained bytes (as after a front-drop).
    let (mut host, client) = UnixStream::pair().expect("socketpair");
    let history = b"restored-history\r\n$ ";
    let hello_total = 5000u64;
    std::thread::spawn(move || {
        let mut host = &mut host;
        Frame::Output(history.to_vec()).write_to(&mut host).unwrap();
        Frame::Synced.write_to(&mut host).unwrap();
        // Keep the socket open so the drain's reads don't hit EOF early.
        std::thread::sleep(Duration::from_millis(50));
    });

    let state = Arc::new(Mutex::new(SharedState {
        scrollback: Vec::new(),
        total: 0,
        last_output_at: None,
        parser: vt100::Parser::new(24, 80, 0),
    }));
    let mut client = client;
    let exited = drain_replay(&mut client, hello_total, &state).expect("drain");
    assert!(!exited);

    let st = state.lock().unwrap();
    // History landed in scrollback (silently — no live-cursor bump), and
    // the cursor was seeded from the host's Hello total.
    assert_eq!(st.scrollback, history);
    assert_eq!(st.total, hello_total);
}

#[test]
fn drain_replay_reports_already_exited_shell() {
    use std::os::unix::net::UnixStream;

    let (mut host, client) = UnixStream::pair().expect("socketpair");
    std::thread::spawn(move || {
        Frame::Exit.write_to(&mut host).unwrap();
        std::thread::sleep(Duration::from_millis(50));
    });
    let state = Arc::new(Mutex::new(SharedState {
        scrollback: Vec::new(),
        total: 0,
        last_output_at: None,
        parser: vt100::Parser::new(24, 80, 0),
    }));
    let mut client = client;
    assert!(drain_replay(&mut client, 0, &state).expect("drain"));
}

/// RAII cleanup for `register_live_session_with_fake_host`. A test body
/// panicking (a failed `assert_eq!`, an `.expect` on `write`/`resize`)
/// must not leave the inserted session in the process-shared registry or
/// its temp socket dir behind for a later test in the same process to
/// observe — `Drop` runs during unwinding, so this holds regardless of
/// where the panic happens. Removing the session also drops its
/// `client`/`io` write halves, which closes the socket and unblocks the
/// fake host's thread if it was still waiting in `Frame::read_from`.
struct LiveSessionGuard {
    id: String,
    dir: PathBuf,
}

impl Drop for LiveSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut sessions) = lock_registry() {
            sessions.remove(&self.id);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Fakes a live host over a real Unix socket, attaches to it via the
/// driver (exactly as `attach()` does), and registers the session live —
/// so `write`/`resize` below exercise the real production call chain:
/// `manager::write`/`manager::resize` -> `BuzzShellHostDriver::input_resize`
/// -> the socket. Returns a guard that panic-safely removes the session
/// and temp dir on drop, and a handle that yields the one frame the fake
/// host receives.
fn register_live_session_with_fake_host(
    id: &str,
) -> (LiveSessionGuard, std::thread::JoinHandle<Frame>) {
    // Unix socket paths are capped well under 104 bytes (SUN_LEN) — keep
    // this short (an atomic counter, not `id` or the platform temp dir,
    // either of which can be long enough to overflow it) and unique per
    // call so two tests in the same process never collide.
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from("/tmp").join(format!("a16-{}-{n}", std::process::id() % 10000));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("host.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let hello = beekeeper_shell_host::proto::Hello {
        total: 0,
        cwd: "/tmp".to_string(),
        shell_pid: None,
    };
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        Frame::Hello(hello).write_to(&mut conn).unwrap();
        Frame::read_from(&mut conn).unwrap().unwrap()
    });

    let driver = session_driver::BuzzShellHostDriver;
    let (io, _hello, _read_half) = driver
        .attach_existing(&socket, Duration::from_secs(2))
        .expect("attach must succeed");
    let client = io.full_client();

    let mut sessions = lock_registry().unwrap();
    sessions.insert(
        id.to_string(),
        ShellSession {
            info: ShellSessionInfo {
                session_id: id.to_string(),
                title: "t".to_string(),
                current_directory: "/tmp".to_string(),
                shell: "/bin/sh".to_string(),
                created_at: 1,
                rows: 24,
                cols: 80,
                running: true,
                restorable: false,
                project_ref: None,
                shared: true,
                roster: Vec::new(),
                coding_session: None,
            },
            client,
            io,
            state: Arc::new(Mutex::new(SharedState {
                scrollback: Vec::new(),
                total: 0,
                last_output_at: None,
                parser: vt100::Parser::new(24, 80, 0),
            })),
            shell_pid: None,
        },
    );
    drop(sessions);
    (
        LiveSessionGuard {
            id: id.to_string(),
            dir,
        },
        server,
    )
}

#[test]
fn write_routes_through_the_driver_to_the_hosts_socket() {
    let id = "test-write-routes-through-driver";
    let (_guard, server) = register_live_session_with_fake_host(id);

    write(id, b"echo hi\n").expect("write must succeed");

    let received = server.join().unwrap();
    assert_eq!(received, Frame::Input(b"echo hi\n".to_vec()));
}

#[test]
fn resize_routes_through_the_driver_and_updates_session_info() {
    let id = "test-resize-routes-through-driver";
    let (_guard, server) = register_live_session_with_fake_host(id);

    resize(id, 50, 200).expect("resize must succeed");

    let received = server.join().unwrap();
    assert_eq!(
        received,
        Frame::Resize {
            rows: 50,
            cols: 200
        }
    );

    let sessions = lock_registry().unwrap();
    let session = sessions.get(id).expect("session still registered");
    assert_eq!(session.info.rows, 50);
    assert_eq!(session.info.cols, 200);
}
