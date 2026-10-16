//! How a tag looks: an optional color from a fixed palette and an optional icon from a small set.
//!
//! The names are the ones the UI knows (`src/lib/tagStyle.ts` lists the same; a test compares the two).
//! A style read from `meta.toml` keeps whatever it says, so a value that only a newer version knows
//! survives a rewrite; what this version does not know is not shown.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The palette, in the order the UI offers it.
pub const COLORS: [&str; 8] = [
    "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple",
];

/// The icons a tag can wear: names of the app's own icon set.
pub const ICONS: [&str; 12] = [
    "star",
    "bookmark",
    "flag",
    "lightbulb",
    "check",
    "alert",
    "book-open",
    "code",
    "folder",
    "user",
    "calendar",
    "link",
];

fn known(list: &'static [&'static str], raw: &str) -> Option<&'static str> {
    let raw = raw.trim();
    list.iter().copied().find(|s| s.eq_ignore_ascii_case(raw))
}

/// The palette color a name stands for (any case), or `None` when it is not in the palette.
pub fn known_color(raw: &str) -> Option<&'static str> {
    known(&COLORS, raw)
}

/// The icon a name stands for (any case), or `None` when it is not in the set.
pub fn known_icon(raw: &str) -> Option<&'static str> {
    known(&ICONS, raw)
}

/// What the UI is told about a styled tag. Matches the UI's `TagStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TagStyleView {
    pub color: Option<&'static str>,
    pub icon: Option<&'static str>,
}

/// The `[tag_styles.<tag>]` table of one tag, as written in `meta.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TagStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    /// Keys written by other versions.
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl TagStyle {
    /// Nothing is kept: no color, no icon and no key of another version.
    pub(super) fn is_blank(&self) -> bool {
        self.color.is_none() && self.icon.is_none() && self.extra.is_empty()
    }

    /// A copy with the color and the icon replaced. A name outside the palette or the set means none.
    /// Keys of other versions stay.
    pub(super) fn with(&self, color: Option<&str>, icon: Option<&str>) -> Self {
        Self {
            color: color.and_then(known_color).map(str::to_owned),
            icon: icon.and_then(known_icon).map(str::to_owned),
            extra: self.extra.clone(),
        }
    }

    /// What is shown, or `None` when the style holds nothing this version knows.
    pub fn view(&self) -> Option<TagStyleView> {
        let view = TagStyleView {
            color: self.color.as_deref().and_then(known_color),
            icon: self.icon.as_deref().and_then(known_icon),
        };
        (view.color.is_some() || view.icon.is_some()).then_some(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> TagStyle {
        toml::from_str(text).expect("parse")
    }

    #[test]
    fn the_lists_have_no_repeats_and_use_lowercase_names() {
        for list in [&COLORS[..], &ICONS[..]] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), list.len(), "{list:?}");
            assert!(list.iter().all(|n| *n == n.to_lowercase()), "{list:?}");
        }
    }

    #[test]
    fn names_are_matched_in_any_case_and_unknown_ones_are_not_names() {
        assert_eq!(known_color("Blue"), Some("blue"));
        assert_eq!(known_color(" TEAL "), Some("teal"));
        assert_eq!(known_color("chartreuse"), None);
        assert_eq!(known_color(""), None);
        assert_eq!(known_icon("Book-Open"), Some("book-open"));
        assert_eq!(known_icon("rocket"), None);
        assert_eq!(known_icon("blue"), None);
    }

    #[test]
    fn a_style_shows_what_this_version_knows() {
        let both = parse("color = \"red\"\nicon = \"flag\"\n");
        assert_eq!(
            both.view(),
            Some(TagStyleView {
                color: Some("red"),
                icon: Some("flag")
            })
        );
        let only_icon = parse("color = \"chartreuse\"\nicon = \"star\"\n");
        assert_eq!(
            only_icon.view(),
            Some(TagStyleView {
                color: None,
                icon: Some("star")
            })
        );
        assert_eq!(
            parse("color = \"chartreuse\"\nicon = \"rocket\"\n").view(),
            None
        );
        assert_eq!(parse("").view(), None);
    }

    #[test]
    fn a_style_it_cannot_show_is_still_kept() {
        let style = parse("color = \"chartreuse\"\nweight = 3\n");
        assert!(!style.is_blank());
        let text = toml::to_string(&style).expect("write");
        assert!(
            text.contains("chartreuse") && text.contains("weight = 3"),
            "{text}"
        );
    }

    #[test]
    fn replacing_the_look_keeps_other_versions_keys_and_drops_unknown_names() {
        let style = parse("color = \"red\"\nweight = 3\n");
        let next = style.with(Some("Blue"), Some("rocket"));
        assert_eq!(
            next.view(),
            Some(TagStyleView {
                color: Some("blue"),
                icon: None
            })
        );
        assert!(toml::to_string(&next)
            .expect("write")
            .contains("weight = 3"));
        assert!(style.with(None, None).extra.contains_key("weight"));
        assert!(TagStyle::default().with(None, Some("nope")).is_blank());
    }
}
