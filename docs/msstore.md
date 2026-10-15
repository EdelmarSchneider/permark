# TrailMark in the Microsoft Store

The Store version of TrailMark is the same program, packaged as an MSIX and built from the same source with one cargo feature,
`store`. This page says what is different about it, how to build the package, and how a release gets to the Store.

## What is different in the Store version

| | Installer version | Store version |
| --- | --- | --- |
| **Updates** | The app checks GitHub Releases and offers "Restart to update". | The Store updates it. The app has no updater: no "Check for updates" in Settings or the command palette, no check at start, no update requests. |
| **Agent setup** | Settings → Agent connection shows the program's full path. | It shows the short lines: `claude mcp add trailmark -- trailmark mcp`, `{"command": "trailmark", "args": ["mcp"]}` and the hook `trailmark context`. The package puts `trailmark` on the `PATH` (an app execution alias, `%LOCALAPPDATA%\Microsoft\WindowsApps\trailmark.exe`). The program's own path is inside `C:\Program Files\WindowsApps\...`, changes with every version and is not meant to be run directly. |
| **Markdown files** | The installer can list TrailMark in the "Open with" menu of `.md` files (and Settings can too). | The package declares `.md` and `.markdown`, so TrailMark is in their "Open with" menu. The file's path is the first argument, as `trailmark <path>` takes it. Choose it as the default in Windows Settings, Default apps. |
| **Right-click "Open with TrailMark" on folders** | Optional, from the installer or Settings. | **Not available.** A package cannot declare it: it needs a shell extension (a COM component), which this package does not have. Open folders from inside TrailMark, or with `trailmark <folder>`. |
| **File manager section in Settings** | Switches for the menu entry and the file association. | Hidden: the package declares the association itself, and what a packaged program writes to the registry goes to a private copy that Windows Explorer never reads. |
| **Requirements** | Windows 10 or 11. | Windows 10 version 1809 or later, 64-bit (the alias needs 1809). Both versions use the system's WebView2 runtime. |

Windows redirects what a packaged program writes to its own AppData folders into a private folder of the package. The Store
version therefore keeps its settings and recent folders apart from an installer version on the same computer, and Windows removes
them when the Store version is uninstalled. Your documents and the `.trailmark` folder of a project are ordinary files and are not
redirected.

## Build the package locally

You need Windows, the [Windows SDK](https://developer.microsoft.com/windows/downloads/windows-sdk/) (for `MakeAppx.exe`), and what
building TrailMark needs anyway (Rust, Node.js 20 or later and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).

```powershell
npm ci
.\scripts\build-msix.ps1 -Build
```

`-Build` builds the Store flavor, `npx tauri build --no-bundle --features store --config src-tauri/tauri.msstore.conf.json`, which
is the program with the `store` feature and without the updater's settings. The script then:

1. lays out a package folder under `src-tauri\target\msix\package`: `trailmark.exe`, the logos and `AppxManifest.xml`. The program is
   the only file it needs: the interface is inside it, and the web view is the system's WebView2 runtime;
2. writes `AppxManifest.xml` from the template in `src-tauri\msstore\AppxManifest.xml`, with the version and the description from
   `tauri.conf.json` filled in;
3. runs `MakeAppx.exe pack`, which checks the manifest against the package schema and that every file it names is there.

The result is `src-tauri\target\msix\TrailMark_<version>_x64.msix`. It is **not signed**: the Store signs what is uploaded to it,
and nobody can install an unsigned package by hand. Without `-Build` the script packs the program that is already built
(`-ExePath` says where it is); `-OutDir`, `-Version` and `-MakeAppx` are also options (`Get-Help .\scripts\build-msix.ps1`).

The package version is the app's version and a fourth part, always `0`, which the Store reserves for itself: 0.4.0 is 0.4.0.0. The
app's version must be three plain numbers. Every upload to the Store needs a higher version than the one before it.

The manifest's identity (`Identity Name`, `Publisher` and the publisher's display name) is what the Store reserved for the app and
must match it exactly. To look inside a package: `makeappx unpack /p TrailMark_0.4.0.0_x64.msix /d out`.

The logos are committed in `src-tauri\msstore\Assets`. If the icon changes, regenerate them with
`.\scripts\make-msix-assets.ps1` (it draws them from `src-tauri\icons\icon.png`) and commit the result.

## Build it in CI

The `Release` workflow (`.github/workflows/release.yml`) has a job, `Microsoft Store package`, that runs on every `v*` tag next to the
installers. It checks that the tag is the app's version, builds the Store flavor on `windows-latest`, packs it with the same script,
and keeps the `.msix` as an **artifact of the run** named `TrailMark-msix-<version>`. It is an artifact, not a release asset: an
unsigned package cannot be installed, and would only confuse the people who read the release page.

The `CI` workflow also builds the Store flavor's tests and lints on every push (`Store flavor`), and packs the manifest with a
stand-in program, so a broken manifest or logo shows up before a release.

To build a package without a release, or from a ref other than the tag, start the workflow by hand: **Actions, Release, Run
workflow** (or `gh workflow run release.yml`):

```bash
gh workflow run release.yml --ref develop -f tag=v0.4.0 -f platforms=none -f msix=true -f msix_ref=develop
```

- `platforms` must be `none`. Any other value rebuilds the installers of `tag` and publishes them again.
- `tag` is required by the workflow and names an existing tag; it only decides what is built when `msix_ref` is empty.
- `msix` turns the package job on, and `msix_ref` is the branch, tag or commit to build it from. It needs the packaging
  (`scripts/build-msix.ps1`) and the Store flavor, which are part of the source.

A version released before the Store flavor existed cannot be packaged from its own tag, because the flavor is part of the source. The
first package (0.4.0.0) is built from the commit that added the flavor. Its app code is that of the v0.4.0 release (the two had no
difference in `src` and `src-tauri/src` when the flavor was added) plus the flavor, and its version is still 0.4.0.

## How a release reaches the Store

Today the last step is manual:

1. Release as usual (`git flow release finish`, push `main`, `develop` and the tag). The `Release` workflow builds the installers
   and, in the `Microsoft Store package` job, `TrailMark-msix-<version>`.
2. Download that artifact from the workflow run (it is a zip with the `.msix` inside).
3. In Partner Center, open the app, start a new submission, upload the `.msix` under Packages, put the release notes (the version's
   section of `CHANGELOG.md`) in the listing, and submit. The Store signs the package and runs its own checks.

The first submission also needs the listing itself: the description, screenshots (`docs/assets` has some), the age rating, the
privacy policy URL, https://edelmarschneider.github.io/trailmark/privacy.html (the page is `site/privacy.html`), and the reason for
the one restricted capability the package declares, `runFullTrust`: TrailMark is a desktop program that reads and writes the folders the
person opens, with the person's own rights.

Submitting from the workflow is possible later with the [Microsoft Store Developer CLI](https://learn.microsoft.com/windows/apps/publish/msstore-dev-cli/overview)
(`msstore publish`). It needs credentials of an Azure AD application that is linked to the Partner Center account (a tenant id, a
client id and a client secret, kept as repository secrets), and those do not exist yet. Until then the upload stays manual.
