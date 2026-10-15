// The lines a person copies to connect an agent (docs/design/v0.4.md section 4). The program is named by the path it runs
// from, which always works; `trailmark` alone only works where it is on the PATH, so it is the fallback when the path
// is not known, and the Settings text says it can replace the path.

/** A Windows path: a drive letter (`C:\`, `C:/`) or a server share (`\\server\share`). */
const WINDOWS_PATH = /^(?:[A-Za-z]:|\\\\)/;
/** What a Unix shell takes as it is, in a word: letters, digits and a few signs that mean nothing to it. */
const SAFE_UNIX_WORD = /^[\w./:@%+=,-]+$/;

/**
 * The program as a word of a command line. A Windows path is double-quoted when it has spaces, quotes or backslashes
 * in it (it loses its backslashes in a Unix-style shell such as Git Bash unless it is quoted). Anything else is a Unix
 * path, which is single-quoted unless it is plain: inside double quotes a shell would still expand a `$` or a backtick
 * in a folder name (an AppImage in an odd place), and single quotes keep every character as it is.
 */
export function programWord(program: string | null): string {
  if (program === null || program === "") return "trailmark";
  if (WINDOWS_PATH.test(program)) {
    return /[\s"\\]/.test(program) ? `"${program.replaceAll('"', '\\"')}"` : program;
  }
  return SAFE_UNIX_WORD.test(program) ? program : `'${program.replaceAll("'", "'\\''")}'`;
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
