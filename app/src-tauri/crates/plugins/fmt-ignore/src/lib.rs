//! Plugin `fmt-ignore`: ignore files (`.gitignore`, `.aiignore`, ...) as a list of patterns. Edits are whole lines:
//! change a pattern, delete a line, add a line; no other line moves.

use std::sync::Arc;

use permark_core::format::config::util::{majority_break, Lines};
use permark_core::format::config::{Group, Row, RowValue, Scalar, Sheet, SheetKind};
use permark_core::format::{
    Claim, ConfigEdit, ConfigFormat, ConfigSyntax, Contributions, EditRefusal, FormatId, NewValue,
    Splice,
};
use permark_core::plugins::{Plugin, PluginKind, StaticPlugin};

#[must_use]
pub fn plugin() -> Arc<dyn Plugin> {
    StaticPlugin::app(
        "fmt-ignore",
        PluginKind::Format,
        Contributions::default().formats(vec![Arc::new(ConfigFormat(Ignore))]),
    )
}

/// The ignore-file syntax.
#[derive(Debug, Clone, Copy)]
pub struct Ignore;

fn line_path(n: usize) -> String {
    format!("L{}", n + 1)
}

fn line_of(path: &str) -> Option<usize> {
    path.strip_prefix('L')?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)
}

impl ConfigSyntax for Ignore {
    fn id(&self) -> FormatId {
        FormatId::IGNORE
    }

    fn claims(&self, name: &str, _ext: Option<&str>, _head: &[u8]) -> Claim {
        let lower = name.to_ascii_lowercase();
        if lower.starts_with('.') && lower.ends_with("ignore") && lower.len() > ".ignore".len() - 1
        {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn language(&self) -> &'static str {
        "ignore"
    }

    fn parse(&self, text: &str) -> Sheet {
        let lines = Lines::new(text);
        let mut groups: Vec<Group> = vec![Group {
            path: String::new(),
            title: String::new(),
            line: 1,
            end: 1,
            comment: None,
            rows: Vec::new(),
        }];
        // a block of comment lines opens a new group titled with its text
        let mut block: Vec<String> = Vec::new();
        let mut block_line = 0;
        let mut previous_comment = false;
        for (n, (_, raw)) in lines.iter().enumerate() {
            let line = raw.trim_end();
            let line_no = u32::try_from(n + 1).unwrap_or(u32::MAX);
            if line.is_empty() {
                previous_comment = false;
                continue;
            }
            if let Some(comment) = line.trim_start().strip_prefix('#') {
                if !previous_comment {
                    block.clear();
                    block_line = line_no;
                }
                previous_comment = true;
                let text = comment.trim();
                if !text.is_empty() {
                    block.push(text.to_owned());
                }
                continue;
            }
            previous_comment = false;
            if !block.is_empty() {
                groups.push(Group {
                    path: line_path(usize::try_from(block_line.saturating_sub(1)).unwrap_or(0)),
                    title: block.join(" "),
                    line: block_line,
                    end: line_no,
                    comment: None,
                    rows: Vec::new(),
                });
                block.clear();
            }
            let path = line_path(n);
            if let Some(group) = groups.last_mut() {
                group.end = group.end.max(line_no);
                group.rows.push(Row {
                    path: path.clone(),
                    key: String::new(),
                    line: line_no,
                    end: line_no,
                    value: RowValue::Pattern {
                        path,
                        text: line.to_owned(),
                        negated: line.starts_with('!'),
                    },
                    comment: None,
                });
            }
        }
        groups.retain(|g| !g.rows.is_empty());
        Sheet {
            kind: SheetKind::Ignore,
            groups,
            failed: false,
        }
    }

    fn token(&self, _old: &Scalar, _new: &NewValue) -> Result<String, EditRefusal> {
        Err(EditRefusal::Unsupported)
    }

    fn edit_lines(
        &self,
        text: &str,
        sheet: &Sheet,
        edit: &ConfigEdit,
    ) -> Result<Splice, EditRefusal> {
        let lines = Lines::new(text);
        let eol = majority_break(text);
        match edit {
            ConfigEdit::Set { path, value } => {
                let n = target(sheet, path, &lines)?;
                let new = value.text();
                if new.contains(['\n', '\r']) {
                    return Err(EditRefusal::Invalid("newline"));
                }
                if new.trim().is_empty() {
                    return Err(EditRefusal::Invalid("empty"));
                }
                let (start, end, _) = lines.bounds(n).ok_or(EditRefusal::NotFound)?;
                Ok(Splice {
                    start,
                    end,
                    insert: new.to_owned(),
                })
            }
            ConfigEdit::Remove { path } => {
                let n = target(sheet, path, &lines)?;
                let (start, end, next) = lines.bounds(n).ok_or(EditRefusal::NotFound)?;
                if next == end && start > 0 {
                    // the last line has no break of its own: take the break before it instead
                    let before = &text[..start];
                    let cut = if before.ends_with("\r\n") {
                        start - 2
                    } else {
                        start - 1
                    };
                    Ok(Splice {
                        start: cut,
                        end,
                        insert: String::new(),
                    })
                } else {
                    Ok(Splice {
                        start,
                        end: next,
                        insert: String::new(),
                    })
                }
            }
            ConfigEdit::Insert { after, text: new } => {
                if new.contains(['\n', '\r']) {
                    return Err(EditRefusal::Invalid("newline"));
                }
                if new.trim().is_empty() {
                    return Err(EditRefusal::Invalid("empty"));
                }
                let n = match after {
                    Some(path) => target(sheet, path, &lines)?,
                    None => lines.count().saturating_sub(1),
                };
                if text.is_empty() {
                    return Ok(Splice {
                        start: 0,
                        end: 0,
                        insert: format!("{new}{eol}"),
                    });
                }
                let (_, end, next) = lines.bounds(n).ok_or(EditRefusal::NotFound)?;
                if next == end {
                    // after a last line that has no break: add one before the new line
                    Ok(Splice {
                        start: end,
                        end,
                        insert: format!("{eol}{new}"),
                    })
                } else {
                    Ok(Splice {
                        start: next,
                        end: next,
                        insert: format!("{new}{eol}"),
                    })
                }
            }
        }
    }
}

/// The 0-based line of the row `path`, when the row is there and a pattern.
fn target(sheet: &Sheet, path: &str, lines: &Lines<'_>) -> Result<usize, EditRefusal> {
    let row = sheet.row(path).ok_or(EditRefusal::NotFound)?;
    if !matches!(row.value, RowValue::Pattern { .. }) {
        return Err(EditRefusal::NotFound);
    }
    let n = line_of(path).ok_or(EditRefusal::NotFound)?;
    if n < lines.count() {
        Ok(n)
    } else {
        Err(EditRefusal::NotFound)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn manifest_is_a_valid_app_plugin() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "fmt-ignore");
        assert_eq!(manifest.kind, PluginKind::Format);
        assert!(manifest.default_enabled);
        assert!(manifest.depends_on.is_empty());
    }
}
