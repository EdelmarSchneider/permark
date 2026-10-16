# TrailMark in the Microsoft Store

The Store version of TrailMark is the same program, packaged as an MSIX and built from the same source with one cargo feature,
`store`. This page says what is different about it, how the right-click menu on folders works in a package (and how to try it on a
real install), how to build the package, and how a release gets to the Store.

## What is different in the Store version

| | Installer version | Store version |
| --- | --- | --- |
| **Updates** | The app checks GitHub Releases and offers "Restart to update". | The Store updates it. The app has no updater: no "Check for updates" in Settings or the command palette, no check at start, no update requests. |
| **Agent setup** | Settings → Agent connection shows the program's full path. | It shows the short lines: `claude mcp add trailmark -- trailmark mcp`, `{"command": "trailmark", "args": ["mcp"]}` and the hook `trailmark context`. The package puts `trailmark` on the `PATH` (an app execution alias, `%LOCALAPPDATA%\Microsoft\WindowsApps\trailmark.exe`). The program's own path is inside `C:\Program Files\WindowsApps\...`, changes with every version and is not meant to be run directly. |
| **Markdown files** | The installer can list TrailMark in the "Open with" menu of `.md` files (and Settings can too). | The package declares `.md` and `.markdown`, so TrailMark is in their "Open with" menu. The file's path is the first argument, as `trailmark <path>` takes it. Choose it as the default in Windows Settings, Default apps. |
| **Right-click "Open with TrailMark" on folders** | Optional, from the installer or Settings. On Windows 11 it is under "Show more options". | In the package, always: on a folder and on the background of a folder. It is a small component the package carries, [described below](#the-folder-menu-open-with-trailmark), and on Windows 11 it is in the main right-click menu. |
| **File manager section in Settings** | Switches for the menu entry and the file association. | No switches: the package declares the association and the folder menu itself, and what a packaged program writes to the registry goes to a private copy that Windows Explorer never reads. Settings only says where the entries come from. |
| **Requirements** | Windows 10 or 11. | Windows 10 version 1809 or later, 64-bit (the alias needs 1809). Both versions use the system's WebView2 runtime. |

Windows redirects what a packaged program writes to its own AppData folders into a private folder of the package. The Store
version therefore keeps its settings and recent folders apart from an installer version on the same computer, and Windows removes
them when the Store version is uninstalled. Your documents and the `.trailmark` folder of a project are ordinary files and are not
redirected.

## The folder menu ("Open with TrailMark")

The installer version adds the entry with registry keys under `HKEY_CURRENT_USER\Software\Classes` (`src-tauri/src/shell_integration`).
A packaged program cannot do that: what it writes under `HKEY_CURRENT_USER` goes to a private copy of the registry, and File Explorer
reads the real one. Windows has a way for packages instead (documented in
[Add a File Explorer context menu command to a packaged desktop app](https://learn.microsoft.com/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer)):
the package carries a COM class that implements `IExplorerCommand`, and its manifest says for which items the command is on the menu.
That class is the DLL in `src-tauri/shell-ext`.

### How it works

1. **The component.** `src-tauri/shell-ext` is a small Rust crate that builds `trailmark_shell.dll`. It uses the `windows` crate and
   implements the command (`IExplorerCommand`, and `IObjectWithSite` to learn the folder when the menu is on the background of
   one) and its class factory. It exports `DllGetClassObject` and `DllCanUnloadNow`, and nothing to register
   (`DllRegisterServer` does not exist): Windows reads the manifest. It is a crate of its own, not part of the program's build,
   because its build settings differ (below).
2. **The manifest** (`src-tauri/msstore/AppxManifest.xml`, in the application's `Extensions`; namespaces `com`, `desktop4` and
   `desktop5`):
   * `com:Extension Category="windows.comServer"` with a `com:SurrogateServer` holding a `com:Class` with the class id, the DLL's
     path in the package (`Path="trailmark_shell.dll"`) and `ThreadingModel="STA"`;
   * `desktop4:Extension Category="windows.fileExplorerContextMenus"` with two `desktop5:ItemType` elements, `Directory` (a folder)
     and `Directory\Background` (the empty area of an open folder), each with a `desktop5:Verb` whose `Clsid` is the class id.
3. **Where the DLL lives.** At the root of the package, next to `trailmark.exe` (on an installed package,
   `C:\Program Files\WindowsApps\EdelmarSchneider.TrailMark_<version>_x64__<hash>\`). `scripts/build-msix.ps1` builds it and puts it
   there, and checks that every file the manifest names is in the package.
4. **The class id** is `D5FFB935-7A42-4E3D-90E1-0180A7999B94`, generated once with `[guid]::NewGuid()`. It must never change: it is how
   Windows knows the command from one version of the package to the next. It is in three places, the DLL (`CLSID_OPEN_WITH_TRAILMARK`
   in `shell-ext/src/lib.rs`) and the manifest (the class and, twice, the verbs), and tests keep them equal.
5. **When the menu is drawn.** Explorer asks COM for the class. Windows finds it in the package's manifest and starts a helper
   process, `dllhost.exe`, **with the package's identity**, which loads the DLL (a *surrogate server*): the DLL is never loaded into
   Explorer, so a fault in it cannot take Explorer down, at worst the helper and the entry. Explorer then calls `GetState`, `GetTitle`
   and `GetIcon` while it draws the menu, and `Invoke` when the person chooses the entry.

What the command answers:

| Call | Answer |
| --- | --- |
| `GetTitle` | "Open with TrailMark" in the first of the person's Windows display languages that the app has (English, Português, Español, Deutsch, 简体中文; English for any other). It is the display language of Windows, not the language chosen inside TrailMark, because the rest of the menu is in it too (the installer version's entry follows the app's language). The words are those of the installer version's entry (`entries::menu_label`), and a test keeps the two in step. |
| `GetIcon` | `<package folder>\trailmark.exe,0`: the program's first icon, as the installer version's entry. |
| `GetState` | Enabled. Hidden when more than 15 folders are selected (the limit Windows puts on a registry verb that opens a window for each item, which is what the installer version's entry is). |
| `GetToolTip`, `EnumSubCommands` | Not implemented: no tooltip, no sub-menu. |
| `GetFlags`, `GetCanonicalName` | The default flags, and the class id as the name. |
| `Invoke` | See below. |

**`Invoke`** collects the folders: the paths of the selected items (`IShellItemArray`, each as a file system path; an item without
one, such as a virtual folder, is skipped), or, when nothing is selected (the background of a folder), the folder the Explorer window
shows, which it gets from the site Explorer gave the command (`IObjectWithSite`, `SID_SFolderView`, `IFolderView::GetFolder`), the way
Windows Terminal's "Open in Terminal" does. It keeps the complete paths, each once, at most 15. Then, for each folder, it starts
`trailmark.exe` with the folder's path as the one argument (`"trailmark.exe" "<folder>"`, exactly what the installer version's
registry verb runs with `"%1"`), and TrailMark opens it as a project. The program is found from the DLL's own path: the folder that
holds the DLL is the package's, and `trailmark.exe` is in it; nothing is hard-coded, so it is right for whichever version Windows
installed and for a copy of the package. The program starts in its own folder, not in the folder it opens (a program's working folder
cannot be renamed or deleted, and the person may want to do that from the Explorer window the menu was in). It is the way Windows
Terminal's own shell extension starts `WindowsTerminal.exe` from beside its DLL.

**It must not take anything down.** Every entry point runs inside `catch_unwind` and answers with an error code instead of a panic
(the DLL is built with `panic = "unwind"`; the program's release profile aborts, which is why the DLL's crate is a workspace of its own
and is built with its own `cargo build --release`). The menu calls read only what is at hand (the display languages, once); the work is
in `Invoke`. The DLL carries its own Visual C++ runtime (`static_vcruntime`, as the program's build does) so it loads on a computer
without the redistributable, and uses nothing but Windows: no registry, no files, no network.

### Windows 10 and Windows 11

* **Windows 11** (build 22000 and later): a command declared this way is in the modern, first-level right-click menu, next to Windows'
  own commands, not under "Show more options" (Microsoft's guide says so; whether the classic menu repeats it is not known).
* **Windows 10**: the package extension (`desktop4`/`desktop5`) is documented from version 1809 (build 17763), which is the package's
  minimum, and Windows 10 has only the one right-click menu, so the entry is in it. This has not been tried on a Windows 10 computer.
* Neither version lets the app choose the entry's place in the menu, or show it for drives (`Drive`) or Markdown files (those have
  the "Open with" entry from the file association, below).
* After installing or updating the package the entry can need Explorer to be restarted (or a sign-out and sign-in) before it shows.
* Uninstalling the package removes the entry; nothing is left in the registry.

### Checked without installing anything

* `cargo test --manifest-path src-tauri/shell-ext/Cargo.toml` (part of `npm run check`):
  * unit tests for choosing the title from a list of display languages, for finding the program beside the DLL, for the folders
    to open (complete paths only, each once, at most 15) and the command that opens one (the folder is the one argument, as it is,
    also with spaces, accents, a drive root and a trailing backslash), and for the panic guard;
  * `tests/dll.rs`, which copies the built DLL (and a stand-in `trailmark.exe`) to a temporary folder, loads it with `LoadLibrary`,
    looks up `DllGetClassObject` and `DllCanUnloadNow` with `GetProcAddress`, makes the command through the class factory and calls
    `GetTitle`, `GetIcon` (it is the program beside the loaded DLL), `GetState`, `Invoke` (on a real folder item: the stand-in
    program starts), the site, and `DllCanUnloadNow` as objects come and go. It registers nothing and touches no registry key;
* tests in the program that keep the manifest, the crate, the packaging script and the DLL's class id in step with each other and the
  words in step with the registry entry's;
* `MakeAppx pack` in `scripts/build-msix.ps1` (and in the `Store flavor` CI job) validates the manifest against the package schema,
  the `com`, `desktop4` and `desktop5` elements included, and that the DLL is in the package.

### Try it on a real install

Nothing above shows the entry in a real Explorer menu; that needs a package that is installed. There are two ways, and both end
with the checklist under "What to check".

**A. A package flight in Partner Center (the real thing, signed by the Store).** The flight goes to your own Microsoft account only.

1. Build the package: `.\scripts\build-msix.ps1 -Build -Version 0.5.1` on a computer with the Windows SDK (or a manual run of the
   `Release` workflow, [below](#build-it-in-ci), if the version in `tauri.conf.json` is already the one you want to try). Give it a
   version higher than the one that is live, so that the Store prefers it for your account.
2. In Partner Center, open the app, go to **Package flights**, **Create new package flight**, and name it (such as `folder-menu`).
   Choose or create a flight group made of your own Microsoft account (the email you sign in to the Store with), and give the flight a
   rank above the others.
3. Upload the `.msix` on the flight's **Packages** page and submit the flight. It goes through certification (the Store checks and
   signs it), which can take from hours to a few days.
4. When it is published, open the **Microsoft Store** app on the test computer, signed in with that account, and look for updates (**Library**,
   **Get updates**). The Store installs the flighted version in place of the live one.
5. Do "What to check". Delete the flight in Partner Center when you are done with it.

**B. A sideloaded copy with a test certificate (on a computer you control, no Store involved).** It installs beside the Store
version under another name (`EdelmarSchneider.TrailMarkSideload`, shown as "TrailMark (sideload)"), so the Store version is not
touched. It needs an elevated PowerShell for step 3 only, and it changes the certificate stores until step 6 undoes it.

```powershell
# 1. Build the package with its own identity (about the Windows SDK: see "Build the package locally")
.\scripts\build-msix.ps1 -Build -Sideload -Version 0.5.1
$msix = "src-tauri\target\msix\TrailMark_0.5.1.0_x64_sideload.msix"

# 2. A test certificate whose subject is the sideload package's publisher, and the package signed with it
$cert = New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature -CertStoreLocation "Cert:\CurrentUser\My" `
  -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}") `
  -Subject "CN=TrailMark Sideload" -FriendlyName "TrailMark sideload test"
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe |
  Where-Object FullName -like "*\x64\*" | Sort-Object FullName | Select-Object -Last 1
& $signtool.FullName sign /fd SHA256 /sha1 $cert.Thumbprint $msix

# 3. Trust the certificate for installing (ELEVATED PowerShell): only its public part, in "Trusted People"
Export-Certificate -Cert $cert -FilePath "$env:TEMP\trailmark-sideload.cer"
Import-Certificate -FilePath "$env:TEMP\trailmark-sideload.cer" -CertStoreLocation "Cert:\LocalMachine\TrustedPeople"

# 4. Install, then restart Explorer so that it reads the new menu entry (or sign out and in). Restarting it
#    closes your Explorer windows; Windows starts it again by itself.
Add-AppxPackage -Path $msix
Stop-Process -Name explorer

# 5. Do "What to check" below.

# 6. Take it all away again (the last two lines in an elevated PowerShell)
Get-AppxPackage EdelmarSchneider.TrailMarkSideload | Remove-AppxPackage
Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)"
Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Thumbprint -eq $cert.Thumbprint | Remove-Item
```

The sideload copy declares the same folder-menu class id as the Store package will, so only one of the two should be installed at a
time once the Store has a version with the menu: remove the sideload copy first (step 6). Windows needs "Apps from anywhere" or
"Sideload apps" allowed to install it (the default on a normal Windows 11 installation); a computer managed by an organization can
forbid it.

**What to check** (on Windows 11, then on Windows 10 if there is one):

1. Right-click a folder in Explorer. "Open with TrailMark" is in the first-level menu, with the TrailMark icon, in the Windows display
   language (try one of the five). Choose it: TrailMark opens with that folder as the project.
2. Right-click the empty area inside an open folder: the entry is there too, and opens that folder.
3. Select three folders and right-click one of them: the entry is there, and three TrailMark windows open, one for each.
4. A folder with a space, an accent or a very long path in its name opens correctly. A drive root (`D:\`, from the background of the
   drive) does too.
5. The window that opened is the Store app and not a copy that runs apart: a project opened from the menu appears in **Recent**
   when TrailMark is started from the Start menu afterwards (settings and recent folders live in the package's private AppData, so
   an instance started without the package's identity would not share them).
6. Explorer is not affected: nothing is slow when the menu opens, and `dllhost.exe` (the helper) is the only process that loaded the
   DLL (Process Explorer or `tasklist /m trailmark_shell.dll`).
7. Uninstalling the package removes the entry.

### What only an install can verify

Windows must load the package's declaration for any of this, so these are untested until step 1 above has been done:

* that the entry appears (the manifest passes `MakeAppx`'s schema check, but only Windows says that it registers it), in the Windows 11
  first-level menu, with the right title and icon;
* that Windows loads the DLL into `dllhost.exe` with the package's identity, and calls `GetTitle`, `GetIcon`, `GetState` and `Invoke`
  as the in-process tests do (they call them directly, not across processes). The command is not agile (it lives in the surrogate's
  single-threaded apartment, `Agile = false`), so Explorer reaches it through COM's standard marshaling, with the system's own
  proxies for `IExplorerCommand` and `IObjectWithSite`;
* that `GetModuleFileNameW` reports the DLL's path inside the package folder, so that `trailmark.exe` is found beside it;
* that the program started with `CreateProcess` from the helper runs **with the package's identity** (so it shares settings and
  recent folders with the app started from the Start menu). Windows Terminal's extension starts its program the same way, but if
  this turns out not to hold, the fallback is to start the app through `IApplicationActivationManager::ActivateApplication` with the
  package's app user model id (`<package family name>!TrailMark`) and the folder as the argument;
* the background of a folder: the folder is read from Explorer's site, which is the part nothing in the tests can imitate;
* the behaviour on Windows 10, and the "Show more options" menu on Windows 11.

## Build the package locally

You need Windows, the [Windows SDK](https://developer.microsoft.com/windows/downloads/windows-sdk/) (for `MakeAppx.exe`), and what
building TrailMark needs anyway (Rust, Node.js 20 or later and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).

```powershell
npm ci
.\scripts\build-msix.ps1 -Build
```

`-Build` builds the Store flavor, `npx tauri build --no-bundle --features store --config src-tauri/tauri.msstore.conf.json`, which
is the program with the `store` feature and without the updater's settings, and the folder menu's DLL,
`cargo build --release --manifest-path src-tauri/shell-ext/Cargo.toml`. The script then:

1. lays out a package folder under `src-tauri\target\msix\package`: `trailmark.exe`, `trailmark_shell.dll`, the logos and
   `AppxManifest.xml`. The program and the DLL are the only code it needs: the interface is inside the program, and the web view is
   the system's WebView2 runtime;
2. writes `AppxManifest.xml` from the template in `src-tauri\msstore\AppxManifest.xml`, with the version and the description from
   `tauri.conf.json` filled in;
3. runs `MakeAppx.exe pack`, which checks the manifest against the package schema and that every file it names is there.

The result is `src-tauri\target\msix\TrailMark_<version>_x64.msix`. It is **not signed**: the Store signs what is uploaded to it,
and nobody can install an unsigned package by hand (for a try-out copy that can, see [below](#try-it-on-a-real-install)). Without
`-Build` the script packs what is already built (`-ExePath` and `-DllPath` say where it is); `-OutDir`, `-Version`, `-MakeAppx` and
`-Sideload` are also options (`Get-Help .\scripts\build-msix.ps1`).

The package version is the app's version and a fourth part, always `0`, which the Store reserves for itself: 0.4.0 is 0.4.0.0. The
app's version must be three plain numbers. Every upload to the Store needs a higher version than the one before it.

The manifest's identity (`Identity Name`, `Publisher` and the publisher's display name) is what the Store reserved for the app and
must match it exactly. To look inside a package: `makeappx unpack /p TrailMark_0.4.0.0_x64.msix /d out`.

The logos are committed in `src-tauri\msstore\Assets`. If the icon changes, regenerate them with
`.\scripts\make-msix-assets.ps1` (it draws them from `src-tauri\icons\icon.png`) and commit the result.

## Build it in CI

The `Release` workflow (`.github/workflows/release.yml`) has a job, `Microsoft Store package`, that runs on every `v*` tag next to the
installers. It checks that the tag is the app's version, builds the Store flavor and the folder menu's DLL on `windows-latest`, packs
them with the same script, and keeps the `.msix` as an **artifact of the run** named `TrailMark-msix-<version>`. It is an artifact,
not a release asset: an unsigned package cannot be installed, and would only confuse the people who read the release page.

The `CI` workflow also builds the Store flavor's tests and lints on every push (`Store flavor`), builds the DLL in release, and packs
the manifest with a stand-in program, so a broken manifest, logo or DLL shows up before a release. The DLL's own tests and lints run in
the quality gate (`npm run check`), on all three platforms: on macOS and Linux the crate is empty (it is Windows-only code under
`#![cfg(windows)]`), so nothing there can break.

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
