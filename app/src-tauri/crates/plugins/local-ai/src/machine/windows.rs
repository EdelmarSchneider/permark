//! Windows: memory through `GlobalMemoryStatusEx`, graphics adapters through DXGI (no COM apartment needed).
//!
//! The only unsafe code of this module: plain Win32 / DXGI calls on values this module owns.
#![allow(unsafe_code)]

use ::windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use super::{Gpu, MachineProfile};

/// Adapters asked at most (a guard: DXGI ends the list with `DXGI_ERROR_NOT_FOUND`).
const MAX_ADAPTERS: u32 = 16;

pub(super) fn read(profile: &mut MachineProfile) {
    if let Some((total, available)) = memory() {
        profile.ram_total = Some(total);
        profile.ram_available = Some(available);
    }
    profile.gpus = adapters();
}

fn memory() -> Option<(u64, u64)> {
    // SAFETY: MEMORYSTATUSEX is plain data (all-zero valid); `dwLength` set as the API requires.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
    // SAFETY: `status` is a valid, writable MEMORYSTATUSEX that outlives the call.
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut status) };
    (ok != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
}

fn adapters() -> Vec<Gpu> {
    // SAFETY: no arguments; the factory is a COM object released on drop.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut gpus = Vec::new();
    for index in 0..MAX_ADAPTERS {
        // SAFETY: `factory` is a live IDXGIFactory1; an index past the end answers an error (loop ends).
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        // SAFETY: `adapter` is a live IDXGIAdapter1; the description is copied out by value.
        if let Ok(desc) = unsafe { adapter.GetDesc1() } {
            gpus.push(gpu_of(&desc));
        }
    }
    gpus
}

/// One adapter from its DXGI description: NUL-ended UTF-16 name, byte counts, software flag.
pub(super) fn gpu_of(desc: &DXGI_ADAPTER_DESC1) -> Gpu {
    let end = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    let software_bit = u32::try_from(DXGI_ADAPTER_FLAG_SOFTWARE.0).unwrap_or(0);
    let bytes = |n: usize| u64::try_from(n).ok();
    Gpu {
        name: String::from_utf16_lossy(&desc.Description[..end])
            .trim()
            .to_owned(),
        dedicated_vram: bytes(desc.DedicatedVideoMemory),
        shared: bytes(desc.SharedSystemMemory),
        unified: false,
        software: desc.Flags & software_bit != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(name: &str, dedicated: usize, shared: usize, flags: u32) -> DXGI_ADAPTER_DESC1 {
        let mut d = DXGI_ADAPTER_DESC1::default();
        for (slot, c) in d.Description.iter_mut().zip(name.encode_utf16()) {
            *slot = c;
        }
        d.DedicatedVideoMemory = dedicated;
        d.SharedSystemMemory = shared;
        d.Flags = flags;
        d
    }

    #[test]
    fn a_description_becomes_an_adapter() {
        let gpu = gpu_of(&desc("NVIDIA GeForce RTX 4060 ", 8 << 30, 16 << 30, 0));
        assert_eq!(
            gpu,
            Gpu {
                name: "NVIDIA GeForce RTX 4060".into(),
                dedicated_vram: Some(8 << 30),
                shared: Some(16 << 30),
                unified: false,
                software: false,
            }
        );
    }

    #[test]
    fn the_basic_render_driver_is_software() {
        let gpu = gpu_of(&desc("Microsoft Basic Render Driver", 0, 1 << 30, 2));
        assert!(gpu.software);
        assert_eq!(gpu.dedicated_vram, Some(0));
    }

    #[test]
    fn a_full_length_name_without_end_mark_is_kept() {
        let long = "x".repeat(128);
        assert_eq!(gpu_of(&desc(&long, 0, 0, 0)).name, long);
    }
}
