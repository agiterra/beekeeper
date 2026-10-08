//! A scripted [`CommandRunner`] for the device tests: no simulator is booted,
//! nothing is installed, and every call is recorded.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use super::simctl::{CommandRunner, RunError, RunOutput};
use super::toolchain::DeveloperDirs;

/// The real `simctl list -j devices available` output from this Mac
/// (Xcode 27.0, iOS 27.0 runtime), captured read-only.
pub const XCODE27_DEVICES: &[u8] = include_bytes!("fixtures/simctl_devices_available_xcode27.json");
/// The fixture's `iPhone 17` UDID.
pub const IPHONE17_UDID: &str = "70E4C638-DE97-4AB1-990D-D0FC30018372";

/// One recorded call: program, args, env.
type Call = (String, Vec<String>, Vec<(String, String)>);

type Rule = Box<dyn Fn(&str, &[String]) -> Option<Result<RunOutput, RunError>> + Send + Sync>;

/// A runner that answers from rules (first match wins) and records calls.
#[derive(Default)]
pub struct FakeRunner {
    rules: Mutex<Vec<Rule>>,
    once: Mutex<VecDeque<(String, Result<RunOutput, RunError>)>>,
    calls: Mutex<Vec<Call>>,
}

/// A developer directory that exists (via `DEVELOPER_DIR`), so a probe goes
/// on to the fake's `xcrun`.
pub fn present_developer_dir() -> DeveloperDirs {
    DeveloperDirs {
        env: Some(std::env::temp_dir()),
        fallbacks: Vec::new(),
        macos: true,
    }
}

/// No developer directory anywhere: a Mac without Xcode or the Command
/// Line Tools.
pub fn no_developer_dir() -> DeveloperDirs {
    DeveloperDirs {
        env: None,
        fallbacks: vec![std::path::PathBuf::from("/nonexistent/beekeeper/Xcode.app")],
        macos: true,
    }
}

/// A host that is not macOS: the simulator is never offered there.
pub fn not_macos() -> DeveloperDirs {
    DeveloperDirs {
        macos: false,
        ..present_developer_dir()
    }
}

/// A successful output carrying `stdout`.
pub fn ok(stdout: &[u8]) -> Result<RunOutput, RunError> {
    Ok(RunOutput {
        status: Some(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    })
}

/// A failed output carrying `stderr`.
pub fn fail(code: i32, stderr: &str) -> Result<RunOutput, RunError> {
    Ok(RunOutput {
        status: Some(code),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    })
}

impl FakeRunner {
    /// A runner with no rules: every call fails as "not found".
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every call whose program is `program` and whose joined args
    /// contain `needle`.
    pub fn on(self, program: &str, needle: &str, answer: Result<RunOutput, RunError>) -> Self {
        let program = program.to_owned();
        let needle = needle.to_owned();
        self.rules
            .lock()
            .expect("rules")
            .push(Box::new(move |prog, args| {
                (prog.ends_with(&program) && args.join(" ").contains(&needle))
                    .then(|| answer.clone())
            }));
        self
    }

    /// Answer calls matching like [`Self::on`] by running `answer` each time,
    /// so a fake can have side effects (a daemon writing its state file).
    pub fn on_fn(
        self,
        program: &str,
        needle: &str,
        answer: impl Fn() -> Result<RunOutput, RunError> + Send + Sync + 'static,
    ) -> Self {
        let program = program.to_owned();
        let needle = needle.to_owned();
        self.rules
            .lock()
            .expect("rules")
            .push(Box::new(move |prog, args| {
                (prog.ends_with(&program) && args.join(" ").contains(&needle)).then(&answer)
            }));
        self
    }

    /// Answer the next call containing `needle` once with `answer`, before
    /// any rule.
    pub fn once(&self, needle: &str, answer: Result<RunOutput, RunError>) {
        self.once
            .lock()
            .expect("once")
            .push_back((needle.to_owned(), answer));
    }

    /// A runner with the Xcode 27 fixture, a working boot and a JPEG shot.
    pub fn xcode27() -> Self {
        Self::new()
            .on(
                "xcrun",
                "--find simctl",
                ok(b"/Applications/Xcode.app/simctl\n"),
            )
            .on("xcrun", "list -j devices available", ok(XCODE27_DEVICES))
            .on("xcrun", "bootstatus", ok(b""))
            .on("xcrun", "shutdown", ok(b""))
            .on(
                "xcrun",
                "screenshot --type=jpeg",
                ok(&tiny_image(image::ImageFormat::Jpeg, 0)),
            )
            .on(
                "xcrun",
                "screenshot --type=png",
                ok(&tiny_image(image::ImageFormat::Png, 0)),
            )
            .on("open", "-a Simulator", ok(b""))
    }

    /// Every call so far as `program args…`.
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("calls")
            .iter()
            .map(|(program, args, _)| format!("{program} {}", args.join(" ")))
            .collect()
    }

    /// The env of the first call containing `needle`.
    pub fn env_of(&self, needle: &str) -> Option<Vec<(String, String)>> {
        self.calls
            .lock()
            .expect("calls")
            .iter()
            .find(|(program, args, _)| format!("{program} {}", args.join(" ")).contains(needle))
            .map(|(_, _, env)| env.clone())
    }
}

impl CommandRunner for FakeRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        _timeout: Duration,
    ) -> Result<RunOutput, RunError> {
        self.calls
            .lock()
            .expect("calls")
            .push((program.to_owned(), args.to_vec(), env.to_vec()));
        let joined = format!("{program} {}", args.join(" "));
        {
            let mut once = self.once.lock().expect("once");
            if let Some(index) = once.iter().position(|(needle, _)| joined.contains(needle)) {
                if let Some((_, answer)) = once.remove(index) {
                    return answer;
                }
            }
        }
        for rule in self.rules.lock().expect("rules").iter() {
            if let Some(answer) = rule(program, args) {
                return answer;
            }
        }
        Err(RunError::NotFound(program.to_owned()))
    }
}

/// A small encoded image whose pixels depend on `shade`, so two shades hash
/// differently.
pub fn tiny_image(format: image::ImageFormat, shade: u8) -> Vec<u8> {
    sized_image(format, 40, 80, shade)
}

/// An encoded `width`×`height` image with a gradient seeded by `shade`.
pub fn sized_image(format: image::ImageFormat, width: u32, height: u32, shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x as u8).wrapping_mul(3).wrapping_add(shade),
            (y as u8).wrapping_mul(5),
            shade,
        ])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), format)
        .expect("encode");
    out
}
