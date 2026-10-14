//! Images pasted or dropped into a document: written at once to the `assets` folder next to it, so
//! the Markdown can link to a file that already exists. The UI chooses the name (it knows the local
//! time for `<doc-stem>-YYYYMMDD-HHMMSS.png`); this module makes it safe and unique.

use std::io::{self, Write};

use base64::Engine;
use serde::Serialize;

use super::{EditError, OwnWrites, RESERVED_NAMES};
use crate::util::hash_bytes;
use crate::vault::{Vault, VaultError};

/// The folder, next to the document, that holds its images.
const ASSETS_DIR: &str = "assets";
/// Extensions of the images that can be added: PNG, JPG, GIF, WebP and SVG.
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
/// The largest image written (the UI asks for confirmation well below this).
pub(super) const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
/// The longest base64 text that can hold an image of at most [`MAX_ASSET_BYTES`] (four characters for every
/// three bytes), with room for the line breaks and spaces some encoders add around it.
pub(super) const MAX_ASSET_BASE64: usize = MAX_ASSET_BYTES.div_ceil(3) * 4 + 4;
/// How many `-2`, `-3`... names are tried before giving up.
const MAX_CLASH_SUFFIX: u32 = 999;

/// An image that was written. Matches the UI's `AssetWritten`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetWritten {
    /// The vault path of the image.
    pub path: String,
    /// The path to put in the Markdown: relative to the document's folder (`assets/name.png`).
    pub rel: String,
    pub bytes: usize,
}

/// The bytes of an image sent as base64 text. The length of the text is checked first, so an image far over
/// the limit is refused without being decoded into a second large buffer.
pub fn decode_image(data: &str, name_hint: &str) -> Result<Vec<u8>, EditError> {
    let data = data.trim();
    if data.len() > MAX_ASSET_BASE64 {
        return Err(EditError::TooLarge(name_hint.to_owned()));
    }
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| EditError::InvalidData(format!("The image data is not valid: {e}")))
}

/// A file name that is safe on every system: lowercase letters and digits of any script, single `-`.
fn slug(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "image".to_owned()
    } else {
        let stem: String = trimmed.chars().take(80).collect();
        // Windows opens `nul.png` or `con.png` as the device, not as a file.
        if RESERVED_NAMES.contains(&stem.as_str()) {
            format!("{stem}-image")
        } else {
            stem
        }
    }
}

/// Splits a name hint into a safe stem and a known image extension.
fn clean_name(hint: &str) -> Result<(String, String), EditError> {
    let name = hint.rsplit(['/', '\\']).next().unwrap_or_default();
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let ext = ext.to_ascii_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(EditError::InvalidName(
            "Only images can be added here (PNG, JPG, GIF, WebP, SVG)".to_owned(),
        ));
    }
    Ok((slug(stem), ext))
}

/// The folder part of a vault path (`docs/a.md` is `docs`; a file in the root has none).
fn folder_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(folder, _)| folder)
}

pub(super) fn write(
    vault: &Vault,
    own: &OwnWrites,
    doc_path: &str,
    name_hint: &str,
    data: &[u8],
) -> Result<AssetWritten, EditError> {
    if vault.is_shallow() {
        return Err(EditError::Unsupported(
            "Images can't be added to a single file opened on its own".to_owned(),
        ));
    }
    if data.is_empty() {
        return Err(EditError::InvalidName("The image is empty".to_owned()));
    }
    if data.len() > MAX_ASSET_BYTES {
        return Err(EditError::TooLarge(name_hint.to_owned()));
    }
    let (stem, ext) = clean_name(name_hint)?;
    let doc = vault.resolve(doc_path)?;
    let folder = folder_of(&doc.rel);
    let hash = hash_bytes(data);

    for n in 1..=MAX_CLASH_SUFFIX + 1 {
        let file = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let rel_to_doc = format!("{ASSETS_DIR}/{file}");
        let vault_rel = if folder.is_empty() {
            rel_to_doc.clone()
        } else {
            format!("{folder}/{rel_to_doc}")
        };
        // Checked again for every name: an `assets` folder that is a link out of the vault is refused here.
        let target = vault.resolve(&vault_rel)?;
        if let Some(dir) = target.abs.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EditError::io(&vault_rel, e))?;
        }
        own.expect(&target.rel, hash);
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target.abs);
        match created {
            Ok(mut f) => {
                let written = f.write_all(data).and_then(|()| f.sync_all());
                if let Err(e) = written {
                    drop(f);
                    let _ = std::fs::remove_file(&target.abs);
                    own.retract(&target.rel, hash);
                    return Err(EditError::io(&vault_rel, e));
                }
                return Ok(AssetWritten {
                    path: target.rel,
                    rel: rel_to_doc,
                    bytes: data.len(),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                own.retract(&target.rel, hash);
            }
            Err(e) => {
                own.retract(&target.rel, hash);
                return Err(EditError::io(&vault_rel, e));
            }
        }
    }
    Err(EditError::Vault(VaultError::Invalid(format!(
        "no free name for {name_hint}"
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault_with(files: &[&str]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        for f in files {
            let p = dir.path().join(f);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(&p, "x").expect("write");
        }
        let vault = Vault::open(dir.path()).expect("open");
        (dir, vault)
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-----";

    #[test]
    fn names_are_slugged_and_extensions_checked() {
        assert_eq!(
            clean_name("live-feed-20261014-104201.png").ok(),
            Some(("live-feed-20261014-104201".to_owned(), "png".to_owned()))
        );
        assert_eq!(
            clean_name("C:\\Users\\me\\My Screenshot (3).JPG").ok(),
            Some(("my-screenshot-3".to_owned(), "jpg".to_owned()))
        );
        assert_eq!(
            clean_name("../../etc/Diagrama Ação.webp").ok(),
            Some(("diagrama-ação".to_owned(), "webp".to_owned()))
        );
        assert_eq!(
            clean_name("...png").ok(),
            Some(("image".to_owned(), "png".to_owned()))
        );
        for bad in ["a.exe", "a", "a.png.exe", "a.html", ""] {
            assert!(clean_name(bad).is_err(), "{bad}");
        }
        assert_eq!(slug("A  --  B__C"), "a-b-c");
    }

    #[test]
    fn names_that_are_devices_on_windows_get_a_suffix() {
        for device in ["nul", "CON", "Aux", "prn", "com1", "LPT9"] {
            let hint = format!("{device}.png");
            let (stem, ext) = clean_name(&hint).expect("a name");
            assert_eq!(stem, format!("{}-image", device.to_lowercase()), "{hint}");
            assert_eq!(ext, "png");
        }
        // only the whole stem is reserved
        assert_eq!(slug("console"), "console");
        assert_eq!(slug("com10"), "com10");
        let (dir, v) = vault_with(&["a.md"]);
        let w = write(&v, &OwnWrites::new(), "a.md", "NUL.png", PNG).expect("write");
        assert_eq!(w.rel, "assets/nul-image.png");
        assert!(dir.path().join("assets/nul-image.png").is_file());
    }

    #[test]
    fn an_oversized_text_is_refused_before_it_is_decoded() {
        // Not valid base64: a decoder would fail with "invalidData", so "tooLarge" proves the length came first.
        let junk = "!".repeat(MAX_ASSET_BASE64 + 1);
        let err = decode_image(&junk, "big.png").expect_err("too large");
        assert_eq!(err.kind(), "tooLarge");
        let spaced = format!("  {}  ", "!".repeat(MAX_ASSET_BASE64));
        assert_eq!(
            decode_image(&spaced, "x.png")
                .expect_err("not base64")
                .kind(),
            "invalidData",
            "the limit is on the text without the spaces around it"
        );
    }

    #[test]
    fn the_text_limit_fits_an_image_of_the_byte_limit() {
        // 4 characters per 3 bytes, padded: the longest valid text for MAX_ASSET_BYTES
        assert!(MAX_ASSET_BASE64 >= 4 * MAX_ASSET_BYTES.div_ceil(3));
        // and it is not much more than that
        assert!(MAX_ASSET_BASE64 <= 4 * MAX_ASSET_BYTES.div_ceil(3) + 16);
    }

    #[test]
    fn base64_images_decode_and_bad_text_is_named() {
        assert_eq!(decode_image("AQID\n", "a.png").expect("decode"), [1, 2, 3]);
        assert_eq!(decode_image("", "a.png").expect("empty"), Vec::<u8>::new());
        assert_eq!(
            decode_image("not base64!", "a.png")
                .expect_err("bad")
                .kind(),
            "invalidData"
        );
    }

    #[test]
    fn an_image_is_written_next_to_the_document_with_a_relative_link() {
        let (dir, v) = vault_with(&["docs/specs/live-feed.md"]);
        let own = OwnWrites::new();
        let w = write(&v, &own, "docs/specs/live-feed.md", "shot.png", PNG).expect("write");
        assert_eq!(w.rel, "assets/shot.png");
        assert_eq!(w.path, "docs/specs/assets/shot.png");
        assert_eq!(w.bytes, PNG.len());
        assert_eq!(
            fs::read(dir.path().join("docs/specs/assets/shot.png")).expect("read"),
            PNG
        );
    }

    #[test]
    fn a_name_that_is_taken_gets_a_number() {
        let (dir, v) = vault_with(&["a.md"]);
        let own = OwnWrites::new();
        let first = write(&v, &own, "a.md", "img.png", PNG).expect("first");
        let second = write(&v, &own, "a.md", "img.png", b"other").expect("second");
        let third = write(&v, &own, "a.md", "Img.png", b"third").expect("third");
        assert_eq!(first.rel, "assets/img.png");
        assert_eq!(second.rel, "assets/img-2.png");
        assert_eq!(third.rel, "assets/img-3.png");
        assert_eq!(
            fs::read(dir.path().join("assets/img.png")).expect("read"),
            PNG
        );
        assert_eq!(
            fs::read(dir.path().join("assets/img-2.png")).expect("read"),
            b"other"
        );
    }

    #[test]
    fn bad_input_is_refused_and_nothing_is_left_behind() {
        let (dir, v) = vault_with(&["a.md"]);
        let own = OwnWrites::new();
        for (name, data) in [("a.exe", PNG), ("a.png", b"")] {
            assert!(write(&v, &own, "a.md", name, data).is_err(), "{name}");
        }
        assert!(write(&v, &own, "../x.md", "a.png", PNG).is_err());
        let huge = vec![0_u8; MAX_ASSET_BYTES + 1];
        assert_eq!(
            write(&v, &own, "a.md", "a.png", &huge)
                .expect_err("too large")
                .kind(),
            "tooLarge"
        );
        assert!(!dir.path().join("assets").exists());
    }

    #[test]
    fn a_folder_opened_on_its_own_file_does_not_get_assets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.md");
        fs::write(&file, "x").expect("write");
        let v = Vault::open(&file).expect("open");
        let err = write(&v, &OwnWrites::new(), "a.md", "a.png", PNG).expect_err("simple mode");
        assert_eq!(err.kind(), "unsupported");
    }

    #[test]
    fn an_assets_link_that_leaves_the_vault_is_not_followed() {
        let (dir, v) = vault_with(&["a.md"]);
        let outside = tempfile::tempdir().expect("outside");
        let link = dir.path().join("assets");
        #[cfg(windows)]
        let linked = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
        if !linked {
            eprintln!("skipped: cannot create a directory link here");
            return;
        }
        let result = write(&v, &OwnWrites::new(), "a.md", "a.png", PNG);
        assert_eq!(result.expect_err("escapes").kind(), "pathEscape");
        assert!(fs::read_dir(outside.path()).expect("ls").next().is_none());
    }

    #[test]
    fn documents_in_the_root_and_in_worktree_style_folders_find_their_folder() {
        assert_eq!(folder_of("a.md"), "");
        assert_eq!(folder_of("docs/a.md"), "docs");
        assert_eq!(folder_of("@wt/feat/docs/a.md"), "@wt/feat/docs");
    }
}
