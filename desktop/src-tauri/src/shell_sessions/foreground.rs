//! Is a shell running a command right now? (SV-22, DB6)
//!
//! The session view's Terminal badge counts this session's shells on this
//! computer that are running a command at this moment. "Running a command"
//! is a fact the terminal itself keeps: a job-control shell hands the
//! terminal's **foreground process group** to the job it starts and takes it
//! back at the prompt. So a shell is busy exactly when its terminal's
//! foreground process group (`tpgid`) is not the shell's own process group.
//!
//! The PTY belongs to the detached host process, so this side has no
//! descriptor to ask (`tcgetpgrp`); it asks the process table instead
//! (`ps -o pid=,pgid=,tpgid= -p <shell pids>`), which reports the same
//! number for the shell's controlling terminal. An idle open shell is not
//! busy; a shell whose pid is unknown, or whose row `ps` did not return, is
//! **unknown** — never counted, and never reported as idle either.

use std::collections::HashMap;

use serde::Serialize;

/// One shell's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellForegroundState {
    pub session_id: String,
    /// `Some(true)` while a command holds the terminal, `Some(false)` at the
    /// prompt, `None` when this machine could not tell.
    pub running_command: Option<bool>,
}

/// A process's group and its terminal's foreground group, from `ps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessGroups {
    pub pgid: i64,
    /// `0` or negative when the process has no controlling terminal.
    pub tpgid: i64,
}

/// Busy when the terminal's foreground group is a real group other than the
/// shell's own; `None` when the process has no terminal to ask about.
pub(crate) fn command_is_running(groups: ProcessGroups) -> Option<bool> {
    if groups.tpgid <= 0 || groups.pgid <= 0 {
        return None;
    }
    Some(groups.tpgid != groups.pgid)
}

/// Parse `ps -o pid=,pgid=,tpgid=` output. Lines that are not three integers
/// are skipped, never guessed at.
pub(crate) fn parse_ps_groups(output: &str) -> HashMap<u32, ProcessGroups> {
    let mut rows = HashMap::new();
    for line in output.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(pgid), Some(tpgid), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(pid), Ok(pgid), Ok(tpgid)) = (
            pid.parse::<u32>(),
            pgid.parse::<i64>(),
            tpgid.parse::<i64>(),
        ) else {
            continue;
        };
        rows.insert(pid, ProcessGroups { pgid, tpgid });
    }
    rows
}

/// Read the process groups of `pids` in one `ps` call. An empty map when
/// `ps` could not run: every shell then reads as unknown.
pub(crate) fn read_process_groups(pids: &[u32]) -> HashMap<u32, ProcessGroups> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    // `ps` exits non-zero when one of the pids is gone; its stdout still
    // carries the rows that exist, so the status is not consulted.
    match std::process::Command::new("ps")
        .args(["-o", "pid=,pgid=,tpgid=", "-p", &list])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) => parse_ps_groups(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => HashMap::new(),
    }
}

/// Answer for each requested shell session, in request order.
pub fn running_commands(session_ids: &[String]) -> Vec<ShellForegroundState> {
    let pids: Vec<(String, Option<u32>)> = session_ids
        .iter()
        .map(|id| (id.clone(), super::manager::live_shell_pid(id)))
        .collect();
    let known: Vec<u32> = pids.iter().filter_map(|(_, pid)| *pid).collect();
    let groups = read_process_groups(&known);
    pids.into_iter()
        .map(|(session_id, pid)| ShellForegroundState {
            running_command: pid
                .and_then(|pid| groups.get(&pid).copied())
                .and_then(command_is_running),
            session_id,
        })
        .collect()
}

#[cfg(test)]
#[path = "foreground_tests.rs"]
mod tests;
