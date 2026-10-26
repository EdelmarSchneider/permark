//! What the data looks like (one flat table? how deep? how much of it sits in uniform arrays?).

use serde_json::Value;

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
