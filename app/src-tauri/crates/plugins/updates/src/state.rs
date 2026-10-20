//! The updater as a pure state machine: `(state, event) -> state`. An event that makes no sense in the current state
//! (a late answer) leaves it as it is.

use serde::Serialize;
use ts_rs::TS;

/// Which step failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "UpdateStage")]
pub enum Stage {
    Check,
    Download,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "state")]
#[ts(export)]
pub enum UpdateState {
    Idle,
    Checking {
        manual: bool,
    },
    /// A check the person asked for found nothing newer.
    UpToDate,
    Available {
        version: String,
        notes: Option<String>,
    },
    Downloading {
        version: String,
        /// 0..=100 once the size is known.
        percent: Option<u8>,
    },
    /// Downloaded and verified: a restart installs it.
    Ready {
        version: String,
    },
    /// Shown only for something the person did (an automatic check fails quietly).
    Error {
        stage: Stage,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    CheckStarted {
        manual: bool,
    },
    Found {
        version: String,
        notes: Option<String>,
    },
    NothingNew,
    CheckFailed {
        message: String,
    },
    DownloadStarted,
    Progress {
        done: u64,
        total: Option<u64>,
    },
    Downloaded,
    DownloadFailed {
        message: String,
    },
    RestartFailed {
        message: String,
    },
    /// The person saw "up to date" | the error.
    Dismiss,
}

fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|t| *t > 0)?;
    let value = (u128::from(done) * 100 / u128::from(total)).min(100);
    u8::try_from(value).ok()
}

impl UpdateState {
    /// The state after `event`.
    #[must_use]
    pub fn next(&self, event: Event) -> Self {
        use UpdateState as S;
        match (self, event) {
            (S::Idle | S::UpToDate | S::Error { .. }, Event::CheckStarted { manual }) => {
                S::Checking { manual }
            }
            (S::Checking { .. }, Event::Found { version, notes }) => {
                S::Available { version, notes }
            }
            (S::Checking { manual: true }, Event::NothingNew) => S::UpToDate,
            (S::Checking { manual: false }, Event::NothingNew | Event::CheckFailed { .. })
            | (S::UpToDate | S::Error { .. }, Event::Dismiss) => S::Idle,
            (S::Checking { manual: true }, Event::CheckFailed { message }) => S::Error {
                stage: Stage::Check,
                message,
            },
            (S::Available { version, .. }, Event::DownloadStarted) => S::Downloading {
                version: version.clone(),
                percent: None,
            },
            (S::Downloading { version, .. }, Event::Progress { done, total }) => S::Downloading {
                version: version.clone(),
                percent: percent(done, total),
            },
            (S::Downloading { version, .. }, Event::Downloaded) => S::Ready {
                version: version.clone(),
            },
            (S::Downloading { .. }, Event::DownloadFailed { message }) => S::Error {
                stage: Stage::Download,
                message,
            },
            (S::Ready { .. }, Event::RestartFailed { message }) => S::Error {
                stage: Stage::Install,
                message,
            },
            (state, _) => state.clone(),
        }
    }
}
