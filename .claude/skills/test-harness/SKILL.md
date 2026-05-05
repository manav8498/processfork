---
name: test-harness
description: How to write integration tests against a real local Llama-3-8B (or skip cleanly when GPU absent).
---

# Test-harness patterns

## Three test tiers

1. **Unit** — `crates/<crate>/src/**/tests`: pure logic, no FS, no network.
2. **Crate-integration** — `crates/<crate>/tests/`: hits real FS via
   `tempfile`, can invoke other crates, no external services.
3. **End-to-end** — `tests/e2e/` and `examples/<N>/`: runs the actual `pf`
   binary, may require GPU/network. Gated.

## GPU gate

For tests that require a CUDA host:

```rust
#[test]
fn vllm_round_trip() {
    if std::env::var("PF_HAS_GPU").as_deref() != Ok("1") {
        eprintln!("skipping: needs PF_HAS_GPU=1 + vLLM ≥0.10");
        return; // intentional skip, NOT a pass
    }
    // … real test …
}
```

Document required env vars in the test's doc-comment.

## Llama-3-8B local

For Python adapter tests we boot vLLM as a subprocess:

```python
import subprocess, time, requests, pytest

@pytest.fixture(scope="session")
def vllm_8b():
    if not os.environ.get("PF_HAS_GPU"):
        pytest.skip("needs PF_HAS_GPU=1")
    proc = subprocess.Popen(
        ["vllm", "serve", "meta-llama/Llama-3-8B",
         "--enforce-deterministic", "--port", "18001"],
    )
    for _ in range(120):
        try:
            requests.get("http://localhost:18001/health", timeout=1)
            break
        except requests.RequestException:
            time.sleep(1)
    else:
        proc.kill()
        pytest.fail("vllm did not start in 120 s")
    yield "http://localhost:18001"
    proc.kill()
```

## Synthetic fixture

For host-portable performance tests we use a synthetic 4-layer fixture in
`crates/pf-core/tests/fixtures/synthetic_4layer.rs` that builds a 1.2 GB
manifest with realistic-shaped blobs. Used by Phase-1 acceptance.

## Determinism

Pin every randomness source: `proptest!` uses an explicit seed, `rand::SeedableRng::seed_from_u64(PF_TEST_SEED)`, summarizer calls in merge tests stub with a fixture model.
