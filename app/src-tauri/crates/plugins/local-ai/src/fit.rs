//! Fit hint + memory budget (AC-9, D-3 heuristic v1 on catalog `needs`; refined in C4 by engine `estimate_memory`).
//! Advice only: nothing here blocks a Get | an assignment.
//!
//! Fit of one model, first rule that holds:
//! 1. best real adapter VRAM ([`MachineProfile::best_vram`]) >= `vram_rec` -> [`Fit::RunsWell`].
//! 2. unified memory (Apple Silicon) and RAM total >= `ram_rec` -> [`Fit::RunsWell`].
//! 3. RAM total unknown -> [`Fit::Unknown`].
//! 4. RAM total >= `ram_rec` -> [`Fit::SlowCpuOnly`].
//! 5. else (below `ram_rec`, below `ram_min` included) -> [`Fit::NotRecommended`]: `ram_min` = files only, no room
//!    left for the context.
//!
//! An adapter whose VRAM is unknown (NVIDIA on Linux) counts as none.
//!
//! Budget: `ram_rec` summed over the distinct models assigned to tasks vs best VRAM (0 if unknown) + 60 % of RAM
//! total (the rest kept for the system + PerMark) -> [`BudgetWarning`] when above; RAM total unknown -> no warning.
//!
//! Models kept on the processor (the person's choice, [`advise`] `device`): fit + budget as on a computer without a
//! graphics adapter ([`MachineProfile::limited_to`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use ts_rs::TS;

use crate::catalog::{Catalog, Needs};
use crate::engine::Device;
use crate::machine::MachineProfile;

/// What this computer can do with one model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ModelFit")]
pub enum Fit {
    RunsWell,
    SlowCpuOnly,
    NotRecommended,
    Unknown,
}

/// Assigned models together want more memory than this computer can give them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BudgetWarning {
    /// Bytes the assigned models want.
    #[ts(type = "number")]
    pub needed: u64,
    /// Bytes they may use here.
    #[ts(type = "number")]
    pub available: u64,
}

/// RPC `local-ai.machine`: the profile, a fit per catalog model, the budget warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MachineInfo {
    pub profile: MachineProfile,
    /// Catalog model id -> fit.
    pub fits: BTreeMap<String, Fit>,
    pub budget: Option<BudgetWarning>,
}

/// Fit of a model wanting `needs` here (rules: module doc).
#[must_use]
pub fn fit(needs: &Needs, profile: &MachineProfile) -> Fit {
    if profile.best_vram().is_some_and(|v| v >= needs.vram_rec) {
        return Fit::RunsWell;
    }
    let Some(ram) = profile.ram_total else {
        return Fit::Unknown;
    };
    if ram >= needs.ram_rec {
        if profile.unified() {
            Fit::RunsWell
        } else {
            Fit::SlowCpuOnly
        }
    } else {
        Fit::NotRecommended
    }
}

/// Share of RAM total models may use (the rest kept for the system + PerMark): 60 %.
#[must_use]
pub(crate) const fn ram_share(total: u64) -> u64 {
    total / 5 * 3
}

/// Warning when the models wanting `assigned` exceed this computer's budget (rules: module doc).
#[must_use]
pub fn budget<'a>(
    assigned: impl IntoIterator<Item = &'a Needs>,
    profile: &MachineProfile,
) -> Option<BudgetWarning> {
    let ram = profile.ram_total?;
    let needed = assigned
        .into_iter()
        .fold(0u64, |sum, n| sum.saturating_add(n.ram_rec));
    let available = profile
        .best_vram()
        .unwrap_or(0)
        .saturating_add(ram_share(ram));
    (needed > available).then_some(BudgetWarning { needed, available })
}

/// Advice for the catalog given the ids of the models assigned to tasks (repeats counted once; unknown ids skipped)
/// and where the person lets models run (`device`).
#[must_use]
pub fn advise(
    catalog: &Catalog,
    profile: MachineProfile,
    assigned: &[String],
    device: Device,
) -> MachineInfo {
    // the profile told stays whole (the page shows the adapters); the advice reckons with what a load may use
    let usable = profile.clone().limited_to(device);
    let fits = catalog
        .models
        .iter()
        .map(|m| (m.id.clone(), fit(&m.needs, &usable)))
        .collect();
    let distinct: BTreeSet<&str> = assigned.iter().map(String::as_str).collect();
    let budget = budget(
        distinct
            .into_iter()
            .filter_map(|id| catalog.model(id))
            .map(|m| &m.needs),
        &usable,
    );
    MachineInfo {
        profile,
        fits,
        budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Gpu;

    const GIB: u64 = 1 << 30;

    fn needs(ram_min: u64, ram_rec: u64) -> Needs {
        Needs {
            ram_min,
            ram_rec,
            vram_rec: ram_rec,
        }
    }

    fn gpu(dedicated: Option<u64>, unified: bool, software: bool) -> Gpu {
        Gpu {
            name: "g".into(),
            dedicated_vram: dedicated,
            shared: None,
            unified,
            software,
        }
    }

    fn machine(ram: Option<u64>, gpus: Vec<Gpu>) -> MachineProfile {
        MachineProfile {
            ram_total: ram,
            ram_available: None,
            cpu_logical: Some(8),
            gpus,
            os: "windows".into(),
        }
    }

    #[test]
    fn enough_vram_runs_well_even_with_little_ram() {
        let p = machine(Some(2 * GIB), vec![gpu(Some(8 * GIB), false, false)]);
        assert_eq!(fit(&needs(4 * GIB, 5 * GIB), &p), Fit::RunsWell);
        // exactly the recommended VRAM is enough
        assert_eq!(fit(&needs(4 * GIB, 8 * GIB), &p), Fit::RunsWell);
    }

    #[test]
    fn a_software_adapter_never_counts() {
        let p = machine(Some(16 * GIB), vec![gpu(Some(32 * GIB), false, true)]);
        assert_eq!(fit(&needs(4 * GIB, 5 * GIB), &p), Fit::SlowCpuOnly);
    }

    #[test]
    fn unified_memory_runs_well_when_ram_covers_the_recommendation() {
        let p = machine(Some(16 * GIB), vec![gpu(None, true, false)]);
        assert_eq!(fit(&needs(4 * GIB, 5 * GIB), &p), Fit::RunsWell);
        assert_eq!(fit(&needs(4 * GIB, 20 * GIB), &p), Fit::NotRecommended);
    }

    #[test]
    fn enough_ram_without_a_fitting_adapter_is_slow() {
        let p = machine(Some(16 * GIB), vec![gpu(Some(2 * GIB), false, false)]);
        assert_eq!(fit(&needs(4 * GIB, 5 * GIB), &p), Fit::SlowCpuOnly);
        assert_eq!(fit(&needs(4 * GIB, 16 * GIB), &p), Fit::SlowCpuOnly);
    }

    #[test]
    fn ram_below_the_recommendation_is_not_recommended() {
        let p = machine(Some(6 * GIB), Vec::new());
        // below the minimum
        assert_eq!(fit(&needs(7 * GIB, 8 * GIB), &p), Fit::NotRecommended);
        // files fit, the context does not
        assert_eq!(fit(&needs(5 * GIB, 7 * GIB), &p), Fit::NotRecommended);
    }

    #[test]
    fn unknown_ram_without_a_fitting_adapter_is_unknown() {
        let p = machine(None, vec![gpu(None, false, false)]);
        assert_eq!(fit(&needs(GIB, 2 * GIB), &p), Fit::Unknown);
        let p = machine(None, vec![gpu(Some(4 * GIB), false, false)]);
        assert_eq!(fit(&needs(GIB, 2 * GIB), &p), Fit::RunsWell);
    }

    #[test]
    fn budget_warns_only_above_vram_plus_60_percent_of_ram() {
        let p = machine(Some(10 * GIB), vec![gpu(Some(2 * GIB), false, false)]);
        // budget: 2 + 6 = 8 GiB
        let small = needs(GIB, 3 * GIB);
        let big = needs(4 * GIB, 5 * GIB);
        assert_eq!(budget([&small, &big], &p), None);
        assert_eq!(
            budget([&big, &big], &p),
            Some(BudgetWarning {
                needed: 10 * GIB,
                available: 8 * GIB,
            })
        );
        assert_eq!(budget([], &p), None);
    }

    #[test]
    fn budget_is_silent_when_ram_is_unknown() {
        let p = machine(None, vec![gpu(Some(GIB), false, false)]);
        assert_eq!(budget([&needs(GIB, 100 * GIB)], &p), None);
    }

    #[test]
    fn advice_counts_each_assigned_model_once() {
        let catalog = crate::catalog::bundled().unwrap();
        let big = catalog
            .models
            .iter()
            .max_by_key(|m| m.needs.ram_rec)
            .unwrap();
        let p = machine(Some(big.needs.ram_rec * 3 / 2), Vec::new());
        // budget 0.6 x 1.5 x rec = 0.9 x rec < rec: one model warns; the same id twice warns the same
        let once = advise(
            catalog,
            p.clone(),
            std::slice::from_ref(&big.id),
            Device::Auto,
        );
        let twice = advise(catalog, p, &[big.id.clone(), big.id.clone()], Device::Auto);
        assert_eq!(once.budget, twice.budget);
        assert_eq!(once.budget.map(|b| b.needed), Some(big.needs.ram_rec));
        assert_eq!(once.fits.len(), catalog.models.len());
        assert_eq!(once.fits[&big.id], Fit::SlowCpuOnly);
    }

    /// Processor only: hints + budget as on a computer without a graphics card; the profile told keeps its
    /// adapters (the page still shows them).
    #[test]
    fn processor_only_advice_counts_no_graphics_memory() {
        let catalog = crate::catalog::bundled().unwrap();
        let big = catalog
            .models
            .iter()
            .max_by_key(|m| m.needs.ram_rec)
            .unwrap();
        let ram = big.needs.ram_rec * 3 / 2;
        let p = machine(Some(ram), vec![gpu(Some(big.needs.vram_rec), false, false)]);
        let assigned = std::slice::from_ref(&big.id);
        let auto = advise(catalog, p.clone(), assigned, Device::Auto);
        assert_eq!(auto.fits[&big.id], Fit::RunsWell);
        assert_eq!(auto.budget, None);
        let cpu = advise(catalog, p.clone(), assigned, Device::Cpu);
        assert_eq!(cpu.fits[&big.id], Fit::SlowCpuOnly);
        assert_eq!(
            cpu.budget,
            Some(BudgetWarning {
                needed: big.needs.ram_rec,
                available: ram_share(ram),
            })
        );
        assert_eq!(cpu.profile, p);
    }
}
