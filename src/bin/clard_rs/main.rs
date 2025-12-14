//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]
#![forbid(unsafe_code)]

use clard_rs::application::APP;

/// Boot ClardRs
fn main() {
    abscissa_core::boot(&APP);
}
