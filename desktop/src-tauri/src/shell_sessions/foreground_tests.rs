//! The foreground read (SV-22): the parser and rule on fixed `ps` rows, and
//! the whole read against a real job-control shell in a real PTY — busy
//! while `sleep` holds the terminal, idle again at the prompt.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use super::{command_is_running, parse_ps_groups, read_process_groups, ProcessGroups};

#[test]
fn the_shell_at_its_prompt_owns_the_terminal_and_is_not_busy() {
    assert_eq!(
        command_is_running(ProcessGroups {
            pgid: 4100,
            tpgid: 4100
        }),
        Some(false)
    );
}

#[test]
fn a_job_holding_the_terminal_makes_the_shell_busy() {
    assert_eq!(
        command_is_running(ProcessGroups {
            pgid: 4100,
            tpgid: 4188
        }),
        Some(true)
    );
}

#[test]
fn a_process_without_a_terminal_is_unknown_never_idle() {
    for tpgid in [0, -1] {
        assert_eq!(
            command_is_running(ProcessGroups { pgid: 4100, tpgid }),
            None
        );
    }
}

#[test]
fn ps_rows_parse_and_junk_is_skipped() {
    let rows = parse_ps_groups("  4100  4100  4188\n garbage line\n4200 4200 -1\n1 2\n");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.get(&4100),
        Some(&ProcessGroups {
            pgid: 4100,
            tpgid: 4188
        })
    );
    assert_eq!(
        rows.get(&4200),
        Some(&ProcessGroups {
            pgid: 4200,
            tpgid: -1
        })
    );
}

#[test]
fn no_pids_reads_nothing_and_runs_nothing() {
    assert!(read_process_groups(&[]).is_empty());
}

/// Poll the real read until it says `want`, or give up after `limit`.
fn wait_for(pid: u32, want: Option<bool>, limit: Duration) -> Option<bool> {
    let started = Instant::now();
    loop {
        let seen = read_process_groups(&[pid])
            .get(&pid)
            .copied()
            .and_then(command_is_running);
        if seen == want || started.elapsed() > limit {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn a_real_shell_reads_busy_while_a_command_runs_and_idle_at_the_prompt() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut builder = CommandBuilder::new("/bin/sh");
    builder.arg("-i");
    builder.env_clear();
    builder.env("PATH", "/bin:/usr/bin");
    builder.env("TERM", "dumb");
    builder.env("PS1", "$ ");
    let mut child = pair.slave.spawn_command(builder).expect("spawn sh");
    drop(pair.slave);
    let pid = child.process_id().expect("shell pid");
    // Drain the PTY so the shell never blocks on a full buffer.
    let mut reader = pair.master.try_clone_reader().expect("reader");
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
    });
    let mut writer = pair.master.take_writer().expect("writer");

    assert_eq!(
        wait_for(pid, Some(false), Duration::from_secs(5)),
        Some(false),
        "a shell at its prompt is not running a command"
    );
    writer.write_all(b"sleep 30\n").expect("type sleep");
    writer.flush().expect("flush");
    assert_eq!(
        wait_for(pid, Some(true), Duration::from_secs(5)),
        Some(true),
        "sleep holds the terminal's foreground group"
    );
    // Ctrl-C ends the job; the shell takes the terminal back.
    writer.write_all(b"\x03").expect("interrupt");
    writer.flush().expect("flush");
    assert_eq!(
        wait_for(pid, Some(false), Duration::from_secs(5)),
        Some(false),
        "back at the prompt, the badge clears"
    );
    let _ = child.kill();
    let _ = child.wait();
}
