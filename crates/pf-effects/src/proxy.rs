// SPDX-License-Identifier: MIT
//! `ToolProxy` — wraps a runtime's tool dispatch so every call passes through
//! the effect ledger.
//!
//! Adapter authors register their tools by name once, then invoke through the
//! proxy. The proxy hashes args, mints an idempotency key, runs the tool,
//! hashes the result, appends to the ledger, returns the result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde_json::Value;

use crate::ledger::{Ledger, SideEffectClass, args_hash, mint_idempotency_key};

/// A dispatchable tool. Synchronous for the v1 proxy; the SDKs wrap this in
/// `tokio::task::spawn_blocking` for async use.
pub trait ToolHandler: Send + Sync {
    /// The tool's declared side-effect class. Misclassification is a contract
    /// violation by the tool author, not a ledger bug.
    fn side_effect_class(&self) -> SideEffectClass;
    /// Invoke the tool. Args and result are arbitrary JSON.
    fn call(&self, args: &Value) -> pf_core::Result<Value>;
}

/// Holds tool registrations + the ledger + the secret. `Send + Sync` via
/// inner `Mutex`; cheap to `clone()`.
#[derive(Clone)]
pub struct ToolProxy {
    inner: Arc<ToolProxyInner>,
}

struct ToolProxyInner {
    tools: Mutex<HashMap<String, Arc<dyn ToolHandler>>>,
    ledger: Mutex<Ledger>,
}

impl ToolProxy {
    /// Construct a proxy backed by the given (probably empty) ledger.
    #[must_use]
    pub fn new(ledger: Ledger) -> Self {
        Self {
            inner: Arc::new(ToolProxyInner {
                tools: Mutex::new(HashMap::new()),
                ledger: Mutex::new(ledger),
            }),
        }
    }

    /// Register a tool under `id`. Subsequent registrations under the same
    /// `id` overwrite (last wins).
    pub fn register(&self, id: impl Into<String>, handler: Arc<dyn ToolHandler>) {
        self.inner.tools.lock().unwrap().insert(id.into(), handler);
    }

    /// Invoke a registered tool. The call is recorded in the ledger before
    /// being returned to the caller.
    pub fn invoke(&self, id: &str, args: &Value) -> pf_core::Result<Value> {
        let handler = {
            let tools = self.inner.tools.lock().unwrap();
            tools
                .get(id)
                .cloned()
                .ok_or_else(|| pf_core::Error::Integrity(format!("unregistered tool: {id}")))?
        };
        let arg_h = args_hash(args)?;
        let key = mint_idempotency_key()?;
        let result = handler.call(args)?;
        let result_h = args_hash(&result)?;
        self.inner.ledger.lock().unwrap().append(
            Utc::now(),
            id,
            arg_h,
            key,
            result_h,
            handler.side_effect_class(),
        )?;
        Ok(result)
    }

    /// Snapshot the current ledger (clones it so the caller can serialize
    /// without holding the lock).
    pub fn ledger_snapshot(&self) -> Ledger {
        self.inner.ledger.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::SessionSecret;
    use serde_json::json;

    struct AddTool;
    impl ToolHandler for AddTool {
        fn side_effect_class(&self) -> SideEffectClass {
            SideEffectClass::Pure
        }
        fn call(&self, args: &Value) -> pf_core::Result<Value> {
            let a = args.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = args.get("b").and_then(Value::as_i64).unwrap_or(0);
            Ok(json!({"sum": a + b}))
        }
    }

    struct EmailTool;
    impl ToolHandler for EmailTool {
        fn side_effect_class(&self) -> SideEffectClass {
            SideEffectClass::Irreversible
        }
        fn call(&self, _args: &Value) -> pf_core::Result<Value> {
            Ok(json!({"sent": true}))
        }
    }

    #[test]
    fn invoke_records_in_ledger() {
        let ledger = Ledger::new(SessionSecret::new(b"t".to_vec()));
        let proxy = ToolProxy::new(ledger);
        proxy.register("add", Arc::new(AddTool));
        let r = proxy.invoke("add", &json!({"a": 2, "b": 40})).unwrap();
        assert_eq!(r, json!({"sum": 42}));
        let snap = proxy.ledger_snapshot();
        assert_eq!(snap.entries().len(), 1);
        assert_eq!(snap.entries()[0].tool_id, "add");
        assert_eq!(snap.entries()[0].side_effect_class, SideEffectClass::Pure);
        snap.verify().unwrap();
    }

    #[test]
    fn unknown_tool_errors_cleanly() {
        let proxy = ToolProxy::new(Ledger::new(SessionSecret::new(b"t".to_vec())));
        let err = proxy.invoke("missing", &json!({})).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }

    #[test]
    fn mixed_classes_all_chain_correctly() {
        let proxy = ToolProxy::new(Ledger::new(SessionSecret::new(b"t".to_vec())));
        proxy.register("add", Arc::new(AddTool));
        proxy.register("email", Arc::new(EmailTool));
        for _ in 0..10 {
            proxy.invoke("add", &json!({"a": 1, "b": 1})).unwrap();
            proxy.invoke("email", &json!({"to": "x@y"})).unwrap();
        }
        let snap = proxy.ledger_snapshot();
        assert_eq!(snap.entries().len(), 20);
        snap.verify().unwrap();
    }
}
