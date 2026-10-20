//! Analyses: measures of a text (word count first; token counts, spelling later). Plugins contribute them; the
//! registry runs the enabled ones.

use serde::Serialize;
use ts_rs::TS;

/// What the text is: Markdown is measured without its syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TextKind {
    Markdown,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MeasureUnit {
    Words,
    Characters,
    Minutes,
    Tokens,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Measure {
    /// `<analysis>.<measure>` (`word-count.words`).
    pub id: String,
    #[ts(type = "number")]
    pub value: u64,
    pub unit: MeasureUnit,
}

/// One analysis.
pub trait Analysis: Send + Sync {
    fn id(&self) -> &'static str;
    fn measure(&self, text: &str, kind: TextKind) -> Vec<Measure>;
}
