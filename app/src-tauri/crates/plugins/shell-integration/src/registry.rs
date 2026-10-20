//! Writes, removes and reads the entries under `HKEY_CURRENT_USER`. A `Registry` works on two roots so a test (and a
//! development build) points it at a throwaway key instead of the real `Software\Classes`.

use std::io;
use std::path::Path;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use crate::entries::{tidy_parents, Entry, Key, Language};
use crate::ShellState;

/// Where the entries and the choices live.
#[derive(Debug, Clone)]
pub struct Registry {
    /// Stands for `Software\Classes`.
    classes: String,
    /// The app's key (`Software\PerMark`): the recorded choices.
    app: String,
    /// Tell Explorer after a change (off for throwaway roots).
    notify: bool,
}

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

/// `Ok` when what was to go was not there.
fn missing_is_fine(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl Registry {
    /// The person's real registry.
    #[must_use]
    pub fn current_user() -> Self {
        Self {
            classes: r"Software\Classes".to_owned(),
            app: r"Software\PerMark".to_owned(),
            notify: true,
        }
    }

    /// A registry under `root` (a key below `HKEY_CURRENT_USER`): `<root>\Classes` + `<root>`; Explorer is not told.
    #[must_use]
    pub fn under(root: &str) -> Self {
        Self {
            classes: format!(r"{root}\Classes"),
            app: root.to_owned(),
            notify: false,
        }
    }

    fn class_path(&self, key: &str) -> String {
        format!(r"{}\{key}", self.classes)
    }

    fn write(&self, key: &Key) -> io::Result<()> {
        let (handle, _) = hkcu().create_subkey(self.class_path(&key.path))?;
        for (name, data) in &key.values {
            handle.set_value(name, data)?;
        }
        Ok(())
    }

    fn exists(&self, key: &str) -> bool {
        hkcu()
            .open_subkey_with_flags(self.class_path(key), KEY_READ)
            .is_ok()
    }

    /// Deletes `key` when nothing at all is in it; `Ok(true)` when it is gone (or never was).
    fn delete_if_empty(&self, key: &str) -> io::Result<bool> {
        let path = self.class_path(key);
        let handle = match hkcu().open_subkey_with_flags(&path, KEY_READ) {
            Ok(handle) => handle,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(e) => return Err(e),
        };
        let info = handle.query_info()?;
        if info.sub_keys > 0 || info.values > 0 {
            return Ok(false);
        }
        drop(handle);
        missing_is_fine(hkcu().delete_subkey(&path))?;
        Ok(true)
    }

    /// After an entry went: its empty parents go too, nearest first, stopping at the first that holds anything.
    fn tidy_up(&self, key: &str) {
        for parent in tidy_parents(key) {
            if !matches!(self.delete_if_empty(&parent), Ok(true)) {
                break;
            }
        }
    }

    fn record(&self, entry: Entry, on: bool) -> io::Result<()> {
        let (handle, _) = hkcu().create_subkey_with_flags(&self.app, KEY_WRITE)?;
        handle.set_value(entry.choice(), &u32::from(on))
    }

    fn changed(&self) {
        if self.notify {
            crate::notify::associations_changed();
        }
    }

    /// What is there now (any key of an entry = on).
    #[must_use]
    pub fn state(&self) -> ShellState {
        let on = |entry: Entry| entry.verb_keys().iter().any(|key| self.exists(key));
        ShellState {
            menu: on(Entry::Menu),
            new_file: on(Entry::NewFile),
        }
    }

    /// Adds (`on`) or removes an entry, records the choice, tells Explorer.
    ///
    /// # Errors
    /// The registry refused a write | delete.
    pub fn set(&self, entry: Entry, on: bool, exe: &Path, language: Language) -> io::Result<()> {
        if on {
            for key in entry.keys(exe, language) {
                self.write(&key)?;
            }
        } else {
            for key in entry.verb_keys() {
                missing_is_fine(hkcu().delete_subkey_all(self.class_path(&key)))?;
                self.tidy_up(&key);
            }
        }
        self.record(entry, on)?;
        self.changed();
        Ok(())
    }

    /// Rewords the entries that are there; `Ok(false)` when there was nothing to reword.
    ///
    /// # Errors
    /// The registry refused a write.
    pub fn relabel(&self, language: Language) -> io::Result<bool> {
        let mut any = false;
        for entry in Entry::ALL {
            for key in entry.verb_keys() {
                if let Ok(handle) = hkcu().open_subkey_with_flags(self.class_path(&key), KEY_WRITE)
                {
                    handle.set_value("", &entry.label(language))?;
                    any = true;
                }
            }
        }
        if any {
            self.changed();
        }
        Ok(any)
    }

    /// Removes every entry and records both choices off (the plugin was switched off: it leaves nothing behind).
    ///
    /// # Errors
    /// The registry refused a delete.
    pub fn withdraw(&self, exe: &Path) -> io::Result<()> {
        for entry in Entry::ALL {
            self.set(entry, false, exe, Language::En)?;
        }
        Ok(())
    }

    /// The recorded choice (`None`: never chosen).
    #[must_use]
    pub fn choice(&self, entry: Entry) -> Option<bool> {
        hkcu()
            .open_subkey_with_flags(&self.app, KEY_READ)
            .ok()?
            .get_value::<u32, _>(entry.choice())
            .ok()
            .map(|v| v != 0)
    }
}
