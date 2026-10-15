//! Adds, removes and reads the "Open with TrailMark" entries in the Windows registry (current user only).
//!
//! [`Registry`] works on three roots so a test can point it at a throwaway subtree instead of the real
//! `Software\Classes`: where the entries live, where the app keeps the person's choices (read by the installer,
//! so an update respects them), and where Windows records the default app of an extension.

use std::io;
use std::path::Path;

use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

use super::entries::{
    self, Key, ASSOCIATION_CHOICE, EXTENSIONS, MENU_CHOICE, MENU_TARGETS, PROG_ID,
};
use super::{notify, ShellState};
use crate::session::Language;

const CLASSES: &str = r"Software\Classes";
/// The app's own key; the installer keeps the language here too.
const APP_KEY: &str = r"Software\TrailMark";
const FILE_EXTS: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

pub(crate) struct Registry {
    classes: String,
    app_key: String,
    file_exts: String,
    /// Whether to tell the shell after a change. Off for the throwaway registries of the tests.
    notify: bool,
}

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

/// `Ok` when the thing to delete was not there in the first place.
fn missing_is_fine(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl Registry {
    /// The real registry of the current user.
    pub(crate) fn current_user() -> Self {
        Self {
            classes: CLASSES.to_owned(),
            app_key: APP_KEY.to_owned(),
            file_exts: FILE_EXTS.to_owned(),
            notify: true,
        }
    }

    /// A registry rooted at `root` (a key under `HKEY_CURRENT_USER`), for tests.
    #[cfg(test)]
    pub(crate) fn under(root: &str) -> Self {
        Self {
            classes: format!(r"{root}\Classes"),
            app_key: format!(r"{root}\App"),
            file_exts: format!(r"{root}\FileExts"),
            notify: false,
        }
    }

    fn class_path(&self, key: &str) -> String {
        format!(r"{}\{key}", self.classes)
    }

    fn write(&self, key: &Key) -> io::Result<()> {
        let (handle, _) = hkcu().create_subkey(self.class_path(&key.path))?;
        for value in &key.values {
            handle.set_value(&value.name, &value.data)?;
        }
        Ok(())
    }

    /// Deletes a key and everything under it. Only ever called with keys that belong to TrailMark.
    fn delete_tree(&self, key: &str) -> io::Result<()> {
        missing_is_fine(hkcu().delete_subkey_all(self.class_path(key)))
    }

    /// Deletes one value of a key, leaving the key and its other values alone.
    fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
        let handle = match hkcu().open_subkey_with_flags(self.class_path(key), KEY_SET_VALUE) {
            Ok(handle) => handle,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        missing_is_fine(handle.delete_value(name))
    }

    fn exists(&self, key: &str) -> bool {
        hkcu().open_subkey(self.class_path(key)).is_ok()
    }

    fn has_value(&self, key: &str, name: &str) -> bool {
        hkcu()
            .open_subkey(self.class_path(key))
            .and_then(|handle| handle.get_raw_value(name))
            .is_ok()
    }

    /// Remembers the choice for the installer: an update then neither re-adds what was removed nor removes what
    /// was kept.
    fn record(&self, choice: &str, enabled: bool) -> io::Result<()> {
        let (handle, _) = hkcu().create_subkey(&self.app_key)?;
        handle.set_value(choice, &u32::from(enabled))
    }

    fn changed(&self) {
        if self.notify {
            notify::associations_changed();
        }
    }

    /// What is in the registry now.
    pub(crate) fn state(&self) -> ShellState {
        ShellState {
            supported: true,
            context_menu: MENU_TARGETS
                .iter()
                .any(|target| self.exists(&format!(r"{}\command", target.key()))),
            association: self.exists(&format!(r"{PROG_ID}\shell\open\command"))
                || EXTENSIONS
                    .iter()
                    .any(|ext| self.has_value(&entries::open_with_key(ext), PROG_ID)),
            is_default: self.default_for(".md").as_deref() == Some(PROG_ID),
        }
    }

    /// The `ProgID` Windows has recorded as the person's choice for an extension. It is read-only for programs
    /// (Windows protects it with a hash). Newer Windows 11 builds keep it in `UserChoiceLatest\ProgId` (a key named
    /// `ProgId`) and may have no `UserChoice` at all; older ones only have `UserChoice`.
    fn default_for(&self, extension: &str) -> Option<String> {
        let base = format!(r"{}\{extension}", self.file_exts);
        let progid_in = |key: String| {
            hkcu()
                .open_subkey(key)
                .ok()?
                .get_value::<String, _>("ProgId")
                .ok()
        };
        progid_in(format!(r"{base}\UserChoiceLatest\ProgId"))
            .or_else(|| progid_in(format!(r"{base}\UserChoice")))
    }

    /// Adds or removes the context-menu entries.
    pub(crate) fn set_context_menu(
        &self,
        enabled: bool,
        exe: &Path,
        language: Language,
    ) -> io::Result<()> {
        if enabled {
            for key in entries::menu_keys(exe, entries::menu_label(language)) {
                self.write(&key)?;
            }
        } else {
            for target in &MENU_TARGETS {
                self.delete_tree(&target.key())?;
            }
        }
        self.record(MENU_CHOICE, enabled)?;
        self.changed();
        Ok(())
    }

    /// Adds or removes the `ProgID` and its place in the "Open with" lists. The default app is never touched.
    pub(crate) fn set_association(
        &self,
        enabled: bool,
        exe: &Path,
        language: Language,
    ) -> io::Result<()> {
        if enabled {
            for key in entries::association_keys(exe, entries::type_name(language)) {
                self.write(&key)?;
            }
            for key in entries::open_with_keys() {
                self.write(&key)?;
            }
        } else {
            for ext in EXTENSIONS {
                self.delete_value(&entries::open_with_key(ext), PROG_ID)?;
            }
            self.delete_tree(PROG_ID)?;
        }
        self.record(ASSOCIATION_CHOICE, enabled)?;
        self.changed();
        Ok(())
    }

    /// Words the entries that exist in `language` (after the person switched the interface language). Entries
    /// that are not there are not created, and the commands are left as they are.
    pub(crate) fn refresh_labels(&self, language: Language) -> io::Result<()> {
        let mut touched = false;
        for target in &MENU_TARGETS {
            touched |= self.relabel(&target.key(), entries::menu_label(language))?;
        }
        touched |= self.relabel(PROG_ID, entries::type_name(language))?;
        if touched {
            self.changed();
        }
        Ok(())
    }

    /// Sets the default value of a key, when the key exists. Says whether it did.
    fn relabel(&self, key: &str, label: &str) -> io::Result<bool> {
        match hkcu().open_subkey_with_flags(self.class_path(key), KEY_SET_VALUE) {
            Ok(handle) => {
                handle.set_value("", &label)?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A throwaway registry under `HKCU\Software\TrailMarkTest<pid>_<n>`, deleted when it goes out of scope (even
    /// if the test fails). The real `Software\Classes` is never touched.
    struct Scratch {
        root: String,
        registry: Registry,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let root = format!(
                r"Software\TrailMarkTest{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let registry = Registry::under(&root);
            Self { root, registry }
        }

        fn key(path: &str) -> Option<RegKey> {
            hkcu().open_subkey(path).ok()
        }

        fn class(&self, path: &str) -> Option<RegKey> {
            Self::key(&self.registry.class_path(path))
        }

        /// The default value of a key under `Classes`.
        fn default_of(&self, path: &str) -> Option<String> {
            self.class(path)?.get_value::<String, _>("").ok()
        }

        fn app_choice(&self, name: &str) -> Option<u32> {
            Self::key(&self.registry.app_key)?
                .get_value::<u32, _>(name)
                .ok()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            assert!(self.root.starts_with(r"Software\TrailMarkTest"));
            let _ = hkcu().delete_subkey_all(&self.root);
        }
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Trail Mark\trailmark.exe")
    }

    const PROGRAM: &str = r#""C:\Program Files\Trail Mark\trailmark.exe""#;

    #[test]
    fn an_empty_registry_has_nothing_enabled() {
        let s = Scratch::new();
        assert_eq!(
            s.registry.state(),
            ShellState {
                supported: true,
                context_menu: false,
                association: false,
                is_default: false
            }
        );
    }

    #[test]
    fn the_menu_entries_are_written_where_the_shell_looks_for_them() {
        let s = Scratch::new();
        s.registry
            .set_context_menu(true, &exe(), Language::De)
            .expect("enable");
        assert!(s.registry.state().context_menu);
        for (class, placeholder) in [
            ("Directory", "%1"),
            (r"Directory\Background", "%V"),
            (r"SystemFileAssociations\.md", "%1"),
            (r"SystemFileAssociations\.markdown", "%1"),
        ] {
            let verb = format!(r"{class}\shell\TrailMark");
            assert_eq!(
                s.default_of(&verb).as_deref(),
                Some("Mit TrailMark öffnen"),
                "{verb}"
            );
            let icon = s
                .class(&verb)
                .and_then(|k| k.get_value::<String, _>("Icon").ok());
            assert_eq!(icon, Some(format!("{PROGRAM},0")), "{verb}");
            assert_eq!(
                s.default_of(&format!(r"{verb}\command")),
                Some(format!("{PROGRAM} \"{placeholder}\"")),
                "{verb}"
            );
        }
        assert_eq!(s.app_choice(MENU_CHOICE), Some(1));
        // the association is a separate choice
        assert!(!s.registry.state().association);
        assert!(s.class("TrailMark.Markdown").is_none());
    }

    #[test]
    fn removing_the_menu_removes_only_the_menu_entries() {
        let s = Scratch::new();
        // verbs and values of other programs, in the same keys
        for other in [
            r"Directory\shell\Other\command",
            r"Directory\Background\shell\Other\command",
            r"SystemFileAssociations\.md\shell\Other\command",
        ] {
            hkcu()
                .create_subkey(s.registry.class_path(other))
                .expect("other verb");
        }
        s.registry
            .set_context_menu(true, &exe(), Language::En)
            .expect("enable");
        s.registry
            .set_context_menu(false, &exe(), Language::En)
            .expect("disable");
        assert!(!s.registry.state().context_menu);
        for class in [
            "Directory",
            r"Directory\Background",
            r"SystemFileAssociations\.md",
            r"SystemFileAssociations\.markdown",
        ] {
            assert!(s.class(&format!(r"{class}\shell\TrailMark")).is_none());
        }
        assert!(s.class(r"Directory\shell\Other\command").is_some());
        assert!(s
            .class(r"Directory\Background\shell\Other\command")
            .is_some());
        assert!(s
            .class(r"SystemFileAssociations\.md\shell\Other\command")
            .is_some());
        assert_eq!(s.app_choice(MENU_CHOICE), Some(0));
    }

    #[test]
    fn turning_things_on_twice_or_off_twice_is_fine() {
        let s = Scratch::new();
        s.registry
            .set_context_menu(false, &exe(), Language::En)
            .expect("off when there is nothing");
        s.registry
            .set_association(false, &exe(), Language::En)
            .expect("off when there is nothing");
        for _ in 0..2 {
            s.registry
                .set_context_menu(true, &exe(), Language::En)
                .expect("on");
            s.registry
                .set_association(true, &exe(), Language::En)
                .expect("on");
        }
        let state = s.registry.state();
        assert!(state.context_menu && state.association);
    }

    #[test]
    fn the_association_adds_a_prog_id_and_lists_it_in_open_with_without_taking_over() {
        let s = Scratch::new();
        // the extension already belongs to another program, which also lists itself in "Open with"
        let (md, _) = hkcu()
            .create_subkey(s.registry.class_path(".md"))
            .expect(".md");
        md.set_value("", &"md_auto_file").expect("default");
        let (progids, _) = hkcu()
            .create_subkey(s.registry.class_path(r".md\OpenWithProgids"))
            .expect("progids");
        progids.set_value("Other.md", &"").expect("other");

        s.registry
            .set_association(true, &exe(), Language::PtBr)
            .expect("enable");

        let state = s.registry.state();
        assert!(state.association);
        assert!(!state.context_menu);
        assert!(
            !state.is_default,
            "TrailMark is offered, never made the default"
        );
        assert_eq!(
            s.default_of("TrailMark.Markdown").as_deref(),
            Some("Documento Markdown")
        );
        assert_eq!(
            s.default_of(r"TrailMark.Markdown\DefaultIcon"),
            Some(format!("{PROGRAM},0"))
        );
        assert_eq!(
            s.default_of(r"TrailMark.Markdown\shell\open\command"),
            Some(format!("{PROGRAM} \"%1\""))
        );
        for ext in [".md", ".markdown"] {
            let listed = s.class(&format!(r"{ext}\OpenWithProgids")).expect("list");
            // a value named after the ProgID, with no data
            let value = listed.get_value::<String, _>("TrailMark.Markdown");
            assert_eq!(value.ok().as_deref(), Some(""), "{ext}");
        }
        // the other program is untouched
        assert_eq!(s.default_of(".md").as_deref(), Some("md_auto_file"));
        assert!(s
            .class(r".md\OpenWithProgids")
            .and_then(|k| k.get_raw_value("Other.md").ok())
            .is_some());
        assert_eq!(s.app_choice(ASSOCIATION_CHOICE), Some(1));
        assert_eq!(s.app_choice(MENU_CHOICE), None);
    }

    #[test]
    fn removing_the_association_leaves_the_other_programs_alone() {
        let s = Scratch::new();
        let (md, _) = hkcu()
            .create_subkey(s.registry.class_path(".md"))
            .expect(".md");
        md.set_value("", &"md_auto_file").expect("default");
        let (progids, _) = hkcu()
            .create_subkey(s.registry.class_path(r".md\OpenWithProgids"))
            .expect("progids");
        progids.set_value("Other.md", &"").expect("other");

        s.registry
            .set_association(true, &exe(), Language::En)
            .expect("enable");
        s.registry
            .set_association(false, &exe(), Language::En)
            .expect("disable");

        assert!(!s.registry.state().association);
        assert!(s.class("TrailMark.Markdown").is_none());
        assert!(!s
            .registry
            .has_value(r".md\OpenWithProgids", "TrailMark.Markdown"));
        assert!(!s
            .registry
            .has_value(r".markdown\OpenWithProgids", "TrailMark.Markdown"));
        assert_eq!(s.default_of(".md").as_deref(), Some("md_auto_file"));
        assert!(s.registry.has_value(r".md\OpenWithProgids", "Other.md"));
        assert_eq!(s.app_choice(ASSOCIATION_CHOICE), Some(0));
    }

    #[test]
    fn the_state_reflects_what_is_there_even_when_only_part_of_it_is() {
        let s = Scratch::new();
        // someone deleted all but the background entry by hand
        hkcu()
            .create_subkey(
                s.registry
                    .class_path(r"Directory\Background\shell\TrailMark\command"),
            )
            .expect("background only");
        assert!(s.registry.state().context_menu);
        s.registry
            .set_context_menu(false, &exe(), Language::En)
            .expect("disable");
        assert!(!s.registry.state().context_menu);

        // listed in "Open with" but without a ProgID of its own
        let (progids, _) = hkcu()
            .create_subkey(s.registry.class_path(r".markdown\OpenWithProgids"))
            .expect("progids");
        progids
            .set_value("TrailMark.Markdown", &"")
            .expect("listed");
        assert!(s.registry.state().association);
        s.registry
            .set_association(false, &exe(), Language::En)
            .expect("disable");
        assert!(!s.registry.state().association);
    }

    #[test]
    fn the_default_app_is_read_from_the_choice_windows_recorded() {
        let s = Scratch::new();
        let choice = |progid: &str| {
            let (key, _) = hkcu()
                .create_subkey(format!(r"{}\.md\UserChoice", s.registry.file_exts))
                .expect("user choice");
            key.set_value("ProgId", &progid).expect("progid");
        };
        assert!(!s.registry.state().is_default);
        choice("md_auto_file");
        assert!(!s.registry.state().is_default);
        choice("TrailMark.Markdown");
        assert!(s.registry.state().is_default);
    }

    #[test]
    fn newer_windows_records_the_choice_in_user_choice_latest() {
        // Windows 11 builds without a `UserChoice` key keep the choice in `UserChoiceLatest\ProgId`.
        let s = Scratch::new();
        let latest = |progid: &str| {
            let (key, _) = hkcu()
                .create_subkey(format!(
                    r"{}\.md\UserChoiceLatest\ProgId",
                    s.registry.file_exts
                ))
                .expect("user choice latest");
            key.set_value("ProgId", &progid).expect("progid");
        };
        latest("Applications\\other.exe");
        assert!(!s.registry.state().is_default);
        latest("TrailMark.Markdown");
        assert!(s.registry.state().is_default);
    }

    #[test]
    fn switching_the_language_relabels_what_exists_and_creates_nothing() {
        let s = Scratch::new();
        s.registry
            .refresh_labels(Language::De)
            .expect("nothing to do");
        assert!(s.class("Directory").is_none());
        assert!(s.class("TrailMark.Markdown").is_none());

        s.registry
            .set_context_menu(true, &exe(), Language::En)
            .expect("enable");
        s.registry.refresh_labels(Language::Es).expect("relabel");
        for target in &MENU_TARGETS {
            let verb = target.key();
            assert_eq!(
                s.default_of(&verb).as_deref(),
                Some("Abrir con TrailMark"),
                "{verb}"
            );
            assert_eq!(
                s.default_of(&format!(r"{verb}\command")),
                Some(format!("{PROGRAM} \"{}\"", target.placeholder)),
                "the command is left alone"
            );
        }
        // the association was not on, so it is still not
        assert!(!s.registry.state().association);
        assert!(s.class("TrailMark.Markdown").is_none());

        s.registry
            .set_association(true, &exe(), Language::En)
            .expect("enable");
        s.registry.refresh_labels(Language::PtBr).expect("relabel");
        assert_eq!(
            s.default_of("TrailMark.Markdown").as_deref(),
            Some("Documento Markdown")
        );
    }

    #[test]
    fn the_scratch_registry_is_removed_when_the_test_is_done() {
        let root;
        {
            let s = Scratch::new();
            s.registry
                .set_context_menu(true, &exe(), Language::En)
                .expect("enable");
            root = s.root.clone();
            assert!(hkcu().open_subkey(&root).is_ok());
        }
        assert!(hkcu().open_subkey(&root).is_err());
    }
}
