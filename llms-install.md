# PerMark MCP Server — Automated LLM Installation Guide (`llms-install.md`)

PerMark is a local-first desktop Markdown app with a built-in Model Context Protocol (MCP) server (`permark mcp`) over standard input/output (`stdio`). It requires **no API keys, no account, and no environment variables**.

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
curl -fsSL https://github.com/EdelmarSchneider/permark/releases/latest/download/PerMark_0.5.8_amd64.deb -o /tmp/permark.deb
sudo apt-get update && sudo apt-get install -y /tmp/permark.deb
rm -f /tmp/permark.deb
```

## Step 2: Verify the Binary
Verify that `permark` is on `PATH` (or at its standard install path):
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
In the PerMark desktop app, open the project folder and toggle **Settings → Agent Connection** to **On** (this writes `.permark/` state locally so `permark mcp` can serve `get_context`, `list_unread`, `search`, `list_tags`, `list_comments`, `resolve_comment`, `list_notes`, `read_note`, and `open` for that folder).
