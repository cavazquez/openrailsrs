//! Linux DRM accounting. Process residency and whole-device usage are distinct:
//! summing either with RSS would double count shared/system-backed allocations.
use bevy::prelude::*;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceMemoryReport {
    pub pci: String,
    pub process_vram_mib: Option<f64>,
    pub process_gtt_mib: Option<f64>,
    pub device_used_mib: Option<f64>,
    pub device_total_mib: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GraphicsMemoryReport {
    pub process_vram_mib: Option<f64>,
    pub peak_process_vram_mib: Option<f64>,
    pub process_gtt_mib: Option<f64>,
    pub device_used_mib: Option<f64>,
    pub device_total_mib: Option<f64>,
    /// Only this device contributes physical capacity/usage to the policy.
    /// Vulkan enumeration can leave idle file descriptors for other GPUs open.
    pub active_device_pci: Option<String>,
    pub devices: Vec<DeviceMemoryReport>,
    pub source: Option<&'static str>,
}

#[derive(Resource, Default)]
pub struct GraphicsMemory(pub GraphicsMemoryReport);

/// Residency pressure uses hysteresis so cosmetic allocations cannot oscillate.
#[derive(Resource, Default)]
pub struct ResidencyPolicy {
    pub pressured: bool,
    pub transitions: u64,
    pub budget_mib: Option<f64>,
    pub basis: Option<&'static str>,
}
impl ResidencyPolicy {
    pub fn observe(&mut self, used: Option<f64>, total: Option<f64>) {
        let Some((used, total)) = used
            .zip(total)
            .filter(|(u, t)| u.is_finite() && t.is_finite() && *t > 0.)
        else {
            return;
        };
        let next = if self.pressured {
            used / total > 0.78
        } else {
            used / total >= 0.90
        };
        if next != self.pressured {
            self.pressured = next;
            self.transitions += 1;
        }
    }
    pub fn keep_radius(&self, view: f32) -> f32 {
        if self.pressured {
            view + 2048.
        } else {
            view + 4096.
        }
    }
}
pub fn update_residency(memory: Res<GraphicsMemory>, mut policy: ResMut<ResidencyPolicy>) {
    let configured = std::env::var("OPENRAILSRS_VRAM_BUDGET_MIB")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| (256..=65536).contains(n))
        .map(f64::from);
    if let Some(budget) = configured {
        policy.budget_mib = Some(budget);
        policy.basis = Some("measured process residency / configured budget");
        policy.observe(memory.0.process_vram_mib, Some(budget));
    } else {
        policy.budget_mib = memory.0.device_total_mib;
        policy.basis = Some("measured active-device usage / physical device total");
        policy.observe(memory.0.device_used_mib, memory.0.device_total_mib);
    }
}

#[derive(Debug, PartialEq)]
struct DrmClient {
    id: String,
    pci: String,
    vram: Option<f64>,
    gtt: Option<f64>,
}

fn mib(line: &str) -> Option<f64> {
    let mut words = line.split_whitespace();
    let number = words.next()?.parse::<f64>().ok()?;
    let divisor = match words.next()? {
        "KiB" | "kB" => 1024.0,
        "MiB" => 1.0,
        "bytes" | "B" => 1024.0 * 1024.0,
        _ => return None,
    };
    (number.is_finite() && number >= 0.0).then_some(number / divisor)
}

fn client(text: &str) -> Option<DrmClient> {
    let field = |key: &str| text.lines().find_map(|line| line.strip_prefix(key));
    let id = field("drm-client-id:")?.trim().to_owned();
    let pci = field("drm-pdev:")?.trim().to_owned();
    if !pci
        .chars()
        .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
    {
        return None;
    }
    Some(DrmClient {
        id,
        pci,
        vram: field("drm-resident-vram:")
            .and_then(mib)
            .or_else(|| field("drm-memory-vram:").and_then(mib)),
        gtt: field("drm-resident-gtt:")
            .and_then(mib)
            .or_else(|| field("drm-memory-gtt:").and_then(mib)),
    })
}

fn bytes_mib(path: &Path) -> Option<f64> {
    let bytes = std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(bytes as f64 / (1024.0 * 1024.0))
}

fn device_accounts(clients: impl IntoIterator<Item = DrmClient>) -> Vec<DeviceMemoryReport> {
    let mut seen = HashSet::new();
    let mut devices: BTreeMap<String, DeviceMemoryReport> = BTreeMap::new();
    for c in clients {
        // Duplicate FDs do not represent additional GPU allocations.
        if !seen.insert((c.pci.clone(), c.id)) {
            continue;
        }
        let device = devices
            .entry(c.pci.clone())
            .or_insert_with(|| DeviceMemoryReport {
                pci: c.pci,
                ..Default::default()
            });
        if let Some(vram) = c.vram {
            *device.process_vram_mib.get_or_insert(0.) += vram;
        }
        if let Some(gtt) = c.gtt {
            *device.process_gtt_mib.get_or_insert(0.) += gtt;
        }
    }
    devices.into_values().collect()
}

fn active_device(devices: &[DeviceMemoryReport]) -> Option<&DeviceMemoryReport> {
    // GTT matters on integrated GPUs. Dedicated VRAM alone would select an
    // idle discrete GPU over the renderer that owns shared allocations.
    devices
        .iter()
        .filter_map(|d| {
            let amount = d.process_vram_mib.unwrap_or(0.) + d.process_gtt_mib.unwrap_or(0.);
            (amount > 0.).then_some((d, amount))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(d, _)| d)
}

fn sum_available(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let mut sum = None;
    for value in values.into_iter().flatten() {
        *sum.get_or_insert(0.) += value;
    }
    sum
}

impl GraphicsMemory {
    pub fn sample(&mut self) {
        let Ok(entries) = std::fs::read_dir("/proc/self/fdinfo") else {
            return;
        };
        let mut devices = device_accounts(entries.flatten().filter_map(|entry| {
            std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| client(&s))
        }));
        for device in &mut devices {
            let base = Path::new("/sys/bus/pci/devices").join(&device.pci);
            device.device_used_mib = bytes_mib(&base.join("mem_info_vram_used"));
            device.device_total_mib = bytes_mib(&base.join("mem_info_vram_total"));
        }
        let vram = sum_available(devices.iter().map(|d| d.process_vram_mib));
        let gtt = sum_available(devices.iter().map(|d| d.process_gtt_mib));
        let active = active_device(&devices);
        self.0.process_vram_mib = vram;
        self.0.process_gtt_mib = gtt;
        self.0.active_device_pci = active.map(|d| d.pci.clone());
        self.0.device_used_mib = active.and_then(|d| d.device_used_mib);
        self.0.device_total_mib = active.and_then(|d| d.device_total_mib);
        self.0.devices = devices;
        self.0.source = (vram.is_some() || gtt.is_some()).then_some("linux_drm_fdinfo");
        if let Some(value) = vram {
            self.0.peak_process_vram_mib =
                Some(self.0.peak_process_vram_mib.unwrap_or(0.0).max(value));
        }
    }

    pub fn pressure(&self) -> bool {
        self.0
            .device_used_mib
            .zip(self.0.device_total_mib)
            .is_some_and(|(used, total)| total > 0.0 && used / total > 0.90)
    }

    pub fn hud_text(&self) -> String {
        let amount =
            |n: Option<f64>| n.map_or_else(|| "no disponible".into(), |v| format!("{v:.0} MiB"));
        format!(
            "VRAM del proceso {} · pico {}\nMemoria compartida del proceso {}\nGPU activa: {} usados / {} totales",
            amount(self.0.process_vram_mib),
            amount(self.0.peak_process_vram_mib),
            amount(self.0.process_gtt_mib),
            amount(self.0.device_used_mib),
            amount(self.0.device_total_mib)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pressure_releases_hysteresis_only_after_available_memory_recovers() {
        let mut policy = ResidencyPolicy::default();
        for used in [90., 87., 82., 79.] {
            policy.observe(Some(used), Some(100.));
            assert!(policy.pressured);
        }
        assert_eq!(policy.transitions, 1);
        policy.observe(Some(77.), Some(100.));
        assert!(!policy.pressured);
        policy.observe(None, None);
        assert_eq!(policy.transitions, 2);
    }
    #[test]
    fn drm_reports_residency_and_preserves_unavailable_fields() {
        let c = client("drm-client-id:\t8\ndrm-pdev:\t0000:03:00.0\ndrm-memory-vram:\t4096 KiB\ndrm-resident-vram:\t2048 KiB\ndrm-memory-gtt:\t1048576 bytes\n").unwrap();
        assert_eq!(c.vram, Some(2.0));
        assert_eq!(c.gtt, Some(1.0));
        assert!(client("pos:\t0\nflags:\t0100000\n").is_none());
        assert!(client("drm-client-id: 2\ndrm-pdev: ../../etc\n").is_none());
        assert_eq!(mib("nan KiB"), None);
    }
    #[test]
    fn idle_second_gpu_and_duplicate_fds_do_not_inflate_active_capacity() {
        let lines = [
            "drm-client-id: 1\ndrm-pdev: 0000:03:00.0\ndrm-resident-vram: 0 KiB\ndrm-resident-gtt: 0 KiB\n",
            "drm-client-id: 2\ndrm-pdev: 0000:0b:00.0\ndrm-resident-vram: 0 KiB\ndrm-resident-gtt: 1048576 KiB\n",
            "drm-client-id: 2\ndrm-pdev: 0000:0b:00.0\ndrm-resident-vram: 0 KiB\ndrm-resident-gtt: 1048576 KiB\n",
        ];
        let mut devices = device_accounts(lines.into_iter().filter_map(client));
        assert_eq!(devices.len(), 2);
        devices[0].device_total_mib = Some(8192.);
        devices[1].device_total_mib = Some(496.);
        let active = active_device(&devices).unwrap();
        assert_eq!(active.pci, "0000:0b:00.0");
        assert_eq!(active.device_total_mib, Some(496.));
        assert_eq!(
            sum_available(devices.iter().map(|d| d.process_gtt_mib)),
            Some(1024.)
        );
        assert_eq!(sum_available([None, None]), None);
        devices[0].process_vram_mib = Some(2048.);
        assert_eq!(active_device(&devices).unwrap().pci, "0000:03:00.0");
    }
    #[test]
    fn no_memory_data_does_not_mean_zero_or_memory_pressure() {
        let mut m = GraphicsMemory::default();
        assert!(!m.pressure());
        assert!(m.hud_text().contains("no disponible"));
        m.0.device_used_mib = Some(950.0);
        m.0.device_total_mib = Some(1000.0);
        assert!(m.pressure());
    }
}
