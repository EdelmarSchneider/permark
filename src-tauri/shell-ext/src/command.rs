//! The "Open with PerMark" command: what File Explorer asks of it, and what it answers.
//!
//! Explorer makes one of these when it builds the right-click menu of a folder (or of the background of one), asks it for
//! its title, icon and state, and calls `Invoke` when the person picks it. Everything before `Invoke` runs while the menu is
//! being drawn, so it only reads what is already at hand; the work is in `Invoke`.

// What `#[implement]` writes (the COM glue around the struct below) trips these two pedantic lints; it is not ours to change.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

use windows::core::Error;
use windows::core::{implement, IUnknown, Interface, Ref, Result, BOOL, GUID, PWSTR};
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, E_POINTER};
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray, ECF_DEFAULT,
    ECS_ENABLED, ECS_HIDDEN,
};

use crate::guard::guarded;
use crate::launch;
use crate::module::{self, InUse};
use crate::{labels, shell, CLSID_NEW_MARKDOWN, CLSID_OPEN_WITH_PERMARK};

/// Which entry of the menu this command is.
#[derive(Clone, Copy)]
pub(crate) enum MenuAction {
    /// Open the folder.
    Open,
    /// Make a Markdown file in the folder and open the folder.
    NewMarkdown,
}

impl MenuAction {
    pub(crate) fn from_class(class: &GUID) -> Option<Self> {
        if *class == CLSID_OPEN_WITH_PERMARK {
            Some(Self::Open)
        } else if *class == CLSID_NEW_MARKDOWN {
            Some(Self::NewMarkdown)
        } else {
            None
        }
    }

    fn class_id(self) -> GUID {
        match self {
            Self::Open => CLSID_OPEN_WITH_PERMARK,
            Self::NewMarkdown => CLSID_NEW_MARKDOWN,
        }
    }
}

// `Agile = false`: the default makes the object agile (callable from any apartment, through the free-threaded marshaler),
// which is not what it is. It lives in the surrogate's single-threaded apartment, and the `RefCell` and the site it keeps
// (an Explorer object, bound to that apartment) rely on that.
#[implement(IExplorerCommand, IObjectWithSite, Agile = false)]
pub(crate) struct OpenWithPerMark {
    action: MenuAction,
    /// Explorer's site: how the command finds the folder when it was used on the background of one. Explorer calls the
    /// command on one thread (the COM class is declared `STA`), so a `RefCell` is enough.
    site: RefCell<Option<IUnknown>>,
    _in_use: InUse,
}

impl OpenWithPerMark {
    pub(crate) fn new(action: MenuAction) -> Self {
        Self {
            action,
            site: RefCell::new(None),
            _in_use: InUse::new(),
        }
    }

    /// The folders the person means: the selected folders, or, with nothing selected (the background of a folder), the folder
    /// the window is showing. A selection of items that have no path is not "nothing selected": it opens nothing.
    fn folders(&self, items: Option<&IShellItemArray>) -> Vec<PathBuf> {
        if shell::count(items).unwrap_or(0) > 0 {
            return shell::selected_paths(items);
        }
        let site = self.site.try_borrow().ok().and_then(|site| site.clone());
        site.as_ref()
            .and_then(shell::folder_of_site)
            .into_iter()
            .collect()
    }
}

/// The program of the package, beside this DLL.
fn program() -> Result<PathBuf> {
    module::path()
        .and_then(|dll| launch::package_program(&dll))
        .ok_or_else(|| Error::from(E_FAIL))
}

/// The title, in the display language of Windows. Found once: the language changes only when the person signs in again.
fn title(action: MenuAction) -> &'static str {
    match action {
        MenuAction::Open => {
            static TITLE: OnceLock<&'static str> = OnceLock::new();
            TITLE.get_or_init(|| labels::for_languages(&shell::display_languages()))
        }
        MenuAction::NewMarkdown => {
            static TITLE: OnceLock<&'static str> = OnceLock::new();
            TITLE.get_or_init(|| labels::new_markdown_for_languages(&shell::display_languages()))
        }
    }
}

impl IExplorerCommand_Impl for OpenWithPerMark_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guarded(|| shell::alloc_string(title(self.action)))
    }

    /// The first icon of the program: the one the installer version's entry shows.
    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guarded(|| shell::alloc_string(&format!("{},0", program()?.display())))
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    /// A name of its own, so that Explorer does not take it for another command.
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(self.action.class_id())
    }

    /// Shown for folders (what the package registers it for). Hidden for a selection of more folders than it opens.
    fn GetState(&self, items: Ref<IShellItemArray>, _ok_to_be_slow: BOOL) -> Result<u32> {
        guarded(|| {
            let selected = shell::count(items.as_ref()).unwrap_or(0) as usize;
            let state = if selected > launch::MOST_FOLDERS {
                ECS_HIDDEN
            } else {
                ECS_ENABLED
            };
            Ok(state.0.unsigned_abs())
        })
    }

    fn Invoke(&self, items: Ref<IShellItemArray>, _bind_context: Ref<IBindCtx>) -> Result<()> {
        guarded(|| {
            let program = program()?;
            let folders = launch::folders_to_open(self.folders(items.as_ref()));
            let started = match self.action {
                MenuAction::Open => launch::open(&program, &folders),
                MenuAction::NewMarkdown => launch::new_markdown(&program, &folders),
            };
            started.map_err(Error::from)
        })
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0.unsigned_abs())
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

impl IObjectWithSite_Impl for OpenWithPerMark_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
        guarded(|| {
            *self
                .site
                .try_borrow_mut()
                .map_err(|_| Error::from(E_FAIL))? = site.cloned();
            Ok(())
        })
    }

    fn GetSite(&self, iid: *const GUID, site: *mut *mut c_void) -> Result<()> {
        guarded(|| {
            if iid.is_null() || site.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: `site` was checked not to be null; it is the place the caller wants the interface written to.
            unsafe { *site = std::ptr::null_mut() };
            let held = self.site.try_borrow().map_err(|_| Error::from(E_FAIL))?;
            match held.as_ref() {
                // SAFETY: both pointers are valid (checked above); `query` writes an interface the caller then owns.
                Some(held) => unsafe { held.query(iid, site).ok() },
                None => Err(E_FAIL.into()),
            }
        })
    }
}
