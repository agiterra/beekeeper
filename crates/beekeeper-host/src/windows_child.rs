//! Windows child custody: assign suspended, then resume, kill descendants on close.
use std::os::windows::process::ExitStatusExt;
use std::{io, process::ExitStatus};
use win_custody::{Job, JobLimits, Launch, Process};

/// A held process handle and its private, non-inherited kill-on-close job.
pub struct ProviderChild {
    process: Process,
    job: Job,
    pid: u32,
}

impl ProviderChild {
    /// The PID observed from the child handle, for status only.
    pub fn id(&self) -> u32 {
        self.pid
    }
    /// Observe exit without reopening a PID.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process
            .exit_code()
            .map(|code| code.map(ExitStatus::from_raw))
    }
    /// Wait for the process this Host created.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.process.wait(None)?;
        self.try_wait()?
            .ok_or_else(|| io::Error::other("provider wait completed without exit status"))
    }
    /// Terminate only this child's job, including descendants.
    pub fn kill_tree(&self) -> io::Result<()> {
        self.job.terminate(1)
    }
}

/// The program path with Windows separators, for the launch boundary.
///
/// `cmd.exe` (and so every `.cmd`/`.bat` wrapper) fails when it is started by a
/// mixed-slash path such as `C:\Windows\System32/cmd.exe`: it prints "The
/// directory or file cannot be created." and exits 1, before reading its
/// arguments. A configured `provider_command` can carry `/`, so the Host
/// normalizes at the point it launches, not only in fixtures. A verbatim
/// `\\?\` path is exact — `/` is a literal there — and is left alone, and so is
/// a bare name that `PATH` resolves.
pub(crate) fn normalize_program(program: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let units: Vec<u16> = program.encode_wide().collect();
    let verbatim = [b'\\', b'\\', b'?', b'\\'].map(u16::from);
    if units.starts_with(&verbatim) {
        return program.to_owned();
    }
    let slash = u16::from(b'/');
    let backslash = u16::from(b'\\');
    let fixed: Vec<u16> = units
        .iter()
        .map(|&unit| if unit == slash { backslash } else { unit })
        .collect();
    std::ffi::OsString::from_wide(&fixed)
}

pub(super) fn spawn(
    command: &std::process::Command,
    stdout: &std::fs::File,
    stderr: &std::fs::File,
) -> io::Result<ProviderChild> {
    let job = Job::create(&JobLimits::new().kill_on_close(true))?;
    let null = win_custody::null_device()?;
    let mut launch = Launch::new(normalize_program(command.get_program()));
    launch
        .args(command.get_args())
        .stdin(&null)
        .stdout(stdout)
        .stderr(stderr)
        .create_no_window(true);
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                launch.env(key, value);
            }
            None => {
                launch.env_remove(key);
            }
        }
    }
    if let Some(dir) = command.get_current_dir() {
        launch.current_dir(dir);
    }
    let process = launch.spawn_in_job(&job)?;
    let pid = process.pid()?;
    Ok(ProviderChild { process, job, pid })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_job_reports_exit_and_kill_on_close_policy() {
        let log = tempfile::tempfile().expect("log");
        let system = std::env::var_os("SystemRoot").expect("Windows system root");
        // Components, not "System32/cmd.exe": cmd.exe fails ("The directory or
        // file cannot be created.", exit 1) when launched by a mixed-slash path.
        let cmd = std::path::Path::new(&system)
            .join("System32")
            .join("cmd.exe");
        let mut command = std::process::Command::new(cmd);
        command.args(["/d", "/c", "exit", "7"]);
        let mut child = spawn(&command, &log, &log).expect("spawn in job");
        assert!(child.job.limits().expect("limits").kills_on_close());
        assert_eq!(child.wait().expect("wait").code(), Some(7));
    }

    fn launched_code(program: &std::path::Path, args: &[&str]) -> Option<i32> {
        let log = tempfile::tempfile().expect("log");
        let mut command = std::process::Command::new(program);
        command.args(args);
        let mut child = spawn(&command, &log, &log).expect("spawn in job");
        child.wait().expect("wait").code()
    }

    #[test]
    fn program_paths_are_normalized_at_the_launch_boundary() {
        use std::ffi::OsStr;
        assert_eq!(
            normalize_program(OsStr::new("C:\\Windows\\System32/cmd.exe")),
            OsStr::new("C:\\Windows\\System32\\cmd.exe")
        );
        assert_eq!(
            normalize_program(OsStr::new("C:/a/b/c.exe")),
            OsStr::new("C:\\a\\b\\c.exe")
        );
        // Already right, a bare name and a verbatim path are not touched.
        for same in ["C:\\a\\b.exe", "python", "\\\\?\\C:\\a/b.exe"] {
            assert_eq!(normalize_program(OsStr::new(same)), OsStr::new(same));
        }
    }

    #[test]
    fn a_mixed_slash_program_path_still_launches_cmd() {
        // The raw form makes cmd.exe print "The directory or file cannot be
        // created." and exit 1; the launch boundary must repair it.
        let system = std::env::var("SystemRoot").expect("Windows system root");
        let mixed = format!("{system}\\System32/cmd.exe");
        assert_eq!(
            launched_code(std::path::Path::new(&mixed), &["/d", "/c", "exit", "7"]),
            Some(7)
        );
    }

    #[test]
    fn a_verbatim_program_path_launches() {
        let system = std::env::var_os("SystemRoot").expect("Windows system root");
        let cmd = std::path::Path::new(&system)
            .join("System32")
            .join("cmd.exe");
        let verbatim = std::fs::canonicalize(cmd).expect("canonicalize");
        assert!(verbatim.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(
            launched_code(&verbatim, &["/d", "/c", "exit", "5"]),
            Some(5)
        );
    }

    #[test]
    fn a_cmd_wrapper_reached_by_a_mixed_slash_path_runs() {
        let dir = tempfile::tempdir().expect("dir");
        let script = dir.path().join("wrapper.cmd");
        std::fs::write(&script, "@exit /b 9\r\n").expect("script");
        let mixed = format!("{}/wrapper.cmd", dir.path().display());
        assert_eq!(launched_code(std::path::Path::new(&mixed), &[]), Some(9));
    }
}
