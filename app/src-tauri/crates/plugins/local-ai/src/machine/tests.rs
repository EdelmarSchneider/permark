use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static READS: AtomicUsize = AtomicUsize::new(0);

fn counted() -> MachineProfile {
    READS.fetch_add(1, Ordering::SeqCst);
    MachineProfile::unknown()
}

/// AC-1 dormant: nothing read until asked; then kept for [`FRESH`], read anew after.
#[test]
fn the_profile_is_read_on_first_ask_and_kept_while_fresh() {
    let machine = Machine::new(counted);
    assert_eq!(READS.load(Ordering::SeqCst), 0);
    let t0 = Instant::now();
    let _ = machine.profile_at(t0);
    assert_eq!(READS.load(Ordering::SeqCst), 1);
    let _ = machine.profile_at(t0 + FRESH / 2);
    assert_eq!(READS.load(Ordering::SeqCst), 1);
    let _ = machine.profile_at(t0 + FRESH);
    assert_eq!(READS.load(Ordering::SeqCst), 2);
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

#[test]
fn best_vram_skips_software_and_unknown_adapters() {
    let mut p = MachineProfile::unknown();
    assert_eq!(p.best_vram(), None);
    p.gpus = vec![
        gpu(Some(64 << 30), false, true),
        gpu(None, false, false),
        gpu(Some(2 << 30), false, false),
        gpu(Some(8 << 30), false, false),
    ];
    assert_eq!(p.best_vram(), Some(8 << 30));
    assert!(!p.unified());
    p.gpus.push(gpu(None, true, false));
    assert!(p.unified());
}

/// A processor-only load counts on no graphics adapter (memory guard, fit hints, budget); any other load on the
/// profile as read.
#[test]
fn a_processor_only_load_sees_no_graphics_adapter() {
    let mut p = MachineProfile::unknown();
    p.gpus = vec![gpu(Some(8 << 30), false, false), gpu(None, true, false)];
    assert_eq!(p.clone().limited_to(Device::Auto), p);
    let cpu = p.clone().limited_to(Device::Cpu);
    assert_eq!(cpu.best_vram(), None);
    assert!(!cpu.unified());
    assert_eq!(
        cpu,
        MachineProfile {
            gpus: Vec::new(),
            ..p
        }
    );
}

/// Real detection on this computer (prints it): `cargo test -p permark-plugin-local-ai -- --ignored --nocapture
/// machine`.
#[test]
#[ignore = "reads this computer; run by hand"]
fn print_this_machine() {
    let profile = detect();
    println!("{profile:#?}");
    assert_eq!(profile.os, std::env::consts::OS);
}
