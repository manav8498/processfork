# Your first agent fork

A 5-minute walkthrough. Assumes the `pf` binary is on your PATH (see
[Install](./install.md)).

## 1. Snapshot a sandbox

```bash
mkdir -p /tmp/agent-sandbox/src
echo "fn main() { println!(\"hello\"); }" > /tmp/agent-sandbox/src/main.rs

pf --store ~/.processfork snapshot \
  --agent-id demo \
  --fs-root  /tmp/agent-sandbox
# → sha256:1c2497b0dc23d21b8068b26f54c0d8b14b7fdf704c11a456dca7e36eaf6fbed6
```

## 2. Inspect the manifest

```bash
pf --store ~/.processfork log
pf --store ~/.processfork status
```

`pf log` prints the snapshot DAG newest-first; `pf status` prints the
store size.

## 3. Fork into 4 divergent branches

```bash
pf --store ~/.processfork fork sha256:1c24… -n 4 --explore "try alternatives"
# → 4 new CIDs printed, one per line
```

Each fork is a manifest-level branch — its `parents = [<source>]`. The
underlying layer blobs are shared via CAS dedup; storage cost is the
new manifest only (~600 B per fork).

## 4. Mutate one branch

```bash
echo "fn main() { println!(\"hello, world\"); }" > /tmp/agent-sandbox/src/main.rs
pf --store ~/.processfork snapshot --agent-id demo --fs-root /tmp/agent-sandbox \
  --name updated
# → sha256:7e670a428c073dec837b879dcf78ec2f1eb54c2c1949b25457f7300515a7aaa5
```

## 5. Diff the two

```bash
pf --store ~/.processfork diff sha256:1c24… sha256:7e67…
# - world.fs = sha256:231945ab…
# + world.fs = sha256:de9a5a4a…
# (every other layer identical)
```

## 6. Restore on a different path

```bash
pf --store ~/.processfork checkout sha256:7e67… --into /tmp/restored
cat /tmp/restored/src/main.rs
# fn main() { println!("hello, world"); }
```

## 7. Push to a registry

```bash
mkdir -p /tmp/my-registry
pf --store ~/.processfork push sha256:7e67… file:///tmp/my-registry
# ✓ pushed sha256:7e67… → file:///tmp/my-registry
```

## 8. Pull on a fresh store

```bash
pf --store /tmp/store-on-other-host pull file:///tmp/my-registry
# sha256:7e670a428c073dec837b879dcf78ec2f1eb54c2c1949b25457f7300515a7aaa5
```

The CID round-trips identical — content-addressed all the way down.

That's it. Real integrations live one level up, in
[Integrations](./integrations/README.md).
