//! Fake plugins for tests only: record starts + drops into a shared log.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use super::{Manifest, Plugin, PluginCx, PluginError, PluginId, PluginKind, PluginScope, Running};
use crate::conditions::Requirement;

/// Shared log of lifecycle events: `"start:<id>"`, `"stop:<id>"`.
pub(crate) type Log = Arc<Mutex<Vec<String>>>;

pub(crate) struct Fake {
    manifest: Manifest,
    pub(crate) log: Log,
    pub(crate) fail: bool,
}

impl Fake {
    pub(crate) fn new(id: &'static str, scope: PluginScope, deps: &'static [&'static str]) -> Self {
        Self {
            manifest: Manifest {
                id: PluginId::of(id),
                kind: PluginKind::Feature,
                scope,
                requires: Requirement::NONE,
                depends_on: Cow::Owned(deps.iter().map(|d| PluginId::of(d)).collect()),
                default_enabled: true,
            },
            log: Log::default(),
            fail: false,
        }
    }

    pub(crate) fn with_log(mut self, log: &Log) -> Self {
        self.log = Arc::clone(log);
        self
    }

    pub(crate) fn failing(mut self) -> Self {
        self.fail = true;
        self
    }

    pub(crate) fn default_off(mut self) -> Self {
        self.manifest.default_enabled = false;
        self
    }
}

pub(crate) fn fake(
    id: &'static str,
    scope: PluginScope,
    deps: &'static [&'static str],
) -> Arc<dyn Plugin> {
    Arc::new(Fake::new(id, scope, deps))
}

struct FakeRunning {
    id: PluginId,
    log: Log,
}

impl Running for FakeRunning {}

impl Drop for FakeRunning {
    fn drop(&mut self) {
        if let Ok(mut log) = self.log.lock() {
            log.push(format!("stop:{}", self.id));
        }
    }
}

impl Plugin for Fake {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn start(&self, cx: &PluginCx) -> Result<Box<dyn Running>, PluginError> {
        if self.fail {
            return Err(PluginError::Start {
                id: cx.id.clone(),
                reason: "fake failure".to_owned(),
            });
        }
        if let Ok(mut log) = self.log.lock() {
            log.push(format!("start:{}", cx.id));
        }
        Ok(Box::new(FakeRunning {
            id: cx.id.clone(),
            log: Arc::clone(&self.log),
        }))
    }
}
