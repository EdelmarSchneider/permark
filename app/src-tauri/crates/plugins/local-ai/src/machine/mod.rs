//! Machine profile (AC-9): memory, processor threads, graphics adapters of this computer, read from OS APIs (D-7).
//!
//! Best effort: every value may be unknown (`None`); reading never panics, never spawns a program (no `nvidia-smi`),
//! never writes. Per OS:
//! - Windows: `GlobalMemoryStatusEx` (memory) + DXGI `EnumAdapters1` / `GetDesc1` (adapters, VRAM, software flag).
//! - macOS: `sysctlbyname` `hw.memsize` (memory; free unknown); Apple Silicon (`hw.optional.arm64`) -> one adapter
//!   with unified memory.
//! - Linux: `/proc/meminfo` (`MemTotal`, `MemAvailable`) + `/sys/class/drm/card<N>/device` (vendor; VRAM only where
//!   the driver tells it: AMD `mem_info_vram_total`). NVIDIA VRAM unknown.
//!
//! Read lazily on the first `local-ai.machine` call (never at plugin start, AC-1), then kept [`FRESH`] (free memory
//! changes). Plugin-private: promote to core when a second consumer appears.

#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;

use crate::engine::Device;

/// How long a read profile is answered again before it is read anew.
pub const FRESH: Duration = Duration::from_secs(30);

/// One graphics adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GpuAdapter")]
pub struct Gpu {
    pub name: String,
    /// Memory of the adapter itself, bytes (`None`: unknown | not separate).
    #[ts(type = "number | null")]
    pub dedicated_vram: Option<u64>,
    /// System memory the adapter may borrow, bytes (`None`: unknown).
    #[ts(type = "number | null")]
    pub shared: Option<u64>,
    /// Shares one memory pool with the processor (Apple Silicon): models may use most of the RAM.
    pub unified: bool,
    /// Drawn by the processor (Microsoft Basic Render Driver): never counts as a graphics adapter.
    pub software: bool,
}

/// What this computer offers (AC-9); unknown values `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "MachineProfile")]
pub struct MachineProfile {
    /// Bytes.
    #[ts(type = "number | null")]
    pub ram_total: Option<u64>,
    /// Bytes free now (`None`: unknown, macOS).
    #[ts(type = "number | null")]
    pub ram_available: Option<u64>,
    /// Logical processors (threads).
    pub cpu_logical: Option<u32>,
    pub gpus: Vec<Gpu>,
    /// `windows` | `macos` | `linux` | other `std::env::consts::OS`.
    pub os: String,
}

impl MachineProfile {
    /// Nothing known (memory | adapters unreadable).
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            ram_total: None,
            ram_available: None,
            cpu_logical: cpu_logical(),
            gpus: Vec::new(),
            os: std::env::consts::OS.to_owned(),
        }
    }

    /// Largest memory of a real (not software) adapter, bytes; `None`: no adapter with known memory.
    #[must_use]
    pub fn best_vram(&self) -> Option<u64> {
        self.gpus
            .iter()
            .filter(|g| !g.software)
            .filter_map(|g| g.dedicated_vram)
            .max()
    }

    /// An adapter shares the processor's memory (Apple Silicon).
    #[must_use]
    pub fn unified(&self) -> bool {
        self.gpus.iter().any(|g| g.unified && !g.software)
    }

    /// What a load on `device` may count on here: processor only -> no graphics adapter (memory guard, fit hints,
    /// budget then reckon without graphics memory).
    #[must_use]
    pub fn limited_to(mut self, device: Device) -> Self {
        if device == Device::Cpu {
            self.gpus.clear();
        }
        self
    }
}

fn cpu_logical() -> Option<u32> {
    std::thread::available_parallelism()
        .ok()
        .and_then(|n| u32::try_from(n.get()).ok())
}

/// Profile of this computer, read now (best effort).
#[must_use]
pub fn detect() -> MachineProfile {
    let mut profile = MachineProfile::unknown();
    #[cfg(windows)]
    windows::read(&mut profile);
    #[cfg(target_os = "macos")]
    macos::read(&mut profile);
    #[cfg(target_os = "linux")]
    linux::read(&mut profile);
    profile
}

/// The profile, read on first ask and again once older than [`FRESH`].
pub struct Machine {
    read: fn() -> MachineProfile,
    kept: Mutex<Option<(Instant, MachineProfile)>>,
}

impl Machine {
    /// Reads with `read` when asked (nothing read now).
    #[must_use]
    pub fn new(read: fn() -> MachineProfile) -> Self {
        Self {
            read,
            kept: Mutex::new(None),
        }
    }

    /// The profile now.
    #[must_use]
    pub fn profile(&self) -> MachineProfile {
        self.profile_at(Instant::now())
    }

    fn profile_at(&self, now: Instant) -> MachineProfile {
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*kept {
            Some((at, profile)) if now.saturating_duration_since(*at) < FRESH => profile.clone(),
            _ => {
                let profile = (self.read)();
                *kept = Some((now, profile.clone()));
                profile
            }
        }
    }
}

#[cfg(test)]
mod tests;
