//! DiskSage core: everything that does not need the UI or the network.
//!
//! - [`scan`]       parallel directory walker that builds folder totals
//! - [`rules`]      known space-eaters (caches, temp, build output, ...)
//! - [`safety`]     protected paths that nothing may ever delete
//! - [`candidates`] picks what to show on the Cleanup screen
//! - [`duplicates`] size -> partial hash -> full hash duplicate finder

pub mod candidates;
pub mod duplicates;
pub mod rules;
pub mod safety;
pub mod scan;
pub mod util;

use serde::{Deserialize, Serialize};

/// How safe it is to move something to the Recycle Bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Safe,
    Caution,
    Keep,
}

impl Verdict {
    pub fn parse(s: &str) -> Option<Verdict> {
        match s.trim().to_ascii_lowercase().as_str() {
            "safe" => Some(Verdict::Safe),
            "caution" => Some(Verdict::Caution),
            "keep" => Some(Verdict::Keep),
            _ => None,
        }
    }
}
