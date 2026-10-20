//! Plugin RPC: named handlers a running plugin registers; one app command calls them with JSON. The edge stays data
//! only (a sandboxed plugin form stays possible). A handler exists while its plugin runs (RAII registration).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock, Weak};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PluginId;
use crate::error::{ErrorKind, IpcError};

/// One call: the window it came from and its arguments (`null` when the caller sent none).
#[derive(Debug, Clone)]
pub struct RpcCall {
    pub window: String,
    pub args: Value,
}

pub type RpcHandler = Arc<dyn Fn(&RpcCall) -> Result<Value, IpcError> + Send + Sync>;

/// Arguments of a method that takes none.
#[derive(Debug, Default, Deserialize)]
pub struct NoArgs {}

/// Wraps a typed function: arguments in from JSON (`null` = `{}`), result out to JSON.
pub fn typed<A, R>(
    f: impl Fn(A, &RpcCall) -> Result<R, IpcError> + Send + Sync + 'static,
) -> RpcHandler
where
    A: DeserializeOwned,
    R: Serialize,
{
    Arc::new(move |call| {
        let args = if call.args.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            call.args.clone()
        };
        let args: A = serde_json::from_value(args)
            .map_err(|e| IpcError::new(ErrorKind::InvalidData).detail(e.to_string()))?;
        let result = f(args, call)?;
        serde_json::to_value(result)
            .map_err(|e| IpcError::new(ErrorKind::Internal).detail(e.to_string()))
    })
}

type Key = (PluginId, String);

#[derive(Default)]
pub struct RpcTable {
    handlers: RwLock<BTreeMap<Key, (u64, RpcHandler)>>,
    next: AtomicU64,
}

impl std::fmt::Debug for RpcTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcTable").finish_non_exhaustive()
    }
}

impl RpcTable {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers `plugin.method` until the returned registration is dropped. A newer registration of the same name
    /// replaces this one (the older one's drop leaves it alone).
    #[must_use]
    pub fn register(
        self: &Arc<Self>,
        plugin: &PluginId,
        method: &str,
        handler: RpcHandler,
    ) -> RpcRegistration {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let key = (plugin.clone(), method.to_owned());
        self.handlers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), (id, handler));
        RpcRegistration {
            table: Arc::downgrade(self),
            key,
            id,
        }
    }

    /// Calls `plugin.method`.
    ///
    /// # Errors
    /// `Unavailable` (subject `plugin.method`): nobody serves it (plugin off | not started); the handler's own error.
    pub fn call(&self, plugin: &str, method: &str, call: &RpcCall) -> Result<Value, IpcError> {
        let handler = {
            let map = self.handlers.read().unwrap_or_else(PoisonError::into_inner);
            PluginId::parse(plugin)
                .and_then(|id| map.get(&(id, method.to_owned())))
                .map(|(_, handler)| Arc::clone(handler))
        };
        match handler {
            Some(handler) => handler(call),
            None => {
                Err(IpcError::new(ErrorKind::Unavailable).subject(format!("{plugin}.{method}")))
            }
        }
    }

    /// Names served now (`plugin.method`), sorted.
    #[must_use]
    pub fn served(&self) -> Vec<String> {
        self.handlers
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .map(|(plugin, method)| format!("{plugin}.{method}"))
            .collect()
    }
}

/// Teardown symmetry: the handler goes with this.
#[derive(Debug)]
pub struct RpcRegistration {
    table: Weak<RpcTable>,
    key: Key,
    id: u64,
}

impl Drop for RpcRegistration {
    fn drop(&mut self) {
        if let Some(table) = self.table.upgrade() {
            let mut map = table
                .handlers
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if map.get(&self.key).is_some_and(|(id, _)| *id == self.id) {
                map.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> PluginId {
        PluginId::parse(text).unwrap()
    }

    fn call(args: Value) -> RpcCall {
        RpcCall {
            window: "main".to_owned(),
            args,
        }
    }

    #[derive(Deserialize)]
    struct Add {
        a: i64,
        b: i64,
    }

    #[test]
    fn a_registered_method_answers_and_goes_with_its_registration() {
        let table = RpcTable::new();
        let registration = table.register(
            &id("demo"),
            "add",
            typed(|args: Add, _| Ok(args.a + args.b)),
        );
        let out = table.call("demo", "add", &call(serde_json::json!({"a": 2, "b": 3})));
        assert_eq!(out.unwrap(), serde_json::json!(5));
        assert_eq!(table.served(), ["demo.add"]);
        drop(registration);
        let gone = table.call("demo", "add", &call(Value::Null)).unwrap_err();
        assert_eq!(gone.kind, ErrorKind::Unavailable);
        assert_eq!(table.served(), Vec::<String>::new());
    }

    #[test]
    fn bad_arguments_are_invalid_data_and_null_means_an_empty_object() {
        let table = RpcTable::new();
        let _keep = table.register(&id("demo"), "add", typed(|a: Add, _| Ok(a.a + a.b)));
        let _none = table.register(&id("demo"), "ping", typed(|_: NoArgs, _| Ok("pong")));
        let bad = table
            .call("demo", "add", &call(serde_json::json!({"a": "x"})))
            .unwrap_err();
        assert_eq!(bad.kind, ErrorKind::InvalidData);
        assert_eq!(
            table.call("demo", "ping", &call(Value::Null)).unwrap(),
            serde_json::json!("pong")
        );
    }

    #[test]
    fn a_newer_registration_survives_the_drop_of_the_older_one() {
        let table = RpcTable::new();
        let old = table.register(&id("demo"), "m", typed(|_: NoArgs, _| Ok(1)));
        let _new = table.register(&id("demo"), "m", typed(|_: NoArgs, _| Ok(2)));
        drop(old);
        assert_eq!(
            table.call("demo", "m", &call(Value::Null)).unwrap(),
            serde_json::json!(2)
        );
    }

    #[test]
    fn unknown_plugin_names_and_bad_ids_are_unavailable() {
        let table = RpcTable::new();
        for plugin in ["nobody", "Not Valid", ""] {
            let err = table.call(plugin, "m", &call(Value::Null)).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Unavailable, "{plugin}");
        }
    }
}
