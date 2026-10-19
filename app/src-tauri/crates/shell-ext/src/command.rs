//! The menu command: what Explorer asks of it while it draws the menu (title, icon, state: only what is at hand) and
//! `Invoke` when the person picks it (the work).

// What `#[implement]` writes (the COM glue around the struct) trips these pedantic lints.
#![allow(clippy::inline_always, clippy::ref_as_ptr)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

use windows::core::{implement, Error, IUnknown, Interface, Ref, Result, BOOL, GUID, PWSTR};
use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL, E_POINTER};
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray, ECF_DEFAULT,
    ECS_ENABLED, ECS_HIDDEN,
};

use crate::guard::guarded;
use crate::module::{self, InUse};
use crate::{labels, launch, shell, CLSID_NEW_MARKDOWN, CLSID_OPEN_WITH_PERMARK};

/// Which entry this command is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuAction {
    /// Open the folder.
    Open,
    /// Make a Markdown file in the folder and open it.
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

    /// The title in Windows' display language, found once (it changes only at the next sign-in).
    fn title(self) -> &'static str {
        static OPEN: OnceLock<&'static str> = OnceLock::new();
        static NEW: OnceLock<&'static str> = OnceLock::new();
        match self {
            Self::Open => {
                OPEN.get_or_init(|| labels::pick(&labels::OPEN, &shell::display_languages()))
            }
            Self::NewMarkdown => {
                NEW.get_or_init(|| labels::pick(&labels::NEW_MARKDOWN, &shell::display_languages()))
            }
        }
    }
}

// `Agile = false`: the object lives in the surrogate's single-threaded apartment (the class is declared `STA`); the
// `RefCell` and the site (an Explorer object bound to that apartment) rely on it.
#[implement(IExplorerCommand, IObjectWithSite, Agile = false)]
pub(crate) struct MenuCommand {
    action: MenuAction,
    /// Explorer's site: how the command finds the folder when used on the background of one.
    site: RefCell<Option<IUnknown>>,
    _in_use: InUse,
}

impl MenuCommand {
    pub(crate) fn new(action: MenuAction) -> Self {
        Self {
            action,
            site: RefCell::new(None),
            _in_use: InUse::new(),
        }
    }

    /// The selected folders, or with nothing selected (the background of a folder) the folder the window shows. A
    /// selection whose items have no path is not "nothing selected": it opens nothing.
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

impl IExplorerCommand_Impl for MenuCommand_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guarded(|| shell::alloc_string(self.action.title()))
    }

    /// The program's first icon (the installer version's entry shows the same).
    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guarded(|| shell::alloc_string(&format!("{},0", program()?.display())))
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    /// A name of its own, so Explorer does not take it for another command.
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(self.action.class_id())
    }

    /// Shown for folders; hidden for a selection of more folders than one use opens.
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
            launch::start(&program, &folders, self.action == MenuAction::NewMarkdown)
                .map_err(Error::from)
        })
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_DEFAULT.0.unsigned_abs())
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

impl IObjectWithSite_Impl for MenuCommand_Impl {
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
            // SAFETY: `site` is not null (checked above): the place the caller wants the interface in.
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
