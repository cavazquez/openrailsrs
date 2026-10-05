//! Linux DRM accounting. Process residency and whole-device usage are distinct:
//! summing either with RSS would double count shared/system-backed allocations.
use bevy::prelude::*;
use serde::Serialize;
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, Default, Serialize)]
pub struct GraphicsMemoryReport {
    pub process_vram_mib: Option<f64>,
    pub peak_process_vram_mib: Option<f64>,
    pub process_gtt_mib: Option<f64>,
    pub device_used_mib: Option<f64>,
    pub device_total_mib: Option<f64>,
    pub source: Option<&'static str>,
}

#[derive(Resource, Default)]
pub struct GraphicsMemory(pub GraphicsMemoryReport);

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

impl GraphicsMemory {
    pub fn sample(&mut self) {
        let Ok(entries) = std::fs::read_dir("/proc/self/fdinfo") else {
            return;
        };
        let mut clients = HashSet::new();
        let mut devices = HashSet::new();
        let mut vram = None;
        let mut gtt = None;
        for entry in entries.flatten() {
            let Some(c) = std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| client(&s))
            else {
                continue;
            };
            // An fd can be duplicated; the driver/client pair identifies one account.
            if !clients.insert((c.pci.clone(), c.id)) {
                continue;
            }
            if let Some(value) = c.vram {
                *vram.get_or_insert(0.0) += value;
            }
            if let Some(value) = c.gtt {
                *gtt.get_or_insert(0.0) += value;
            }
            devices.insert(c.pci);
        }
        let mut used = None;
        let mut total = None;
        for pci in devices {
            let base = Path::new("/sys/bus/pci/devices").join(pci);
            if let (Some(u), Some(t)) = (
                bytes_mib(&base.join("mem_info_vram_used")),
                bytes_mib(&base.join("mem_info_vram_total")),
            ) {
                *used.get_or_insert(0.0) += u;
                *total.get_or_insert(0.0) += t;
            }
        }
        self.0.process_vram_mib = vram;
        self.0.process_gtt_mib = gtt;
        self.0.device_used_mib = used;
        self.0.device_total_mib = total;
        self.0.source = vram.map(|_| "linux_drm_fdinfo");
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
            "VRAM del proceso {} · pico {}\nMemoria compartida del proceso {}\nGPU completa: {} usados / {} totales",
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
    fn drm_reports_residency_and_preserves_unavailable_fields() {
        let c = client("drm-client-id:\t8\ndrm-pdev:\t0000:03:00.0\ndrm-memory-vram:\t4096 KiB\ndrm-resident-vram:\t2048 KiB\ndrm-memory-gtt:\t1048576 bytes\n").unwrap();
        assert_eq!(c.vram, Some(2.0));
        assert_eq!(c.gtt, Some(1.0));
        assert!(client("pos:\t0\nflags:\t0100000\n").is_none());
        assert!(client("drm-client-id: 2\ndrm-pdev: ../../etc\n").is_none());
        assert_eq!(mib("nan KiB"), None);
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
