//! Thread affinity governor for big.LITTLE ARM mobile processor topologies.
//!
//! Pinning main UI/FFI threads to Performance cores guarantees low latency and 120 FPS
//! rendering response, while pinning background networking, chunk hashing, and DHT routing
//! to low-power Efficiency cores minimizes battery drain and thermal throttling.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CpuTopology {
    pub performance_cores: Vec<usize>,
    pub efficiency_cores: Vec<usize>,
}

impl CpuTopology {
    /// Detects CPU topology by parsing sysfs max CPU frequencies on Linux/Android.
    pub fn discover() -> Self {
        let mut perf = Vec::new();
        let mut eff = Vec::new();

        let mut core_freqs: Vec<(usize, u64)> = Vec::new();
        let sys_cpu = Path::new("/sys/devices/system/cpu");

        if sys_cpu.exists() {
            if let Ok(entries) = fs::read_dir(sys_cpu) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("cpu") && name_str[3..].parse::<usize>().is_ok() {
                        let core_idx: usize = name_str[3..].parse().unwrap();
                        let freq_path = entry.path().join("cpufreq/cpuinfo_max_freq");
                        let freq = fs::read_to_string(freq_path)
                            .ok()
                            .and_then(|s| s.trim().parse::<u64>().ok())
                            .unwrap_or(0);
                        core_freqs.push((core_idx, freq));
                    }
                }
            }
        }

        if core_freqs.is_empty() {
            // Fallback default: assume 8 cores where 0..4 are EFF, 4..8 are PERF
            return Self {
                performance_cores: vec![4, 5, 6, 7],
                efficiency_cores: vec![0, 1, 2, 3],
            };
        }

        core_freqs.sort_by_key(|&(idx, _)| idx);
        let max_freq = core_freqs.iter().map(|&(_, f)| f).max().unwrap_or(0);

        if max_freq == 0 {
            // If frequencies couldn't be read, split first half vs second half
            let mid = core_freqs.len() / 2;
            for (idx, _) in core_freqs {
                if idx < mid {
                    eff.push(idx);
                } else {
                    perf.push(idx);
                }
            }
        } else {
            let threshold = (max_freq as f64 * 0.85) as u64;
            for (idx, freq) in core_freqs {
                if freq >= threshold {
                    perf.push(idx);
                } else {
                    eff.push(idx);
                }
            }
        }

        if perf.is_empty() {
            perf = eff.clone();
        }

        Self {
            performance_cores: perf,
            efficiency_cores: eff,
        }
    }
}

/// Pins the calling thread to Performance cores.
pub fn pin_to_performance_cores() -> Result<(), String> {
    let topology = CpuTopology::discover();
    apply_affinity(&topology.performance_cores)
}

/// Pins the calling thread to Efficiency cores.
pub fn pin_to_efficiency_cores() -> Result<(), String> {
    let topology = CpuTopology::discover();
    apply_affinity(&topology.efficiency_cores)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
// Scoped exception to the workspace `unsafe_code = deny` policy: the libc
// affinity calls have no safe wrapper; all unsafe is contained to this fn.
#[allow(unsafe_code)]
fn apply_affinity(cores: &[usize]) -> Result<(), String> {
    if cores.is_empty() {
        return Ok(());
    }
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for &core in cores {
            libc::CPU_SET(core, &mut set);
        }
        let res = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
        if res == 0 {
            Ok(())
        } else {
            Err(format!("sched_setaffinity failed with code {}", res))
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn apply_affinity(_cores: &[usize]) -> Result<(), String> {
    // Non-Linux platforms degrade gracefully to standard thread scheduling
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_topology_discovery() {
        let topo = CpuTopology::discover();
        assert!(!topo.performance_cores.is_empty());
    }

    #[test]
    fn test_pinning_does_not_panic() {
        let _ = pin_to_efficiency_cores();
        let _ = pin_to_performance_cores();
    }

    #[test]
    fn test_apply_affinity_empty_cores_is_noop() {
        assert!(apply_affinity(&[]).is_ok());
        // A real pin may succeed or fail depending on sandbox/permissions;
        // it must just return a Result without panicking.
        let _ = apply_affinity(&[0]);
    }

    #[test]
    fn test_cpu_topology_partitions_cores() {
        let topo = CpuTopology::discover();
        let total = topo.performance_cores.len() + topo.efficiency_cores.len();
        assert!(total > 0);
        // discover() is built on sysfs data; the perf fallback guarantees a
        // non-empty performance set, but eff can be empty on uniform-freq
        // machines (every core lands in perf).
        assert!(!topo.performance_cores.is_empty());
    }
}
