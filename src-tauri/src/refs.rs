//! The images that the Markdown documents of the folder use, kept up to date one document at a time.
//!
//! The Explorer filter shows an image when a document links to it, wherever the image sits. Finding the
//! links is the scanner's job (it knows every inline link, image, reference definition and `href` / `src`
//! of a Markdown text, see [`crate::fileops::scan`]); this module resolves each destination against the
//! document the way the reading view does, keeps the ones that name an image, and remembers them per
//! document, so that saving a document reads that document only and the set the UI asks for is always
//! current.

use std::collections::HashMap;

use crate::fileops::scan::{find_destinations_here, split_suffix, unescape, Dest, Form};
use crate::links::{join_relative, percent_decode, scheme_of};
use crate::util::image_mime;
use crate::vault::key;

/// The vault path that `dest`, found in `text` (the document `doc`), points at, when it is an image.
fn image_target(doc: &str, text: &str, dest: &Dest) -> Option<String> {
    let (path_raw, _) = split_suffix(dest.inner(text));
    let path = unescape(path_raw);
    let path = path.trim();
    if path.is_empty()
        || path.starts_with("//")
        || path.starts_with("\\\\")
        // URLs, `data:` images and drive paths (`C:\...`: a one-letter scheme)
        || scheme_of(path).is_some()
        // an HTML attribute value is not unescaped here: `&amp;` and friends are left alone
        || (dest.form == Form::Html && path.contains('&'))
    {
        return None;
    }
    let decoded = percent_decode(path).replace('\\', "/");
    let target = join_relative(doc, &decoded)?;
    image_mime(&target).map(|_| target)
}

/// The images that the Markdown `text` of the document `doc` (a vault path, `@wt/<name>/...` in a
/// worktree) links to, embeds or defines a reference for, as vault paths: sorted, without duplicates.
/// Paths are resolved relative to the document and are not checked against the disk, so an image that
/// does not exist (yet) is listed too. Runs on the calling thread: call it from a parser thread.
pub(crate) fn images_used(doc: &str, text: &str) -> Vec<String> {
    let mut images: Vec<String> = find_destinations_here(text)
        .iter()
        .filter_map(|dest| image_target(doc, text, dest))
        .collect();
    images.sort();
    images.dedup();
    images
}

/// Which images are used, and by how many documents.
#[derive(Debug, Default)]
pub struct ImageRefs {
    /// What each document uses (documents that use nothing are not kept).
    by_doc: HashMap<String, Vec<String>>,
    /// Image path, with the number of documents using it.
    uses: HashMap<String, usize>,
}

impl ImageRefs {
    /// Replaces what `doc` uses with `images` (sorted, without duplicates, see [`images_used`]).
    /// Returns whether the set of used images changed.
    pub fn set_doc(&mut self, doc: &str, images: Vec<String>) -> bool {
        let old = self.by_doc.remove(&key(doc)).unwrap_or_default();
        let mut changed = false;
        // The new images first: one that stays never drops to zero users on the way.
        for image in &images {
            let count = self.uses.entry(image.clone()).or_insert(0);
            changed |= *count == 0;
            *count += 1;
        }
        for image in &old {
            changed |= self.release(image);
        }
        if !images.is_empty() {
            self.by_doc.insert(key(doc), images);
        }
        changed
    }

    /// Forgets a document. Returns whether the set of used images changed.
    pub fn remove_doc(&mut self, doc: &str) -> bool {
        self.set_doc(doc, Vec::new())
    }

    /// Forgets every document below the folder `dir` (`docs` covers `docs/a.md`, not `docs2/b.md`).
    /// Returns whether the set of used images changed.
    pub fn remove_under(&mut self, dir: &str) -> bool {
        let prefix = format!("{}/", key(dir).trim_end_matches('/'));
        let below: Vec<String> = self
            .by_doc
            .keys()
            .filter(|doc| doc.starts_with(&prefix))
            .cloned()
            .collect();
        let mut changed = false;
        for doc in below {
            for image in self.by_doc.remove(&doc).unwrap_or_default() {
                changed |= self.release(&image);
            }
        }
        changed
    }

    /// One document fewer uses `image`. Returns whether nobody uses it any more.
    fn release(&mut self, image: &str) -> bool {
        let Some(count) = self.uses.get_mut(image) else {
            return false;
        };
        *count -= 1;
        if *count > 0 {
            return false;
        }
        self.uses.remove(image);
        true
    }

    /// Every image some document uses, sorted.
    pub fn images(&self) -> Vec<String> {
        let mut images: Vec<String> = self.uses.keys().cloned().collect();
        images.sort();
        images
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(doc: &str, text: &str) -> Vec<String> {
        images_used(doc, text)
    }

    #[test]
    fn inline_images_resolve_from_the_folder_of_the_document() {
        assert_eq!(
            used("docs/guide.md", "![logo](assets/logo.png)\n"),
            ["docs/assets/logo.png"]
        );
        assert_eq!(
            used("docs/sub/deep.md", "![x](../img/a.png) ![y](./b.jpg)\n"),
            ["docs/img/a.png", "docs/sub/b.jpg"]
        );
        assert_eq!(used("README.md", "![x](docs/a.gif)"), ["docs/a.gif"]);
    }

    #[test]
    fn a_link_into_another_folder_counts_wherever_the_image_sits() {
        assert_eq!(
            used("docs/a.md", "![shot](../site/assets/shot.png)\n"),
            ["site/assets/shot.png"]
        );
        // the image of a link to an image is as much in use as an embedded one
        assert_eq!(
            used("a.md", "[![thumb](t/small.png)](t/big.png)\n"),
            ["t/big.png", "t/small.png"]
        );
        // a root-relative path starts at the root of the folder
        assert_eq!(
            used("docs/deep/a.md", "![x](/assets/logo.svg)"),
            ["assets/logo.svg"]
        );
    }

    #[test]
    fn spaces_encoding_titles_anchors_and_backslashes_are_understood() {
        assert_eq!(
            used("a.md", "![x](my%20pics/a%20b.png \"title\")"),
            ["my pics/a b.png"]
        );
        assert_eq!(used("a.md", "![x](<my pics/a b.png>)"), ["my pics/a b.png"]);
        assert_eq!(
            used("a.md", "![x](img\\sub\\a.png?raw=1#top)"),
            ["img/sub/a.png"]
        );
        assert_eq!(used("a.md", "![x](%C3%A1rea/ima.png)"), ["área/ima.png"]);
    }

    #[test]
    fn reference_style_definitions_count() {
        let text = "See ![the logo][logo] and ![again][].\n\n[logo]: ./assets/logo.png \"Logo\"\n[again]: shot.webp\n[site]: https://example.com/x.png\n";
        assert_eq!(
            used("docs/a.md", text),
            ["docs/assets/logo.png", "docs/shot.webp"]
        );
    }

    #[test]
    fn html_images_count() {
        let text = "<p align=\"center\"><img src=\"../media/pipeline.svg\" width=\"400\"></p>\n\nText with <img src='inline.png' alt=x> inside.\n";
        assert_eq!(
            used("docs/a.md", text),
            ["docs/inline.png", "media/pipeline.svg"]
        );
    }

    #[test]
    fn urls_drive_paths_data_images_and_non_images_are_not_used_images() {
        let text = "![a](https://example.com/a.png) ![b](//cdn.example.com/b.png) ![c](data:image/png;base64,AAAA)\n\n![d](C:\\pics\\d.png) ![e](docs/readme.md) [f](notes.txt) ![g](nothing)\n\n![h](../../../up.png)\n";
        assert!(
            used("docs/a.md", text).is_empty(),
            "{:?}",
            used("docs/a.md", text)
        );
    }

    #[test]
    fn code_and_front_matter_are_not_scanned() {
        let text = "---\nimage: cover.png\n---\n\n```md\n![x](in-code.png)\n```\n\n    ![y](indented.png)\n\nUse `![z](span.png)` like this.\n\n![real](real.png)\n";
        assert_eq!(used("a.md", text), ["real.png"]);
    }

    #[test]
    fn a_worktree_document_keeps_its_prefix() {
        assert_eq!(
            used(
                "@wt/feature/docs/a.md",
                "![x](shots/a.png) ![y](/assets/b.png) ![z](../c.png)"
            ),
            [
                "@wt/feature/assets/b.png",
                "@wt/feature/c.png",
                "@wt/feature/docs/shots/a.png"
            ]
        );
        // a link cannot climb out of the worktree into the main folder
        assert_eq!(
            used("@wt/feature/a.md", "![x](../main.png)"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn duplicates_collapse_and_the_answer_is_sorted() {
        assert_eq!(
            used("a.md", "![](z.png) ![](a.png)\n\n[r]: z.png\n![](./z.png)"),
            ["a.png", "z.png"]
        );
    }

    #[test]
    fn nothing_to_find_in_a_document_without_images() {
        assert_eq!(used("a.md", ""), Vec::<String>::new());
        assert_eq!(
            used("a.md", "# Title\n\nJust [a link](b.md) and text.\n"),
            Vec::<String>::new()
        );
    }

    fn refs(docs: &[(&str, &[&str])]) -> ImageRefs {
        let mut refs = ImageRefs::default();
        for (doc, images) in docs {
            refs.set_doc(doc, images.iter().map(|s| (*s).to_owned()).collect());
        }
        refs
    }

    fn owned(images: &[&str]) -> Vec<String> {
        images.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_set_is_the_union_of_what_the_documents_use() {
        let r = refs(&[("a.md", &["x.png", "y.png"]), ("b.md", &["y.png", "z.png"])]);
        assert_eq!(r.images(), ["x.png", "y.png", "z.png"]);
    }

    #[test]
    fn an_image_stays_while_any_document_still_uses_it() {
        let mut r = refs(&[("a.md", &["x.png", "y.png"]), ("b.md", &["y.png"])]);
        // a.md drops y.png but b.md still uses it; x.png was only in a.md
        assert!(
            r.set_doc("a.md", owned(&["z.png"])),
            "x.png left, z.png came"
        );
        assert_eq!(r.images(), ["y.png", "z.png"]);
        assert!(r.remove_doc("b.md"), "y.png left");
        assert_eq!(r.images(), ["z.png"]);
    }

    #[test]
    fn reports_a_change_only_when_the_set_changes() {
        let mut r = ImageRefs::default();
        assert!(r.set_doc("a.md", owned(&["x.png"])));
        assert!(
            !r.set_doc("a.md", owned(&["x.png"])),
            "saved again, same images"
        );
        assert!(
            !r.set_doc("b.md", owned(&["x.png"])),
            "a second user of x.png"
        );
        assert!(!r.remove_doc("a.md"), "b.md still uses it");
        assert!(r.remove_doc("b.md"));
        assert!(!r.remove_doc("b.md"), "already gone");
        assert!(!r.remove_doc("never-seen.md"));
        assert!(!r.set_doc("c.md", Vec::new()), "a document with no images");
        assert_eq!(r.images(), Vec::<String>::new());
    }

    #[test]
    fn a_renamed_document_moves_its_images() {
        let mut r = refs(&[("old.md", &["x.png"])]);
        // the indexer forgets the old path and reads the new one: its relative link now points elsewhere
        assert!(r.remove_doc("old.md"));
        assert!(r.set_doc("sub/new.md", owned(&["sub/x.png"])));
        assert_eq!(r.images(), ["sub/x.png"]);
    }

    #[test]
    fn removing_a_folder_forgets_its_documents_but_not_their_namesakes() {
        let mut r = refs(&[
            ("docs/a.md", &["a.png"]),
            ("docs/deep/b.md", &["b.png"]),
            ("docs2/c.md", &["c.png"]),
            ("docs.md", &["d.png"]),
        ]);
        assert!(r.remove_under("docs"));
        assert_eq!(r.images(), ["c.png", "d.png"]);
        assert!(!r.remove_under("docs/"), "nothing left below");
    }

    #[test]
    fn removing_a_worktree_forgets_its_documents() {
        let mut r = refs(&[
            ("@wt/f/a.md", &["@wt/f/x.png"]),
            ("@wt/f2/a.md", &["@wt/f2/x.png"]),
            ("a.md", &["x.png"]),
        ]);
        assert!(r.remove_under("@wt/f"));
        assert_eq!(r.images(), ["@wt/f2/x.png", "x.png"]);
    }

    #[cfg(windows)]
    #[test]
    fn documents_are_found_whatever_the_case_of_their_path_on_windows() {
        let mut r = refs(&[("Docs/A.md", &["x.png"])]);
        assert!(r.remove_doc("docs/a.md"));
        assert_eq!(r.images(), Vec::<String>::new());
    }
}
