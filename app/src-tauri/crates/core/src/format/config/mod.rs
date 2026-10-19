//! Config formats: TOML, YAML, INI, dotenv, properties, ignore files. A plugin supplies a [`ConfigSyntax`] (claims,
//! parse, the token a new value is written as); [`ConfigFormat`] turns it into a [`Format`]: the sheet body, the
//! outline, surgical edits (a [`Splice`] of one value token), the secret spans and the keys search may read.

pub mod fidelity;
mod matches;
mod model;
pub mod util;

#[cfg(test)]
mod tests;

pub use matches::{ignore_matches, IgnoreMatches, PatternMatches};
pub use model::{
    ConfigEdit, EditRefusal, Group, NewValue, Part, Quote, Row, RowValue, Scalar, ScalarKind,
    Sheet, SheetKind, Span, Splice,
};

use super::{Body, Claim, Editability, Format, FormatId, RenderCx, Rendered};

/// Texts over this many bytes are shown as text (lock files, generated files).
pub const SHEET_MAX_BYTES: usize = 256 * 1024;
/// A sheet with more rows is shown as text.
pub const SHEET_MAX_ROWS: usize = 3000;
/// Longest keys text handed to search.
const KEYS_LIMIT: usize = 20_000;

/// One config syntax.
pub trait ConfigSyntax: Send + Sync + 'static {
    fn id(&self) -> FormatId;
    fn claims(&self, name: &str, ext: Option<&str>, head: &[u8]) -> Claim;
    /// Language of the text view and of Source.
    fn language(&self) -> &'static str;
    /// The structure of `text`. Never panics; a part it cannot read is a `Raw` row.
    fn parse(&self, text: &str) -> Sheet;
    /// The token that replaces the value token of `old`.
    ///
    /// # Errors
    /// The new value cannot be written for this scalar.
    fn token(&self, old: &Scalar, new: &NewValue) -> Result<String, EditRefusal>;
    /// Edits that are not a value change (ignore files: add | delete a line).
    ///
    /// # Errors
    /// `Unsupported` unless the syntax is line based.
    fn edit_lines(
        &self,
        _text: &str,
        _sheet: &Sheet,
        _edit: &ConfigEdit,
    ) -> Result<Splice, EditRefusal> {
        Err(EditRefusal::Unsupported)
    }
}

/// A [`ConfigSyntax`] as a [`Format`].
#[derive(Debug, Clone, Copy)]
pub struct ConfigFormat<S>(pub S);

impl<S: ConfigSyntax> ConfigFormat<S> {
    fn parsed(&self, text: &str) -> Sheet {
        let mut sheet = self.0.parse(text);
        sheet.finalize();
        sheet
    }

    fn text_body(&self, text: &str, note: &'static str) -> Rendered {
        Rendered {
            body: Body::Text {
                text: text.to_owned(),
                language: Some(self.0.language().to_owned()),
            },
            notes: vec![note],
        }
    }
}

impl<S: ConfigSyntax> Format for ConfigFormat<S> {
    fn id(&self) -> FormatId {
        self.0.id()
    }

    fn claims(&self, name: &str, ext: Option<&str>, head: &[u8]) -> Claim {
        self.0.claims(name, ext, head)
    }

    fn render(&self, text: &str, _cx: &RenderCx<'_>) -> Rendered {
        if text.len() > SHEET_MAX_BYTES {
            return self.text_body(text, "bigConfig");
        }
        let sheet = self.parsed(text);
        if sheet.failed {
            return self.text_body(text, "parseError");
        }
        if sheet.row_count() > SHEET_MAX_ROWS {
            return self.text_body(text, "bigConfig");
        }
        Rendered {
            body: Body::Sheet { sheet },
            notes: Vec::new(),
        }
    }

    fn sheet(&self, text: &str) -> Option<Sheet> {
        let sheet = self.parsed(text);
        (!sheet.failed).then_some(sheet)
    }

    fn edit(&self, text: &str, edit: &ConfigEdit) -> Result<Splice, EditRefusal> {
        let sheet = self.sheet(text).ok_or(EditRefusal::Invalid("syntax"))?;
        let splice = match edit {
            ConfigEdit::Set { path, value } if sheet.kind == SheetKind::Config => {
                let scalar = sheet.scalar(path).ok_or(EditRefusal::NotFound)?;
                if let Editability::ReadOnly { reason } = &scalar.edit {
                    return Err(EditRefusal::ReadOnly(reason.clone()));
                }
                let span = scalar
                    .span
                    .clone()
                    .ok_or_else(|| EditRefusal::ReadOnly("unknown".to_owned()))?;
                let token = self.0.token(scalar, value)?;
                let splice = Splice {
                    start: span.start,
                    end: span.end,
                    insert: token,
                };
                // the file must still read, and the value must read back as typed
                let changed = splice.apply(text);
                let again = self.sheet(&changed).ok_or(EditRefusal::Invalid("syntax"))?;
                match again.scalar(path) {
                    Some(now) if now.text == value.text() => {}
                    _ => return Err(EditRefusal::Invalid("roundTrip")),
                }
                splice
            }
            _ => self.0.edit_lines(text, &sheet, edit)?,
        };
        Ok(splice)
    }

    fn searchable(&self, text: &str) -> Option<String> {
        let sheet = self.sheet(text)?;
        Some(match sheet.kind {
            SheetKind::Config => sheet.keys_text(KEYS_LIMIT),
            SheetKind::Ignore => sheet.patterns_text(KEYS_LIMIT),
        })
    }
}
