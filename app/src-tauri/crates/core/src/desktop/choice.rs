//! Setting and giving back the default app of a group, over whatever records it on this system.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::FileGroup;
use crate::state::StateData;
use crate::{ErrorKind, IpcError};

/// `detail` of a refusal to give a type back to "no choice" where the system cannot record that (macOS): the person
/// picks another app there (Finder: Get Info, Open with).
pub const NO_HANDLER_TO_RESTORE: &str = "noHandlerToRestore";

/// The refusal of a system that cannot give `key` back to "no choice".
#[must_use]
pub fn no_handler_to_restore(key: &str) -> IpcError {
    IpcError::new(ErrorKind::Unsupported)
        .subject(key)
        .detail(NO_HANDLER_TO_RESTORE)
}

/// Where the system records the default handler of a file type (a MIME type on Linux, a UTI on macOS).
pub trait Handlers {
    /// The keys of a group on this system (MIME types | UTIs).
    fn keys(&self, group: FileGroup) -> Vec<String>;
    /// The handler recorded for `key` now.
    fn get(&self, key: &str) -> Option<String>;
    /// Records `handler` for `key` (`None`: no choice, the system decides).
    ///
    /// # Errors
    /// The system refused | the settings file cannot be written.
    fn set(&self, key: &str, handler: Option<&str>) -> Result<(), IpcError>;
    /// PerMark's own name as a handler (desktop file id | bundle id).
    fn me(&self) -> &str;
}

/// What PerMark remembers about the default app: whether it asked, and the handlers it replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAppState {
    pub asked: bool,
    /// Key -> the handler before PerMark (`None`: there was none).
    #[serde(default)]
    pub previous: BTreeMap<String, Option<String>>,
}

impl StateData for DefaultAppState {
    const VERSION: u32 = 1;
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// PerMark opens every type of the group (a group with no key on this system: no).
#[must_use]
pub fn is_ours(handlers: &dyn Handlers, group: FileGroup) -> bool {
    let keys = handlers.keys(group);
    !keys.is_empty()
        && keys.iter().all(|key| {
            handlers
                .get(key)
                .is_some_and(|handler| same(&handler, handlers.me()))
        })
}

/// Makes PerMark the default of the group (`on`), recording each replaced handler first, or gives each type back to the
/// handler it had (`off`; only types PerMark still holds: a choice the person made elsewhere since then stays).
///
/// # Errors
/// The first key the system refused (the keys before it are already set; `state` records them).
pub fn choose(
    handlers: &dyn Handlers,
    state: &mut DefaultAppState,
    group: FileGroup,
    on: bool,
) -> Result<(), IpcError> {
    for key in handlers.keys(group) {
        let now = handlers.get(&key);
        let held = now.as_deref().is_some_and(|h| same(h, handlers.me()));
        if on {
            if held {
                continue;
            }
            state.previous.entry(key.clone()).or_insert(now);
            handlers.set(&key, Some(handlers.me()))?;
        } else if held {
            // the record goes only once the type is given back: a refusal keeps it for the next try
            let before = state.previous.get(&key).cloned().flatten();
            handlers.set(&key, before.as_deref())?;
            state.previous.remove(&key);
        } else {
            // the person chose another app elsewhere: nothing to give back
            state.previous.remove(&key);
        }
    }
    Ok(())
}
