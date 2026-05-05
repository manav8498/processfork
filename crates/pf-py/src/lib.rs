// SPDX-License-Identifier: MIT
//! Python bindings for ProcessFork. Phase 0 scaffold; real surface lands in
//! Phase 7.

// pyo3 0.22's `#[pyfunction]` macro emits unsafe extraction calls that trip
// the edition-2024 `unsafe_op_in_unsafe_fn` lint. The macro audits the safety
// itself; we suppress for the crate. Re-evaluate when bumping pyo3 ≥0.23.
#![allow(unsafe_op_in_unsafe_fn)]

use pyo3::prelude::*;

/// `pf.digest_of(bytes) -> str` — sanity-check that the binding compiles.
///
/// We take `Vec<u8>` rather than `&[u8]` to dodge pyo3 0.22's
/// `unsafe_op_in_unsafe_fn` warning on macro-generated extraction. Clippy
/// would prefer `&[u8]` for the borrow case; the trade-off is documented in
/// `.claude/skills/pyo3-binding/SKILL.md`.
#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
fn digest_of(bytes: Vec<u8>) -> String {
    pf_core::digest::Digest256::of(&bytes).as_str().to_owned()
}

/// Module init.
#[pymodule]
fn _pf_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(digest_of, m)?)?;
    Ok(())
}
