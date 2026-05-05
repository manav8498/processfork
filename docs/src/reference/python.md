# Python SDK

Install:

```bash
pip install processfork
```

## Quick reference

```python
import processfork as pf

store = pf.PfStore.open("~/.processfork")

cid = pf.snapshot_filesystem(
    store,
    agent_kind="claude-code",
    fs_root="/tmp/sandbox",
    env={"PWD": "/tmp/sandbox"},
    messages=[{"role": "user", "content": "go"}],
)

manifest = pf.read_manifest(store, cid)
pf.checkout_filesystem(store, cid, "/tmp/restored")

report = pf.merge(store, a, b)
print(report["overall"])      # 'clean' | 'conflicted'
```

## Type stubs

Hand-written stubs live in
`crates/pf-py/python/processfork/_pf_py.pyi`. `mypy --strict` callers
get full hints.

## Source

[`crates/pf-py/`](https://github.com/manav8498/processfork/tree/main/crates/pf-py).
