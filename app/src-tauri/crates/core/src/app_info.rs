//! Facts about the running build. IPC DTO: TS type generated into `app/src/api/generated/`.

use serde::Serialize;
use ts_rs::TS;

/// Version = workspace version (`app/package.json` via `npm run version:sync`).
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: &'static str,
}

/// Facts of this build; pure, no I/O.
#[must_use]
pub fn app_info() -> AppInfo {
    AppInfo { version: VERSION }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_workspace_version() {
        assert_eq!(app_info().version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn serializes_with_camel_case_fields() -> Result<(), serde_json::Error> {
        let json = serde_json::to_string(&app_info())?;
        assert_eq!(json, format!("{{\"version\":\"{VERSION}\"}}"));
        Ok(())
    }
}
