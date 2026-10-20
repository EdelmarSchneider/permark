//! TOML front matter (`+++`) for the card: tables flattened to `parent.child`, arrays as lists, datetimes as text.

use permark_core::format::{Field, FieldValue};
use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::clip;

/// Deepest table nesting followed.
const MAX_DEPTH: usize = 16;

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.value().clone(),
        other => other.to_string().trim().to_owned(),
    }
}

fn walk(table: &dyn TableLike, prefix: &str, depth: usize, out: &mut Vec<Field>) {
    for (key, item) in table.iter() {
        let full = if prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{prefix}.{key}")
        };
        match item {
            Item::Value(Value::Array(array)) => out.push(Field {
                key: full,
                value: FieldValue::List {
                    items: array.iter().map(|v| clip(&scalar(v))).collect(),
                },
            }),
            Item::Value(Value::InlineTable(inline)) if depth < MAX_DEPTH => {
                walk(inline, &full, depth + 1, out);
            }
            Item::Value(value) => out.push(Field {
                key: full,
                value: FieldValue::Text {
                    text: clip(&scalar(value)),
                },
            }),
            Item::Table(nested) if depth < MAX_DEPTH => walk(nested, &full, depth + 1, out),
            Item::ArrayOfTables(tables) => out.push(Field {
                key: full,
                value: FieldValue::List {
                    items: tables
                        .iter()
                        .map(|t| clip(&t.to_string().trim().replace('\n', "; ")))
                        .collect(),
                },
            }),
            Item::Table(_) | Item::None => {}
        }
    }
}

/// Fields of a TOML text; text that is not valid TOML gives no fields (the card then shows the raw text).
pub(crate) fn parse(raw: &str) -> Vec<Field> {
    let Ok(doc) = raw.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(doc.as_table(), "", 0, &mut out);
    out
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)] // `assert!(x.is_empty())` reads best for nothing found
mod tests {
    use super::*;

    fn t(text: &str) -> FieldValue {
        FieldValue::Text {
            text: text.to_owned(),
        }
    }

    #[test]
    fn keys_tables_arrays_and_dates() {
        let got: Vec<_> = parse(
            "title = \"A plan\"\ntags = [\"a\", \"b\"]\ndraft = true\ndate = 2026-10-19\nn = 3\n[author]\nname = \"Ana\"\n[author.team]\nid = 7\n",
        )
        .into_iter()
        .map(|f| (f.key, f.value))
        .collect();
        assert_eq!(
            got,
            [
                ("title".to_owned(), t("A plan")),
                (
                    "tags".to_owned(),
                    FieldValue::List {
                        items: vec!["a".into(), "b".into()]
                    }
                ),
                ("draft".to_owned(), t("true")),
                ("date".to_owned(), t("2026-10-19")),
                ("n".to_owned(), t("3")),
                ("author.name".to_owned(), t("Ana")),
                ("author.team.id".to_owned(), t("7")),
            ]
        );
    }

    #[test]
    fn invalid_toml_gives_no_fields() {
        for bad in ["a = ", "= 1", "[x", "a = 1\na = 2", "\u{0}"] {
            assert!(parse(bad).is_empty(), "{bad}");
        }
        assert!(parse("").is_empty());
    }

    #[test]
    fn inline_tables_and_arrays_of_tables() {
        let fields =
            parse("point = { x = 1, y = 2 }\n[[items]]\nname = \"a\"\n[[items]]\nname = \"b\"\n");
        assert_eq!(fields[0].key, "point.x");
        assert_eq!(fields[1].key, "point.y");
        assert!(matches!(&fields[2].value, FieldValue::List { items } if items.len() == 2));
    }
}
