// The lines a person copies to connect an agent (docs/design/v0.4.md section 4). The program is named by the path it runs
// from, which always works; `trailmark` alone only works where it is on the PATH, so it is the fallback when the path
// is not known, and the Settings text says it can replace the path.

/**
 * The program as a word of a command line: quoted when it has spaces, quotes or backslashes in it (a Windows path
 * loses its backslashes in a Unix-style shell such as Git Bash unless it is quoted).
 */
export function programWord(program: string | null): string {
  if (program === null || program === "") return "trailmark";
  return /[\s"\\]/.test(program) ? `"${program.replaceAll('"', '\\"')}"` : program;
}

/** Claude Code: adds TrailMark as an MCP server. */
export function claudeCodeCommand(program: string | null): string {
  return `claude mcp add trailmark -- ${programWord(program)} mcp`;
}

/** Any MCP client that reads a `mcpServers` object (Cursor, Claude Desktop, and others). */
export function mcpJson(program: string | null): string {
  const command = JSON.stringify(program || "trailmark");
  return [
    "{",
    '  "mcpServers": {',
    `    "trailmark": { "command": ${command}, "args": ["mcp"] }`,
    "  }",
    "}",
  ].join("\n");
}

/**
 * A Claude Code hook that adds what the person is looking at to every prompt: the output of `trailmark context` is
 * put in front of the model. Other tools can run the same command from their own hooks.
 */
export function hookJson(program: string | null): string {
  const command = JSON.stringify(`${programWord(program)} context`);
  return [
    "{",
    '  "hooks": {',
    '    "UserPromptSubmit": [',
    `      { "hooks": [{ "type": "command", "command": ${command} }] }`,
    "    ]",
    "  }",
    "}",
  ].join("\n");
}
