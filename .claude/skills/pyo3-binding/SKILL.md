---
name: pyo3-binding
description: Patterns for exposing ProcessFork Rust APIs to Python via pyo3 0.22 and maturin.
---

# pyo3 binding patterns

## Module shape

- The cdylib lib name MUST match the Python import name:
  `[lib] name = "_pf_py"` → `import _pf_py`.
- The Python-facing package wraps the cdylib for ergonomics:
  `crates/pf-py/python/processfork/__init__.py` re-exports.
- Use `#[pymodule] fn _pf_py(m: &Bound<'_, PyModule>) -> PyResult<()>`
  signature (pyo3 0.22 bound API). Old `&PyModule` is deprecated.

## Edition 2024 gotcha

pyo3 0.22's `#[pyfunction]` macro emits unsafe extraction calls that trip
the `unsafe_op_in_unsafe_fn` lint under edition 2024. Either:
- Place `#![allow(unsafe_op_in_unsafe_fn)]` at the top of `lib.rs` (current
  approach), with a TODO to revisit on pyo3 ≥0.23.
- Or accept owned `Vec<u8>` instead of `&[u8]` (avoids one class of warning
  but not all).

## Async

Use `pyo3-asyncio` for `async fn` exposure. Pattern:

```rust
#[pyfunction]
fn snapshot<'p>(py: Python<'p>, agent_id: String) -> PyResult<&'p PyAny> {
    pyo3_asyncio::tokio::future_into_py(py, async move {
        let cid = pf_core::snapshot(&agent_id).await?;
        Ok(cid.to_string())
    })
}
```

## Errors

Wrap `pf_core::Error` into `PyErr` via a manual `From` impl in
`pf-py/src/errors.rs`. Map `Error::Integrity` → `PyValueError`,
`Error::Io` → `PyOSError`, etc. Never swallow with a generic `RuntimeError`.

## Type stubs

Hand-write `crates/pf-py/python/processfork/_pf_py.pyi` so end-users get
type hints. Keep it in sync with the Rust signatures; add a CI check.

## Build

`maturin develop --release` for local dev, `maturin build --release` for
wheels. `crates/pf-py/pyproject.toml` declares `[tool.maturin]
module-name = "processfork._pf_py"`.
