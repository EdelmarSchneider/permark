//! What the data looks like (one flat table? how deep? how much of it sits in uniform arrays?) and where its secret
//! values are.

use permark_core::secrets::{has_token, is_count, is_secret_key, url_password};
use serde_json::{Map, Value};

/// The shape of a value, for judging targets: facts measured once, not states.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Shape {
    /// One uniform table: a non-empty array of objects with the same keys and primitive values.
    pub table: bool,
    /// The root is an array.
    pub array: bool,
    /// Text mixed with elements (XML): no flat target.
    pub mixed: bool,
    /// Share of the primitive values that sit in uniform arrays of objects (TOON tables).
    pub tabular_share: f64,
    /// Deepest nesting of objects and arrays.
    pub depth: usize,
    /// Some array of objects has objects with different keys.
    pub non_uniform: bool,
}

#[must_use]
pub fn shape(value: &Value) -> Shape {
    let mut walk = Walk::default();
    walk.visit(value, 0, false);
    #[allow(clippy::cast_precision_loss)] // a share, not a count
    let tabular_share = if walk.leaves == 0 {
        0.0
    } else {
        walk.tabular as f64 / walk.leaves as f64
    };
    Shape {
        table: crate::write::rows(value).is_some(),
        array: value.is_array(),
        mixed: walk.mixed,
        tabular_share,
        depth: walk.depth,
        non_uniform: walk.non_uniform,
    }
}

#[derive(Default)]
struct Walk {
    leaves: usize,
    tabular: usize,
    depth: usize,
    non_uniform: bool,
    mixed: bool,
}

impl Walk {
    fn visit(&mut self, value: &Value, depth: usize, in_table: bool) {
        self.depth = self.depth.max(depth);
        match value {
            Value::Array(items) => {
                let uniform = uniform_objects(items);
                if !uniform && items.iter().filter(|i| i.is_object()).count() > 1 {
                    self.non_uniform = true;
                }
                for item in items {
                    self.visit(item, depth + 1, in_table || uniform);
                }
            }
            Value::Object(map) => {
                if map.contains_key("#text") && map.values().any(|v| v.is_object() || v.is_array())
                {
                    self.mixed = true;
                }
                for v in map.values() {
                    self.visit(v, depth + 1, in_table);
                }
            }
            _ => {
                self.leaves += 1;
                if in_table {
                    self.tabular += 1;
                }
            }
        }
    }
}

/// Every item is a non-empty object, all with the same keys, all values primitive.
fn uniform_objects(items: &[Value]) -> bool {
    let Some(Value::Object(first)) = items.first() else {
        return false;
    };
    !first.is_empty()
        && items.iter().all(|item| match item {
            Value::Object(map) => {
                map.len() == first.len()
                    && first.keys().all(|k| map.contains_key(k))
                    && map.values().all(|v| !v.is_object() && !v.is_array())
            }
            _ => false,
        })
}

/// `${VAR}`, `$VAR`, `{{ x }}`: a pointer to a secret, not the secret.
fn is_reference(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("${") && t.ends_with('}'))
        || (t.starts_with("{{") && t.ends_with("}}"))
        || t.strip_prefix('$')
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

fn is_secret_value(value: &Value, under_secret: bool) -> bool {
    match value {
        Value::String(s) => {
            (under_secret && !s.is_empty() && !is_reference(s))
                || url_password(s).is_some()
                || has_token(s)
        }
        Value::Number(_) => under_secret,
        _ => false,
    }
}

/// Values under a secret key (and addresses with a password), counted.
#[must_use]
pub fn count_secrets(value: &Value) -> usize {
    fn walk(value: &Value, secret: bool) -> usize {
        match value {
            Value::Array(items) => items.iter().map(|v| walk(v, secret)).sum(),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| match v {
                    // a number under a count key (`tokens: 1200`) is a size
                    Value::Number(n) if is_count(k, &n.to_string()) => 0,
                    _ => walk(v, secret || is_secret_key(k)),
                })
                .sum(),
            other => usize::from(is_secret_value(other, secret)),
        }
    }
    walk(value, false)
}

/// The value with every secret value replaced by a mask (for previews).
#[must_use]
pub fn mask_secrets(value: &Value) -> Value {
    fn walk(value: &Value, secret: bool) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|v| walk(v, secret)).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| match v {
                        Value::Number(n) if is_count(k, &n.to_string()) => (k.clone(), v.clone()),
                        _ => (k.clone(), walk(v, secret || is_secret_key(k))),
                    })
                    .collect::<Map<String, Value>>(),
            ),
            other if is_secret_value(other, secret) => Value::String("••••••".to_owned()),
            other => other.clone(),
        }
    }
    walk(value, false)
}
