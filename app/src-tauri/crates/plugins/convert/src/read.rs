//! Sources -> the data model (a JSON value, key order kept), with the words of what the reading lost. Every reader
//! checks the cancel flag as it goes; none follows a reference outside the text.

use std::collections::HashMap;

use quick_xml::events::Event as XmlEvent;
use quick_xml::XmlVersion;
use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{Event as YamlEvent, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};
use yaml_rust2::Yaml;

use crate::{Failure, Losses, Source, Tick};

/// The data of `text` read as `source`.
///
/// # Errors
/// The text is not valid for the source (with the line when the reader knows it), or the work was cancelled.
pub fn read(
    source: Source,
    text: &str,
    tick: &mut Tick<'_>,
    losses: &mut Losses,
) -> Result<Value, Failure> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match source {
        Source::Json => json(text),
        Source::JsonLines => json_lines(text, tick),
        Source::Yaml => yaml(text, tick, losses),
        Source::Xml => xml(text, tick, losses),
        Source::Csv => csv(text, b',', tick, losses),
        Source::Tsv => csv(text, b'\t', tick, losses),
        Source::Toon => toon(text, losses),
        Source::Toml => toml(text, tick, losses),
    }
}

fn invalid(line: usize, detail: impl Into<String>) -> Failure {
    Failure::Invalid {
        line: u32::try_from(line).unwrap_or(0),
        detail: detail.into(),
    }
}

fn json(text: &str) -> Result<Value, Failure> {
    serde_json::from_str(text).map_err(|e| invalid(e.line(), e.to_string()))
}

fn json_lines(text: &str, tick: &mut Tick<'_>) -> Result<Value, Failure> {
    let mut items = Vec::new();
    for (n, line) in text.lines().enumerate() {
        tick.tick()?;
        if line.trim().is_empty() {
            continue;
        }
        items.push(serde_json::from_str(line).map_err(|e| invalid(n + 1, e.to_string()))?);
    }
    Ok(Value::Array(items))
}

fn toon(text: &str, losses: &mut Losses) -> Result<Value, Failure> {
    let value =
        permark_plugin_fmt_toon::decode(text).map_err(|p| invalid(p.line as usize, p.detail))?;
    if text
        .lines()
        .any(|l| l.trim_start_matches(' ').starts_with('#'))
    {
        losses.add("comments");
    }
    Ok(value)
}

// ---- TOML ---------------------------------------------------------------------------------------------------------

fn toml(text: &str, tick: &mut Tick<'_>, losses: &mut Losses) -> Result<Value, Failure> {
    let doc = toml_edit::Document::parse(text).map_err(|e| {
        let line = e.span().map_or(0, |s| {
            text[..s.start.min(text.len())].matches('\n').count() + 1
        });
        invalid(line, e.message())
    })?;
    if text.lines().any(|l| l.contains('#')) {
        losses.add("comments");
    }
    toml_table(doc.as_table(), tick, losses)
}

fn toml_table(
    table: &toml_edit::Table,
    tick: &mut Tick<'_>,
    losses: &mut Losses,
) -> Result<Value, Failure> {
    let mut map = Map::new();
    for (key, item) in table {
        tick.tick()?;
        let value = match item {
            toml_edit::Item::None => continue,
            toml_edit::Item::Value(v) => toml_value(v, tick, losses)?,
            toml_edit::Item::Table(t) => toml_table(t, tick, losses)?,
            toml_edit::Item::ArrayOfTables(a) => Value::Array(
                a.iter()
                    .map(|t| toml_table(t, tick, losses))
                    .collect::<Result<_, _>>()?,
            ),
        };
        map.insert(key.to_owned(), value);
    }
    Ok(Value::Object(map))
}

fn toml_value(
    value: &toml_edit::Value,
    tick: &mut Tick<'_>,
    losses: &mut Losses,
) -> Result<Value, Failure> {
    tick.tick()?;
    Ok(match value {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::from(*i.value()),
        toml_edit::Value::Float(f) => number(*f.value(), losses),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => {
            losses.add("dates");
            Value::String(d.value().to_string())
        }
        toml_edit::Value::Array(a) => Value::Array(
            a.iter()
                .map(|v| toml_value(v, tick, losses))
                .collect::<Result<_, _>>()?,
        ),
        toml_edit::Value::InlineTable(t) => {
            let mut map = Map::new();
            for (k, v) in t {
                map.insert(k.to_owned(), toml_value(v, tick, losses)?);
            }
            Value::Object(map)
        }
    })
}

/// A float as a JSON number; NaN and infinities have none (null).
fn number(f: f64, losses: &mut Losses) -> Value {
    Number::from_f64(f).map_or_else(
        || {
            losses.add("notNumbers");
            Value::Null
        },
        Value::Number,
    )
}

// ---- CSV | TSV ----------------------------------------------------------------------------------------------------

fn csv(text: &str, delim: u8, tick: &mut Tick<'_>, losses: &mut Losses) -> Result<Value, Failure> {
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut names: Vec<String> = Vec::new();
    {
        let headers = reader
            .headers()
            .map_err(|e| invalid(csv_line(&e), e.to_string()))?;
        for h in headers {
            // a repeated column name gets a number
            let mut name = h.to_owned();
            let mut n = 2;
            while names.contains(&name) {
                losses.add("duplicateColumns");
                name = format!("{h}_{n}");
                n += 1;
            }
            names.push(name);
        }
    }
    let mut rows = Vec::new();
    let mut guessed = false;
    for record in reader.records() {
        tick.tick()?;
        let record = record.map_err(|e| invalid(csv_line(&e), e.to_string()))?;
        if record.len() != names.len() {
            losses.add("uneven");
        }
        let mut map = Map::new();
        for (i, name) in names.iter().enumerate() {
            let cell = record.get(i).unwrap_or("");
            let value = typed_cell(cell);
            guessed |= !value.is_string();
            map.insert(name.clone(), value);
        }
        rows.push(Value::Object(map));
    }
    if guessed {
        losses.add("types");
    }
    Ok(Value::Array(rows))
}

fn csv_line(e: &::csv::Error) -> usize {
    e.position()
        .map_or(0, |p| usize::try_from(p.line()).unwrap_or(0))
}

/// A cell as a number (a JSON number that writes back as the same text), `true | false`, else text.
fn typed_cell(cell: &str) -> Value {
    match cell {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if permark_plugin_fmt_toon_number(cell) {
        if let Ok(Value::Number(n)) = serde_json::from_str::<Value>(cell) {
            if n.to_string() == cell {
                return Value::Number(n);
            }
        }
    }
    Value::String(cell.to_owned())
}

/// The JSON number grammar (no leading zeros, no `+`, no bare dot).
fn permark_plugin_fmt_toon_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    match b.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return false,
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == b.len()
}

// ---- XML ----------------------------------------------------------------------------------------------------------

struct Element {
    name: String,
    map: Map<String, Value>,
    text: String,
    children: bool,
}

/// Elements -> keys; repeated names -> arrays; attributes -> `@name`; text -> `#text` (an element with text only ->
/// a string); comments, processing instructions, the DOCTYPE and whitespace between elements are dropped.
fn xml(text: &str, tick: &mut Tick<'_>, losses: &mut Losses) -> Result<Value, Failure> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<(String, Value)> = None;
    let line_at = |pos: u64| {
        let pos = usize::try_from(pos).unwrap_or(usize::MAX).min(text.len());
        text[..pos].matches('\n').count() + 1
    };
    loop {
        tick.tick()?;
        let event = reader
            .read_event()
            .map_err(|e| invalid(line_at(reader.error_position()), e.to_string()))?;
        match event {
            XmlEvent::Start(_) | XmlEvent::Empty(_) if root.is_some() && stack.is_empty() => {
                return Err(invalid(
                    line_at(reader.buffer_position()),
                    "more than one root element",
                ));
            }
            XmlEvent::Start(e) => stack.push(element(&e, losses)?),
            XmlEvent::Empty(e) => {
                let el = element(&e, losses)?;
                close(el, &mut stack, &mut root, losses);
            }
            XmlEvent::End(_) => {
                if let Some(el) = stack.pop() {
                    close(el, &mut stack, &mut root, losses);
                }
            }
            XmlEvent::Text(t) => {
                let content = t.xml10_content();
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&content);
                } else if !content.trim().is_empty() {
                    return Err(invalid(
                        line_at(reader.buffer_position()),
                        "text outside the root element",
                    ));
                }
            }
            XmlEvent::CData(c) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&c.into_inner());
                }
            }
            XmlEvent::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match r.into_inner().as_ref() {
                        "lt" => "<".to_owned(),
                        "gt" => ">".to_owned(),
                        "amp" => "&".to_owned(),
                        "apos" => "'".to_owned(),
                        "quot" => "\"".to_owned(),
                        other => {
                            losses.add("entities");
                            format!("&{other};")
                        }
                    },
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolved);
                }
            }
            XmlEvent::Comment(_) => losses.add("comments"),
            XmlEvent::PI(_) => losses.add("instructions"),
            XmlEvent::DocType(_) => losses.add("doctype"),
            XmlEvent::Decl(_) => {}
            XmlEvent::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(invalid(
            line_at(reader.buffer_position()),
            "an element is not closed",
        ));
    }
    let (name, value) = root.ok_or_else(|| invalid(0, "no root element"))?;
    let mut map = Map::new();
    map.insert(name, value);
    Ok(Value::Object(map))
}

fn element(e: &quick_xml::events::BytesStart<'_>, losses: &mut Losses) -> Result<Element, Failure> {
    let name = e.name().as_ref().to_owned();
    if name.contains(':') {
        losses.add("namespaces");
    }
    let mut map = Map::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| invalid(0, err.to_string()))?;
        let key = attr.key.as_ref().to_owned();
        if key == "xmlns" || key.starts_with("xmlns:") || key.contains(':') {
            losses.add("namespaces");
        }
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| invalid(0, err.to_string()))?;
        map.insert(format!("@{key}"), Value::String(value.into_owned()));
    }
    Ok(Element {
        name,
        map,
        text: String::new(),
        children: false,
    })
}

fn close(
    el: Element,
    stack: &mut [Element],
    root: &mut Option<(String, Value)>,
    losses: &mut Losses,
) {
    let Element {
        name,
        mut map,
        text,
        children,
    } = el;
    let trimmed = text.trim();
    if trimmed.len() != text.len() {
        losses.add("whitespace");
    }
    let value = if map.is_empty() && !children {
        Value::String(trimmed.to_owned())
    } else {
        if !trimmed.is_empty() {
            if children {
                losses.add("mixed");
            }
            map.insert("#text".to_owned(), Value::String(trimmed.to_owned()));
        }
        Value::Object(map)
    };
    let Some(parent) = stack.last_mut() else {
        *root = Some((name, value));
        return;
    };
    parent.children = true;
    // a repeated name becomes a list
    match parent.map.get_mut(&name) {
        // values of elements are strings and objects: an array here is a list of repeated elements
        Some(Value::Array(items)) => items.push(value),
        Some(existing) => {
            let first = std::mem::take(existing);
            *existing = Value::Array(vec![first, value]);
        }
        None => {
            parent.map.insert(name, value);
        }
    }
}

// ---- YAML ---------------------------------------------------------------------------------------------------------

enum Frame {
    Seq(Vec<Value>, usize),
    Map(Map<String, Value>, Option<String>, usize),
}

/// Builds values from parser events: aliases are copied (within a budget, so a small file cannot expand without
/// bound), tags are dropped, keys that are not strings are written as text.
struct YamlBuilder<'t, 'c> {
    tick: &'t mut Tick<'c>,
    losses: &'t mut Losses,
    stack: Vec<Frame>,
    docs: Vec<Value>,
    anchors: HashMap<usize, (Value, usize)>,
    budget: usize,
    failed: Option<Failure>,
}

fn size(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(size).sum::<usize>(),
        Value::Object(map) => 1 + map.values().map(size).sum::<usize>(),
        _ => 1,
    }
}

impl YamlBuilder<'_, '_> {
    fn insert(&mut self, value: Value, anchor: usize) {
        if anchor > 0 {
            let n = size(&value);
            self.anchors.insert(anchor, (value.clone(), n));
        }
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some(Frame::Seq(items, _)) => items.push(value),
            Some(Frame::Map(map, key, _)) => match key.take() {
                None => {
                    let text = match value {
                        Value::String(s) => s,
                        other => {
                            self.losses.add("keys");
                            other.to_string()
                        }
                    };
                    *key = Some(text);
                }
                Some(k) => {
                    map.insert(k, value);
                }
            },
        }
    }

    fn scalar(&mut self, v: String, style: TScalarStyle, tag: Option<&Tag>) -> Value {
        if tag.is_some_and(|t| t.handle != "tag:yaml.org,2002:") {
            self.losses.add("tags");
        }
        if style != TScalarStyle::Plain || tag.is_some_and(|t| t.suffix == "str") {
            return Value::String(v);
        }
        match Yaml::from_str(&v) {
            Yaml::Integer(i) => Value::from(i),
            Yaml::Real(r) => r
                .parse::<f64>()
                .map_or(Value::String(v), |f| number(f, self.losses)),
            Yaml::Boolean(b) => Value::Bool(b),
            Yaml::Null => Value::Null,
            _ => Value::String(v),
        }
    }
}

impl MarkedEventReceiver for YamlBuilder<'_, '_> {
    fn on_event(&mut self, ev: YamlEvent, _mark: Marker) {
        if self.failed.is_some() {
            return;
        }
        if let Err(stop) = self.tick.tick() {
            self.failed = Some(stop);
            return;
        }
        match ev {
            YamlEvent::Nothing
            | YamlEvent::StreamStart
            | YamlEvent::StreamEnd
            | YamlEvent::DocumentStart => {}
            YamlEvent::DocumentEnd => {
                if self.stack.is_empty() && self.docs.is_empty() {
                    self.docs.push(Value::Null);
                }
            }
            YamlEvent::Alias(id) => {
                self.losses.add("anchors");
                let Some((value, n)) = self.anchors.get(&id).cloned() else {
                    self.insert(Value::Null, 0);
                    return;
                };
                if n > self.budget {
                    self.failed = Some(invalid(0, "aliases expand too far"));
                    return;
                }
                self.budget -= n;
                self.insert(value, 0);
            }
            YamlEvent::Scalar(v, style, anchor, tag) => {
                if anchor > 0 {
                    self.losses.add("anchors");
                }
                let value = self.scalar(v, style, tag.as_ref());
                self.insert(value, anchor);
            }
            YamlEvent::SequenceStart(anchor, tag) => {
                if tag.is_some() {
                    self.losses.add("tags");
                }
                self.stack.push(Frame::Seq(Vec::new(), anchor));
            }
            YamlEvent::MappingStart(anchor, tag) => {
                if tag.is_some() {
                    self.losses.add("tags");
                }
                self.stack.push(Frame::Map(Map::new(), None, anchor));
            }
            YamlEvent::SequenceEnd | YamlEvent::MappingEnd => match self.stack.pop() {
                Some(Frame::Seq(items, anchor)) => self.insert(Value::Array(items), anchor),
                Some(Frame::Map(map, _, anchor)) => self.insert(Value::Object(map), anchor),
                None => {}
            },
        }
    }
}

fn yaml(text: &str, tick: &mut Tick<'_>, losses: &mut Losses) -> Result<Value, Failure> {
    if text
        .lines()
        .any(|l| l.trim_start().starts_with('#') || l.contains(" #"))
    {
        losses.add("comments");
    }
    let mut builder = YamlBuilder {
        tick,
        losses,
        stack: Vec::new(),
        docs: Vec::new(),
        anchors: HashMap::new(),
        budget: 1_000_000 + text.len() / 4,
        failed: None,
    };
    let mut parser = Parser::new_from_str(text);
    parser
        .load(&mut builder, true)
        .map_err(|e| invalid(e.marker().line(), e.info().to_owned()))?;
    if let Some(stop) = builder.failed {
        return Err(stop);
    }
    let mut docs = builder.docs;
    Ok(match docs.len() {
        0 => Value::Null,
        1 => docs.pop().unwrap_or(Value::Null),
        _ => {
            builder.losses.add("documents");
            Value::Array(docs)
        }
    })
}
