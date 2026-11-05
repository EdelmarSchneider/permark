//! Memory guard (AC-5, D-8 v1): before a load, catalog `needs` vs this computer's memory now; too little -> refused
//! ([`EngineError::OutOfMemory`]), no load attempted.
//!
//! - Need = `ram_min` (files) + [`MARGIN`] (context + runtime).
//! - Room = free RAM now (OS available; unknown (macOS) -> RAM total share of the budget rule,
//!   [`crate::fit::ram_share`]) + best real adapter VRAM ([`MachineProfile::best_vram`]; unified memory = one pool,
//!   VRAM never added). RAM unknown entirely -> allowed (the engine's own fit at load decides).
//! - Loads are serialized (runtime): free RAM read now already holds the loaded sessions -> never subtracted again.

use crate::catalog::Needs;
use crate::engine::EngineError;
use crate::fit::ram_share;
use crate::machine::MachineProfile;

/// Room kept beyond the files: context + runtime.
pub const MARGIN: u64 = 512 << 20;

/// Whether a model wanting `needs` may load on `profile` (rules: module doc).
///
/// # Errors
/// [`EngineError::OutOfMemory`].
pub fn check(needs: &Needs, profile: &MachineProfile) -> Result<(), EngineError> {
    let Some(ram) = profile
        .ram_available
        .or_else(|| profile.ram_total.map(ram_share))
    else {
        return Ok(());
    };
    let vram = if profile.unified() {
        0
    } else {
        profile.best_vram().unwrap_or(0)
    };
    if needs.ram_min.saturating_add(MARGIN) > ram.saturating_add(vram) {
        return Err(EngineError::OutOfMemory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Gpu;

    const GIB: u64 = 1 << 30;

    fn gpu(dedicated: Option<u64>, unified: bool, software: bool) -> Gpu {
        Gpu {
            name: "g".into(),
            dedicated_vram: dedicated,
            shared: None,
            unified,
            software,
        }
    }

    /// Case: (name, RAM total, RAM available, adapters, `ram_min`, fits).
    type Case = (&'static str, Option<u64>, Option<u64>, Vec<Gpu>, u64, bool);

    fn cases() -> Vec<Case> {
        vec![
            (
                "free ram covers it",
                Some(16 * GIB),
                Some(4 * GIB),
                vec![],
                3 * GIB,
                true,
            ),
            (
                "margin counts",
                Some(16 * GIB),
                Some(3 * GIB),
                vec![],
                3 * GIB,
                false,
            ),
            (
                "exactly enough",
                Some(16 * GIB),
                Some(3 * GIB + MARGIN),
                vec![],
                3 * GIB,
                true,
            ),
            (
                "free ram low: total ignored",
                Some(64 * GIB),
                Some(GIB),
                vec![],
                2 * GIB,
                false,
            ),
            (
                "vram adds to free ram",
                Some(16 * GIB),
                Some(GIB),
                vec![gpu(Some(8 * GIB), false, false)],
                6 * GIB,
                true,
            ),
            (
                "software adapter never counts",
                Some(16 * GIB),
                Some(GIB),
                vec![gpu(Some(32 * GIB), false, true)],
                6 * GIB,
                false,
            ),
            (
                "unknown vram counts as none",
                Some(16 * GIB),
                Some(GIB),
                vec![gpu(None, false, false)],
                6 * GIB,
                false,
            ),
            // macOS: no free figure -> 60 % of total, one pool
            (
                "unified: share of total",
                Some(16 * GIB),
                None,
                vec![gpu(None, true, false)],
                9 * GIB,
                true,
            ),
            (
                "unified: above the share",
                Some(16 * GIB),
                None,
                vec![gpu(Some(16 * GIB), true, false)],
                10 * GIB,
                false,
            ),
            (
                "nothing known: allowed",
                None,
                None,
                vec![],
                100 * GIB,
                true,
            ),
        ]
    }

    #[test]
    fn needs_plus_margin_against_free_memory() {
        for (name, total, available, gpus, ram_min, fits) in cases() {
            let profile = MachineProfile {
                ram_total: total,
                ram_available: available,
                cpu_logical: Some(8),
                gpus,
                os: "test".into(),
            };
            let needs = Needs {
                ram_min,
                ram_rec: ram_min * 2,
                vram_rec: ram_min * 2,
            };
            let expected = if fits {
                Ok(())
            } else {
                Err(EngineError::OutOfMemory)
            };
            assert_eq!(check(&needs, &profile), expected, "{name}");
        }
    }
}
