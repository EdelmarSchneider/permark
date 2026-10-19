//! The `pm-asset` URI scheme: pictures, sound and video of a workspace for the document view.
//!
//! URL path `/<workspace id>/<project path>`. The workspace is looked up by id (a closed one answers 404); the path goes
//! through the vault (or the single file's folder scope), so nothing outside the workspace can be read; the core decides
//! what may be served as (names on both sides) and which bytes go out (ranges for sound and video).

use permark_core::document::assets::{self, Asset, AssetError, Part};
use permark_core::links::percent_decode;
use permark_core::paths::{VaultError, VaultPath};
use permark_core::workspace::{Workspace, WorkspaceId};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, UriSchemeContext, UriSchemeResponder, Wry};

use crate::shell::state;

pub const SCHEME: &str = "pm-asset";

/// Answers a request on the blocking pool (reading a file must not stall the UI).
#[allow(clippy::needless_pass_by_value)] // the signature Tauri's scheme handler requires
pub fn handle(
    context: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let handle = context.app_handle().clone();
    let path = request.uri().path().to_owned();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    tauri::async_runtime::spawn_blocking(move || {
        let answer = serve(&handle, &path, range.as_deref());
        responder.respond(response(answer));
    });
}

/// Why nothing is served.
enum Refusal {
    NotFound,
    Forbidden,
    Failed,
}

impl From<VaultError> for Refusal {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Escapes(_) | VaultError::Invalid(_) => Self::Forbidden,
            VaultError::NotFound(_) | VaultError::NotAFile(_) | VaultError::NotAFolder(_) => {
                Self::NotFound
            }
            VaultError::Io { .. } => Self::Failed,
        }
    }
}

impl From<AssetError> for Refusal {
    fn from(error: AssetError) -> Self {
        match error {
            AssetError::NotMedia => Self::Forbidden,
            AssetError::NotFound => Self::NotFound,
            AssetError::Io(_) => Self::Failed,
        }
    }
}

/// `/<id>/<project path>` (the whole path percent-encoded once, slashes too, or only its parts) -> the workspace and
/// the decoded project path.
fn split(uri_path: &str) -> Option<(WorkspaceId, String)> {
    let decoded = percent_decode(uri_path.trim_start_matches('/'));
    let (id, rest) = decoded.split_once('/')?;
    let id = WorkspaceId(id.parse().ok()?);
    Some((id, rest.to_owned()))
}

fn serve(handle: &AppHandle, uri_path: &str, range: Option<&str>) -> Result<Asset, Refusal> {
    let (id, path) = split(uri_path).ok_or(Refusal::NotFound)?;
    let workspace = state(handle)
        .registry
        .workspace(id)
        .ok_or(Refusal::NotFound)?;
    let parsed = VaultPath::parse(&path)?;
    let abs = match &workspace {
        Workspace::Project(core) => core.vault().resolve(&parsed)?,
        Workspace::SingleFile(file) => file.scope.resolve_asset(&parsed)?,
    };
    Ok(assets::read(&path, &abs, range)?)
}

fn response(result: Result<Asset, Refusal>) -> Response<Vec<u8>> {
    let mut builder = Response::builder();
    let (status, content_type, body) = match result {
        Ok(asset) => {
            builder = builder.header(header::ACCEPT_RANGES, "bytes");
            match asset.part {
                Part::Whole => (StatusCode::OK, asset.mime, asset.bytes),
                Part::Range { start, end } => {
                    builder = builder.header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{}", asset.total),
                    );
                    (StatusCode::PARTIAL_CONTENT, asset.mime, asset.bytes)
                }
                Part::Unsatisfiable => {
                    builder =
                        builder.header(header::CONTENT_RANGE, format!("bytes */{}", asset.total));
                    (StatusCode::RANGE_NOT_SATISFIABLE, "text/plain", Vec::new())
                }
            }
        }
        Err(refusal) => {
            let status = match refusal {
                Refusal::NotFound => StatusCode::NOT_FOUND,
                Refusal::Forbidden => StatusCode::FORBIDDEN,
                Refusal::Failed => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, "text/plain", Vec::new())
        }
    };
    builder
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff")
        // a picture shown as a document of its own (SVG) could not run scripts or load anything
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; sandbox",
        )
        .body(body)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split_into_workspace_and_decoded_path() {
        assert_eq!(
            split("/7/docs/img/a%20b.png"),
            Some((WorkspaceId(7), "docs/img/a b.png".to_owned()))
        );
        assert_eq!(
            split("/12/%40wt/feat/p.png"),
            Some((WorkspaceId(12), "@wt/feat/p.png".to_owned()))
        );
        assert_eq!(
            split("/1/caf%C3%A9.png").map(|s| s.1),
            Some("café.png".to_owned())
        );
        // Tauri encodes the whole path, slashes included
        assert_eq!(
            split("/7%2Fdocs%2Fimg%2Fa%20b.png"),
            Some((WorkspaceId(7), "docs/img/a b.png".to_owned()))
        );
        for bad in ["", "/", "/7", "/x/a.png", "/-1/a.png", "//a.png"] {
            assert_eq!(split(bad), None, "{bad}");
        }
    }

    #[test]
    fn answers_carry_the_headers_that_make_ranges_and_safety_work() {
        let ok = response(Ok(Asset {
            mime: "video/mp4",
            bytes: vec![1, 2, 3],
            part: Part::Range { start: 0, end: 2 },
            total: 10,
        }));
        assert_eq!(ok.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(ok.headers()[header::CONTENT_RANGE], "bytes 0-2/10");
        assert_eq!(ok.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(ok.headers()["X-Content-Type-Options"], "nosniff");
        assert!(ok.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("sandbox"));
        let late = response(Ok(Asset {
            mime: "video/mp4",
            bytes: Vec::new(),
            part: Part::Unsatisfiable,
            total: 10,
        }));
        assert_eq!(late.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(late.headers()[header::CONTENT_RANGE], "bytes */10");
        for (refusal, status) in [
            (Refusal::NotFound, StatusCode::NOT_FOUND),
            (Refusal::Forbidden, StatusCode::FORBIDDEN),
            (Refusal::Failed, StatusCode::INTERNAL_SERVER_ERROR),
        ] {
            let answer = response(Err(refusal));
            assert_eq!(answer.status(), status);
            assert_eq!(answer.body().len(), 0);
        }
    }
}
