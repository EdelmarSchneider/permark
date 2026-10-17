# TrailMark MCP Server — Automated LLM Installation Guide (`llms-install.md`)

TrailMark is a local-first desktop Markdown IDE with a built-in Model Context Protocol (MCP) server (`trailmark mcp`) over standard input/output (`stdio`). It requires **no API keys, no account, and no environment variables**.

## Step 1: Install the `trailmark` Binary

Choose the command matching the user's operating system:

### Windows
```powershell
winget install EdelmarSchneider.TrailMark
```
*(Alternative via Scoop)*:
```powershell
scoop bucket add trailmark https://github.com/EdelmarSchneider/scoop-trailmark
scoop install trailmark
```

### macOS
```bash
brew tap EdelmarSchneider/trailmark
brew install --cask trailmark
```

### Linux (Debian / Ubuntu / Mint / Pop!_OS — `x86_64`)
```bash
curl -fsSL https://github.com/EdelmarSchneider/trailmark/releases/latest/download/TrailMark_0.5.5_amd64.deb -o /tmp/trailmark.deb
sudo apt-get update && sudo apt-get install -y /tmp/trailmark.deb
rm -f /tmp/trailmark.deb
```

## Step 2: Verify the Binary
Verify that `trailmark` is on `PATH` (or at its standard install path):
- **Windows:** `trailmark` (or `%LOCALAPPDATA%\TrailMark\trailmark.exe`)
- **macOS:** `trailmark` (or `/Applications/TrailMark.app/Contents/MacOS/trailmark`)
- **Linux:** `/usr/bin/trailmark`

## Step 3: Configure the MCP Client (`cline_mcp_settings.json` / `.mcp.json`)

Add the `trailmark` server entry using `stdio` transport:

```json
{
  "mcpServers": {
    "trailmark": {
      "command": "trailmark",
      "args": ["mcp"]
    }
  }
}
```

## Step 4: Enable Agent Connection for the Project Folder
In the TrailMark desktop app, open the project folder and toggle **Settings → Agent Connection** to **On** (this writes `.trailmark/` state locally so `trailmark mcp` can serve `get_context`, `list_unread`, `search`, `list_tags`, `list_comments`, `resolve_comment`, `list_notes`, `read_note`, and `open` for that folder).
