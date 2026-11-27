//! PQ's curve both ways and HLG's inverse OETF by table, read between its
//! steps: too slow to work out per pixel, and the same for every picture,
//! so made once a process.

use std::sync::LazyLock;

use crate::{PQ_PEAK_NITS, hlg_to_scene, nits_to_pq, pq_to_nits};

/// Steps in each table.
const STEPS: usize = 4096;

/// The tables.
pub struct Tables {
    /// The nits at the PQ signals `0, 1 / STEPS, …, 1`.
    pq_nits: Box<[f32]>,
    /// The PQ signals at light whose fourth root over PQ's peak is `0,
    /// 1 / STEPS, …, 1`: even steps there crowd near black, where PQ bends
    /// most.
    pq_signals: Box<[f32]>,
    /// The scene light at the HLG signals `0, 1 / STEPS, …, 1`.
    hlg_scene: Box<[f32]>,
}

impl Tables {
    /// The tables, made the first time they're asked for.
    pub fn get() -> &'static Self {
        static TABLES: LazyLock<Tables> = LazyLock::new(|| {
            let table = |at: &dyn Fn(f32) -> f32| {
                (0..=STEPS)
                    .map(|step| at(step as f32 / STEPS as f32))
                    .collect()
            };
            Tables {
                pq_nits: table(&pq_to_nits),
                pq_signals: table(&|root| nits_to_pq(PQ_PEAK_NITS * root.powi(4))),
                hlg_scene: table(&hlg_to_scene),
            }
        });
        &TABLES
    }

    /// PQ's EOTF: a signal in 0..=1 to nits.
    pub fn pq_nits(&self, signal: f32) -> f32 {
        between(&self.pq_nits, signal)
    }

    /// PQ's inverse EOTF: nits to a signal in 0..=1.
    pub fn pq_signal(&self, nits: f32) -> f32 {
        between(
            &self.pq_signals,
            (nits / PQ_PEAK_NITS).max(0.0).sqrt().sqrt(),
        )
    }

    /// HLG's inverse OETF: a signal in 0..=1 to scene light in 0..=1.
    pub fn hlg_scene(&self, signal: f32) -> f32 {
        between(&self.hlg_scene, signal)
    }
}

/// What `table` holds at `place` in 0..=1, between its steps.
fn between(table: &[f32], place: f32) -> f32 {
    let place = place.clamp(0.0, 1.0) * STEPS as f32;
    let step = (place as usize).min(STEPS - 1);
    let (low, high) = (table[step], table[step + 1]);
    low + (high - low) * (place - step as f32)
}

#[cfg(test)]
#[path = "tests/tables.rs"]
mod tests;
