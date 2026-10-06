//! What the relay process can see of the machine it runs on: CPU, memory and
//! disk, sampled on a fixed cadence and served by `GET /health/system`
//! (`api::system_health`).
//!
//! Every figure is stamped with when it was sampled, and the response carries
//! that age, because a stale sample presented as live is the kind of quiet
//! lie this project treats as a bug. The sampler is a plain thread, not a
//! tokio task: `sysinfo` reads `/proc` (Linux) or asks the kernel (macOS)
//! synchronously, and a thread that sleeps between samples cannot stall the
//! async runtime.
//!
//! What it cannot see is stated rather than papered over. In a container the
//! CPU and memory totals are the host's unless a cgroup limit is set, in
//! which case the limit is reported beside them as `container`; the load
//! average is always the host's; the disks are only the filesystems this
//! process can `statvfs`, so Postgres, Redis and MinIO volumes living in other
//! containers are invisible here.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System};

/// How often the sampler refreshes. Ten seconds keeps the cost negligible
/// and the card on the desktop (which polls on the same order) fresh.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(10);

/// The first CPU reading needs two refreshes a short distance apart;
/// `sysinfo` documents a 200 ms floor, and a second gives a steadier number.
const FIRST_SAMPLE_WARMUP: Duration = Duration::from_secs(1);

/// One sample of the machine, as served on the wire (snake_case JSON).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemHealthSnapshot {
    /// When the sample was taken, RFC 3339 UTC.
    pub sampled_at: DateTime<Utc>,
    /// The sampler's cadence, so a reader knows how stale "fresh" can be.
    pub interval_seconds: u64,
    /// The host this process runs on.
    pub host: HostHealth,
    /// CPU, machine-wide and for this process.
    pub cpu: CpuHealth,
    /// Memory, machine-wide, per cgroup when limited, and for this process.
    pub memory: MemoryHealth,
    /// The filesystems this process can see, deduplicated.
    pub disks: Vec<DiskHealth>,
}

/// Identity of the machine, as far as the process can tell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostHealth {
    /// Kernel host name, or `null` when it cannot be read.
    pub name: Option<String>,
    /// Operating system name and version, or `null`.
    pub os: Option<String>,
    /// Seconds since the machine booted.
    pub uptime_seconds: u64,
    /// Seconds since this relay process started.
    pub relay_uptime_seconds: u64,
}

/// CPU figures. Percentages are 0–100 of the whole machine for
/// `machine_percent`; `process_percent` is of one core, so it can exceed 100
/// on a multi-core machine, the way `top` reports it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuHealth {
    /// Logical cores the kernel exposes to this process.
    pub cores: usize,
    /// Machine-wide usage over the last sample interval, 0–100.
    pub machine_percent: f32,
    /// This process's usage over the last sample interval, percent of one core.
    pub process_percent: f32,
    /// The host's 1/5/15-minute load average; `null` where the platform has none.
    pub load_average: Option<LoadAverage>,
}

/// The classic three load figures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoadAverage {
    /// Over the last minute.
    pub one: f64,
    /// Over the last five minutes.
    pub five: f64,
    /// Over the last fifteen minutes.
    pub fifteen: f64,
}

/// Memory figures in bytes. `machine_*` is what the kernel reports for the
/// whole machine; `container` is present only when a cgroup limit below the
/// machine total applies to this process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryHealth {
    /// Physical memory the kernel reports.
    pub machine_total_bytes: u64,
    /// In use by everything, as the kernel counts it (page cache excluded).
    pub machine_used_bytes: u64,
    /// What the kernel says could be handed out without swapping.
    pub machine_available_bytes: u64,
    /// Swap configured, zero when there is none.
    pub swap_total_bytes: u64,
    /// Swap in use.
    pub swap_used_bytes: u64,
    /// This process's resident set.
    pub process_rss_bytes: u64,
    /// The cgroup this process runs in, when it is limited below the machine.
    pub container: Option<ContainerMemory>,
}

/// A cgroup memory limit and how much of it is taken.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerMemory {
    /// The cgroup's memory ceiling.
    pub limit_bytes: u64,
    /// What the cgroup has taken of it.
    pub used_bytes: u64,
}

/// One filesystem, named by every configured path that lives on it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiskHealth {
    /// What the paths are for, in the order they were configured: e.g.
    /// `["git data", "root"]` when both share one filesystem.
    pub labels: Vec<String>,
    /// The paths that were measured, parallel to `labels`.
    pub paths: Vec<String>,
    /// Size of the filesystem.
    pub total_bytes: u64,
    /// Bytes an unprivileged writer could still use.
    pub available_bytes: u64,
}

/// A filesystem measurement before deduplication: the pure input to
/// [`dedupe_disks`], so the merge rule is testable without a real mount.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskMeasurement {
    /// What the path is for, as the wire will label it.
    pub label: String,
    /// The path that was measured.
    pub path: String,
    /// Kernel filesystem id. Two paths with the same id and total size are
    /// one filesystem; so are two that report the same total and the same
    /// free space at the same instant, which is how one pool of storage
    /// shows up under different ids (APFS volumes in one container, an
    /// overlay root over its host's disk).
    pub filesystem_id: u64,
    /// Size of the filesystem.
    pub total_bytes: u64,
    /// Bytes an unprivileged writer could still use.
    pub available_bytes: u64,
}

/// A path the sampler measures, with the label the wire carries for it.
#[derive(Debug, Clone)]
pub struct WatchedPath {
    /// What the path is for, as the wire will label it.
    pub label: String,
    /// Where to run `statvfs`.
    pub path: PathBuf,
}

static LATEST: RwLock<Option<Arc<SystemHealthSnapshot>>> = RwLock::new(None);

/// The most recent sample, or `None` before the sampler has produced one.
pub fn latest() -> Option<Arc<SystemHealthSnapshot>> {
    LATEST.read().ok().and_then(|guard| guard.clone())
}

fn publish(snapshot: SystemHealthSnapshot) {
    if let Ok(mut guard) = LATEST.write() {
        *guard = Some(Arc::new(snapshot));
    }
}

/// Whole seconds between `sampled_at` and `now`, never negative: a clock
/// that stepped backwards reads as a fresh sample, not a future one.
pub fn age_seconds(sampled_at: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    now.signed_duration_since(sampled_at)
        .num_seconds()
        .max(0)
        .unsigned_abs()
}

/// Merge measurements that landed on the same storage into one entry,
/// keeping the first path's position and appending the later labels. Same
/// storage means the same filesystem id and total size, or the same total
/// and the same free space in one sample (see [`DiskMeasurement`]).
pub fn dedupe_disks(measurements: Vec<DiskMeasurement>) -> Vec<DiskHealth> {
    let mut disks: Vec<(DiskMeasurement, DiskHealth)> = Vec::new();
    for measurement in measurements {
        if let Some((_, existing)) = disks.iter_mut().find(|(first, _)| {
            first.total_bytes == measurement.total_bytes
                && (first.filesystem_id == measurement.filesystem_id
                    || first.available_bytes == measurement.available_bytes)
        }) {
            existing.labels.push(measurement.label);
            existing.paths.push(measurement.path);
            continue;
        }
        let disk = DiskHealth {
            labels: vec![measurement.label.clone()],
            paths: vec![measurement.path.clone()],
            total_bytes: measurement.total_bytes,
            available_bytes: measurement.available_bytes,
        };
        disks.push((measurement, disk));
    }
    disks.into_iter().map(|(_, disk)| disk).collect()
}

/// The cgroup block, only when the limit is real: a cgroup that reports the
/// machine's own total (or more) is not limiting anything worth a second row.
pub fn container_memory(
    limit_bytes: Option<u64>,
    cgroup_used_bytes: u64,
    machine_total_bytes: u64,
) -> Option<ContainerMemory> {
    let limit = limit_bytes?;
    if limit == 0 || limit >= machine_total_bytes {
        return None;
    }
    Some(ContainerMemory {
        limit_bytes: limit,
        used_bytes: cgroup_used_bytes,
    })
}

#[cfg(unix)]
// `statvfs` field widths differ by platform (`c_ulong` / `fsblkcnt_t` are
// u32 on some targets and u64 on others); `u64::from` is the lossless spelling
// on all of them, and is a no-op where clippy notices it.
#[allow(clippy::useless_conversion)]
fn measure_path(label: &str, path: &Path) -> Option<DiskMeasurement> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    let fragment = u64::from(stat.fragment_size());
    // The configured path may be relative (`./repos` in a dev `.env`); the
    // wire names the place on disk, not the spelling in the config.
    let shown = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Some(DiskMeasurement {
        label: label.to_string(),
        path: shown.display().to_string(),
        filesystem_id: u64::from(stat.filesystem_id()),
        total_bytes: u64::from(stat.blocks()).saturating_mul(fragment),
        available_bytes: u64::from(stat.blocks_available()).saturating_mul(fragment),
    })
}

#[cfg(not(unix))]
fn measure_path(_label: &str, _path: &Path) -> Option<DiskMeasurement> {
    None
}

fn load_average() -> Option<LoadAverage> {
    if cfg!(windows) {
        return None;
    }
    let load = System::load_average();
    Some(LoadAverage {
        one: load.one,
        five: load.five,
        fifteen: load.fifteen,
    })
}

struct Sampler {
    system: System,
    pid: Option<sysinfo::Pid>,
    paths: Vec<WatchedPath>,
    relay_started_at: Instant,
}

impl Sampler {
    fn refresh(&mut self) {
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage());
        if let Some(pid) = self.pid {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_memory().with_cpu(),
            );
        }
    }

    fn snapshot(&self) -> SystemHealthSnapshot {
        let process = self.pid.and_then(|pid| self.system.process(pid));
        let machine_total = self.system.total_memory();
        let cgroup = self.system.cgroup_limits();
        let container = container_memory(
            cgroup.as_ref().map(|limits| limits.total_memory),
            cgroup
                .as_ref()
                .map(|limits| limits.total_memory.saturating_sub(limits.free_memory))
                .unwrap_or(0),
            machine_total,
        );
        let disks = dedupe_disks(
            self.paths
                .iter()
                .filter_map(|watched| measure_path(&watched.label, &watched.path))
                .collect(),
        );
        SystemHealthSnapshot {
            sampled_at: Utc::now(),
            interval_seconds: SAMPLE_INTERVAL.as_secs(),
            host: HostHealth {
                name: System::host_name(),
                os: System::long_os_version(),
                uptime_seconds: System::uptime(),
                relay_uptime_seconds: self.relay_started_at.elapsed().as_secs(),
            },
            cpu: CpuHealth {
                cores: self.system.cpus().len(),
                machine_percent: self.system.global_cpu_usage(),
                process_percent: process.map(|p| p.cpu_usage()).unwrap_or(0.0),
                load_average: load_average(),
            },
            memory: MemoryHealth {
                machine_total_bytes: machine_total,
                machine_used_bytes: self.system.used_memory(),
                machine_available_bytes: self.system.available_memory(),
                swap_total_bytes: self.system.total_swap(),
                swap_used_bytes: self.system.used_swap(),
                process_rss_bytes: process.map(|p| p.memory()).unwrap_or(0),
                container,
            },
            disks,
        }
    }
}

/// Start the sampling thread. `git_repo_path` is the relay's own data
/// directory; it and `/` are the two filesystems measured (merged into one
/// row when they are the same). Returns `false` if the thread could not be
/// spawned, in which case `latest()` stays `None` and the endpoint answers
/// 503 honestly rather than inventing a sample.
pub fn spawn_sampler(git_repo_path: PathBuf, relay_started_at: Instant) -> bool {
    let paths = vec![
        WatchedPath {
            label: "git data".to_string(),
            path: git_repo_path,
        },
        WatchedPath {
            label: "root".to_string(),
            path: PathBuf::from("/"),
        },
    ];
    let spawned = std::thread::Builder::new()
        .name("system-health".to_string())
        .spawn(move || {
            let mut sampler = Sampler {
                system: System::new(),
                pid: sysinfo::get_current_pid().ok(),
                paths,
                relay_started_at,
            };
            sampler.refresh();
            std::thread::sleep(FIRST_SAMPLE_WARMUP);
            loop {
                sampler.refresh();
                publish(sampler.snapshot());
                std::thread::sleep(SAMPLE_INTERVAL);
            }
        });
    match spawned {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(%error, "system health sampler could not start");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement(label: &str, path: &str, id: u64, total: u64, avail: u64) -> DiskMeasurement {
        DiskMeasurement {
            label: label.to_string(),
            path: path.to_string(),
            filesystem_id: id,
            total_bytes: total,
            available_bytes: avail,
        }
    }

    #[test]
    fn same_filesystem_becomes_one_row_with_both_labels() {
        let disks = dedupe_disks(vec![
            measurement("git data", "/data/git", 7, 1000, 400),
            measurement("root", "/", 7, 1000, 400),
        ]);
        assert_eq!(
            disks,
            vec![DiskHealth {
                labels: vec!["git data".into(), "root".into()],
                paths: vec!["/data/git".into(), "/".into()],
                total_bytes: 1000,
                available_bytes: 400,
            }]
        );
    }

    #[test]
    fn different_filesystems_stay_separate_in_configured_order() {
        let disks = dedupe_disks(vec![
            measurement("git data", "/data/git", 7, 1000, 400),
            measurement("root", "/", 9, 500, 100),
        ]);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].paths, vec!["/data/git".to_string()]);
        assert_eq!(disks[1].paths, vec!["/".to_string()]);
    }

    #[test]
    fn one_pool_under_two_ids_is_one_row() {
        // APFS: `/` and the data volume are different filesystems sharing
        // one container, so they report the same size and the same free
        // space; an overlay root over the docker host's disk does the same.
        let disks = dedupe_disks(vec![
            measurement("git data", "/data/git", 7, 1000, 400),
            measurement("root", "/", 9, 1000, 400),
        ]);
        assert_eq!(disks.len(), 1);
        assert_eq!(
            disks[0].labels,
            vec!["git data".to_string(), "root".to_string()]
        );
    }

    #[test]
    fn same_size_but_different_free_space_under_different_ids_stays_separate() {
        let disks = dedupe_disks(vec![
            measurement("git data", "/data/git", 7, 1000, 400),
            measurement("root", "/", 9, 1000, 300),
        ]);
        assert_eq!(disks.len(), 2);
    }

    #[test]
    fn same_id_but_different_size_is_not_merged() {
        // Filesystem ids are not unique across mounts on every platform; the
        // total size is the second key so a collision does not hide a disk.
        let disks = dedupe_disks(vec![
            measurement("git data", "/data/git", 7, 1000, 400),
            measurement("root", "/", 7, 2000, 400),
        ]);
        assert_eq!(disks.len(), 2);
    }

    #[test]
    fn container_row_only_when_the_limit_is_below_the_machine() {
        assert_eq!(container_memory(None, 10, 100), None);
        assert_eq!(container_memory(Some(0), 10, 100), None);
        assert_eq!(container_memory(Some(100), 10, 100), None);
        assert_eq!(container_memory(Some(150), 10, 100), None);
        assert_eq!(
            container_memory(Some(64), 10, 100),
            Some(ContainerMemory {
                limit_bytes: 64,
                used_bytes: 10
            })
        );
    }

    #[test]
    fn age_is_whole_seconds_and_never_negative() {
        let sampled = DateTime::parse_from_rfc3339("2026-09-14T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let later = sampled + chrono::Duration::milliseconds(3_900);
        assert_eq!(age_seconds(sampled, later), 3);
        let earlier = sampled - chrono::Duration::seconds(5);
        assert_eq!(age_seconds(sampled, earlier), 0);
    }

    #[test]
    fn wire_shape_is_snake_case_and_round_trips() {
        let snapshot = SystemHealthSnapshot {
            sampled_at: DateTime::parse_from_rfc3339("2026-09-14T20:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            interval_seconds: 10,
            host: HostHealth {
                name: Some("agincus".into()),
                os: None,
                uptime_seconds: 5,
                relay_uptime_seconds: 4,
            },
            cpu: CpuHealth {
                cores: 4,
                machine_percent: 12.5,
                process_percent: 3.0,
                load_average: Some(LoadAverage {
                    one: 0.5,
                    five: 0.4,
                    fifteen: 0.3,
                }),
            },
            memory: MemoryHealth {
                machine_total_bytes: 100,
                machine_used_bytes: 60,
                machine_available_bytes: 40,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
                process_rss_bytes: 7,
                container: None,
            },
            disks: vec![],
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["sampled_at"], "2026-09-14T20:00:00Z");
        assert_eq!(json["memory"]["machine_total_bytes"], 100);
        assert_eq!(json["memory"]["container"], serde_json::Value::Null);
        assert_eq!(json["cpu"]["load_average"]["one"], 0.5);
        let back: SystemHealthSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(back, snapshot);
    }

    #[test]
    fn a_real_sample_of_this_machine_is_plausible() {
        // Exercises the sysinfo and statvfs plumbing once, cheaply: two
        // refreshes a moment apart, then the fields must be self-consistent.
        let mut sampler = Sampler {
            system: System::new(),
            pid: sysinfo::get_current_pid().ok(),
            paths: vec![WatchedPath {
                label: "root".into(),
                path: PathBuf::from("/"),
            }],
            relay_started_at: Instant::now(),
        };
        sampler.refresh();
        std::thread::sleep(Duration::from_millis(250));
        sampler.refresh();
        let snapshot = sampler.snapshot();
        assert!(snapshot.cpu.cores > 0);
        assert!(snapshot.memory.machine_total_bytes > 0);
        assert!(snapshot.memory.machine_used_bytes <= snapshot.memory.machine_total_bytes);
        assert!(snapshot.memory.process_rss_bytes > 0);
        if cfg!(unix) {
            assert_eq!(snapshot.disks.len(), 1);
            assert!(snapshot.disks[0].total_bytes >= snapshot.disks[0].available_bytes);
        }
    }
}
