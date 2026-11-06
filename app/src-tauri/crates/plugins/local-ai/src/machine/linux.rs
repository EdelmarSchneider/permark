//! Linux: memory from `/proc/meminfo`, adapters from `/sys/class/drm/card<N>/device` (plain files, no unsafe).
//!
//! Parsers are pure and built on every OS for the tests; only [`read`] touches the real files.

use std::path::Path;

use super::Gpu;

#[cfg(target_os = "linux")]
pub(super) fn read(profile: &mut super::MachineProfile) {
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        let (total, available) = meminfo(&text);
        profile.ram_total = total;
        profile.ram_available = available;
    }
    profile.gpus = drm_gpus(Path::new("/sys/class/drm"));
}

/// `MemTotal` + `MemAvailable` of `/proc/meminfo` text, bytes (values are kB).
pub(super) fn meminfo(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            let kb = rest.trim().strip_suffix("kB").unwrap_or(rest).trim();
            kb.parse::<u64>().ok()?.checked_mul(1024)
        })
    };
    (field("MemTotal"), field("MemAvailable"))
}

/// Adapters under a `/sys/class/drm` folder: one per `card<N>` (connectors `card<N>-<port>` skipped), by number.
pub(super) fn drm_gpus(drm: &Path) -> Vec<Gpu> {
    let Ok(dir) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<(u32, std::path::PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let n = name.to_str()?.strip_prefix("card")?.parse::<u32>().ok()?;
            Some((n, e.path().join("device")))
        })
        .collect();
    cards.sort_by_key(|(n, _)| *n);
    cards
        .into_iter()
        .filter_map(|(_, device)| card(&device))
        .collect()
}

fn card(device: &Path) -> Option<Gpu> {
    let text = |file: &str| {
        std::fs::read_to_string(device.join(file))
            .ok()
            .map(|s| s.trim().to_owned())
    };
    let number = |file: &str| text(file)?.parse::<u64>().ok();
    let vendor = text("vendor")?;
    let brand = match vendor.as_str() {
        "0x1002" => "AMD",
        "0x10de" => "NVIDIA",
        "0x8086" => "Intel",
        other => other,
    };
    let name = match text("device") {
        Some(id) => format!("{brand} {id}"),
        None => brand.to_owned(),
    };
    Some(Gpu {
        name,
        // AMD (amdgpu) only; NVIDIA | Intel: unknown
        dedicated_vram: number("mem_info_vram_total"),
        shared: number("mem_info_gtt_total"),
        unified: false,
        software: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:       16318412 kB\nMemFree:         1023456 kB\n\
                           MemAvailable:    9876543 kB\nBuffers:          123456 kB\n";

    #[test]
    fn meminfo_gives_total_and_available_in_bytes() {
        assert_eq!(
            meminfo(MEMINFO),
            (Some(16_318_412 * 1024), Some(9_876_543 * 1024))
        );
    }

    #[test]
    fn meminfo_without_the_fields_is_unknown() {
        assert_eq!(meminfo("MemFree: 12 kB\nMemTotalX: 3 kB\n"), (None, None));
        assert_eq!(meminfo("MemTotal: lots kB\n"), (None, None));
        assert_eq!(meminfo(""), (None, None));
    }

    fn put(root: &Path, card: &str, files: &[(&str, &str)]) {
        let device = root.join(card).join("device");
        std::fs::create_dir_all(&device).unwrap();
        for (name, value) in files {
            std::fs::write(device.join(name), format!("{value}\n")).unwrap();
        }
    }

    #[test]
    fn drm_cards_give_vendor_and_amd_vram() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put(
            root,
            "card1",
            &[
                ("vendor", "0x1002"),
                ("device", "0x73bf"),
                ("mem_info_vram_total", "17163091968"),
                ("mem_info_gtt_total", "8589934592"),
            ],
        );
        put(root, "card0", &[("vendor", "0x10de"), ("device", "0x2882")]);
        // connector of card1: not an adapter
        put(root, "card1-HDMI-A-1", &[("vendor", "0x1002")]);
        // no vendor file: skipped
        std::fs::create_dir_all(root.join("card2").join("device")).unwrap();
        std::fs::write(root.join("version"), "drm 1.1.0").unwrap();
        assert_eq!(
            drm_gpus(root),
            vec![
                Gpu {
                    name: "NVIDIA 0x2882".into(),
                    dedicated_vram: None,
                    shared: None,
                    unified: false,
                    software: false,
                },
                Gpu {
                    name: "AMD 0x73bf".into(),
                    dedicated_vram: Some(17_163_091_968),
                    shared: Some(8_589_934_592),
                    unified: false,
                    software: false,
                },
            ]
        );
    }

    #[test]
    fn a_missing_drm_folder_gives_no_adapter() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(drm_gpus(&tmp.path().join("absent")), Vec::<Gpu>::new());
    }
}
