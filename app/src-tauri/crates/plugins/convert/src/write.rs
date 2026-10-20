//! The data model -> targets: TOON (pinned spec), JSON (compact | pretty), JSON Lines, CSV | TSV, YAML (block).

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::{Failure, Losses, Target, Tick};

/// `value` written as `target`. The caller checked that the target is available for the data.
///
/// # Errors
/// The writer failed (TOON cannot write a value), or the work was cancelled.
pub fn write(
    target: Target,
    value: &Value,
    tick: &mut Tick<'_>,
    losses: &mut Losses,
) -> Result<String, Failure> {
    tick.tick()?;
    let fail = |detail: String| Failure::Invalid { line: 0, detail };
    match target {
        Target::Toon => permark_plugin_fmt_toon::encode(value).map_err(fail),
        Target::Json => serde_json::to_string(value).map_err(|e| fail(e.to_string())),
        Target::JsonPretty => serde_json::to_string_pretty(value).map_err(|e| fail(e.to_string())),
        Target::JsonLines => {
            let Value::Array(items) = value else {
                return Err(fail("not an array".to_owned()));
            };
            let mut out = String::new();
            for item in items {
                tick.tick()?;
                out.push_str(&serde_json::to_string(item).map_err(|e| fail(e.to_string()))?);
                out.push('\n');
            }
            Ok(out)
        }
        Target::Csv => table(value, b',', tick, losses),
        Target::Tsv => table(value, b'\t', tick, losses),
        Target::Yaml => {
            let mut out = String::new();
            yaml(value, 0, &mut out, tick)?;
            Ok(out)
        }
    }
}

/// The rows of a uniform table (an array of objects with the same keys and primitive values), else `None`.
#[must_use]
pub fn rows(value: &Value) -> Option<(Vec<&String>, &[Value])> {
    let Value::Array(items) = value else {
        return None;
    };
    let Value::Object(first) = items.first()? else {
        return None;
    };
    if first.is_empty() {
        return None;
    }
    let keys: Vec<&String> = first.keys().collect();
    let uniform = items.iter().all(|item| match item {
        Value::Object(map) => {
            map.len() == keys.len() && keys.iter().all(|k| map.get(*k).is_some_and(is_primitive))
        }
        _ => false,
    });
    uniform.then_some((keys, items.as_slice()))
}

fn is_primitive(value: &Value) -> bool {
    !matches!(value, Value::Array(_) | Value::Object(_))
}

fn table(
    value: &Value,
    delim: u8,
    tick: &mut Tick<'_>,
    losses: &mut Losses,
) -> Result<String, Failure> {
    let fail = |detail: String| Failure::Invalid { line: 0, detail };
    let (keys, items) = rows(value).ok_or_else(|| fail("not one table".to_owned()))?;
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delim)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(keys.iter().map(|k| k.as_bytes()))
        .map_err(|e| fail(e.to_string()))?;
    for item in items {
        tick.tick()?;
        let Value::Object(map) = item else { continue };
        let cells: Vec<String> = keys
            .iter()
            .map(|k| match map.get(*k) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => {
                    losses.add("nulls");
                    String::new()
                }
                Some(other) => other.to_string(),
            })
            .collect();
        writer
            .write_record(&cells)
            .map_err(|e| fail(e.to_string()))?;
    }
    let bytes = writer.into_inner().map_err(|e| fail(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| fail(e.to_string()))
}

// ---- YAML (block style) ---------------------------------------------------------------------------------------------

/// Whether a string can be written plain in YAML (1.1 and 1.2 readers read it back as the same string).
fn yaml_plain(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let first = s.chars().next();
    !(s.is_empty()
        || matches!(
            lower.as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n" | "null" | "~"
        )
        || first.is_some_and(|c| "-?:,[]{}#&*!|>'\"%@`.+ \t".contains(c) || c.is_ascii_digit())
        || s.ends_with([' ', '\t', ':'])
        || s.contains(": ")
        || s.contains(" #")
        || s.contains(['\t', '\n', '\r'])
        || s.chars().any(|c| {
            c.is_control() || c == '\u{feff}' || c == '\u{2028}' || c == '\u{2029}' || c == '\u{85}'
        }))
}

fn yaml_string(s: &str) -> String {
    if yaml_plain(s) {
        s.to_owned()
    } else {
        // JSON string escapes are YAML double-quoted escapes
        serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
    }
}

fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => yaml_string(s),
        Value::Array(_) => "[]".to_owned(),
        Value::Object(_) => "{}".to_owned(),
    }
}

fn is_empty_container(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => true,
    }
}

/// `value` as block YAML lines, each starting with `indent` spaces.
fn yaml(
    value: &Value,
    indent: usize,
    out: &mut String,
    tick: &mut Tick<'_>,
) -> Result<(), Failure> {
    tick.tick()?;
    let pad = " ".repeat(indent);
    match value {
        Value::Object(map) if !map.is_empty() => yaml_map(map, indent, out, tick)?,
        Value::Array(items) if !items.is_empty() => {
            for item in items {
                if is_empty_container(item) {
                    let _ = writeln!(out, "{pad}- {}", yaml_scalar(item));
                    continue;
                }
                // the item at two more spaces, its first line opened by the hyphen
                let mut inner = String::new();
                yaml(item, indent + 2, &mut inner, tick)?;
                out.push_str(&pad);
                out.push_str("- ");
                out.push_str(&inner[indent + 2..]);
            }
        }
        other => {
            out.push_str(&pad);
            out.push_str(&yaml_scalar(other));
            out.push('\n');
        }
    }
    Ok(())
}

fn yaml_map(
    map: &Map<String, Value>,
    indent: usize,
    out: &mut String,
    tick: &mut Tick<'_>,
) -> Result<(), Failure> {
    let pad = " ".repeat(indent);
    for (key, value) in map {
        let key = yaml_string(key);
        if is_empty_container(value) {
            let _ = writeln!(out, "{pad}{key}: {}", yaml_scalar(value));
        } else {
            let _ = writeln!(out, "{pad}{key}:");
            yaml(value, indent + 2, out, tick)?;
        }
    }
    Ok(())
}
