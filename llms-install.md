# PerMark MCP Server — Automated LLM Installation Guide (`llms-install.md`)

PerMark is a local-first desktop unified document workspace for Markdown and the files around it, with a built-in Model Context Protocol (MCP) server (`permark mcp`) over standard input/output (`stdio`). It requires **no API keys, no account, and no environment variables**.

## Step 1: Install the `permark` Binary

Choose the command for the operating system in use:

### Windows
```powershell
winget install EdelmarSchneider.PerMark
```
*(Alternative via Scoop)*:
```powershell
scoop bucket add permark https://github.com/EdelmarSchneider/scoop-permark
scoop install permark
```

### macOS
```bash
brew tap EdelmarSchneider/permark
brew install --cask permark
```

### Linux (Debian / Ubuntu / Mint / Pop!_OS — `x86_64`)
```bash
curl -fsSL https://github.com/EdelmarSchneider/permark/releases/latest/download/PerMark_0.6.1_amd64.deb -o /tmp/permark.deb
sudo apt-get update && sudo apt-get install -y /tmp/permark.deb
rm -f /tmp/permark.deb
```

## Step 2: Verify the Binary
Verify that `permark` is on `PATH` (or at its standard install path). A direct install on Windows or macOS does not put `permark` on `PATH`: then use the full path in the `command` of Step 3.
- **Windows:** `permark` (or `%LOCALAPPDATA%\PerMark\permark.exe`)
- **macOS:** `permark` (or `/Applications/PerMark.app/Contents/MacOS/permark`)
- **Linux:** `/usr/bin/permark`

## Step 3: Configure the MCP Client (`cline_mcp_settings.json` / `.mcp.json`)

Add the `permark` server entry using `stdio` transport:

```json
{
  "mcpServers": {
    "permark": {
      "command": "permark",
      "args": ["mcp"]
    }
  }
}
```

## Step 4: Enable Agent Connection for the Project Folder
The connection is off until the person turns it on, per project. In the PerMark desktop app, open the project folder and switch on **Share this project with agents** in the Settings (the choice is kept in the person's own PerMark data, never in the project, so nothing inside a project can turn it on). `permark mcp` then serves 13 tools for that folder: `get_context`, `list_unread`, `map`, `search`, `list_tags`, `outline`, `read`, `list_comments`, `resolve_comment`, `list_notes`, `read_note`, `activity` and `open`. An agent reads, shows a place and marks comments done; it never edits files through PerMark. With PerMark closed, the tools read from the project's files (`get_context` says PerMark is not open; `open` starts it).
