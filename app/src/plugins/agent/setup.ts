// The lines a person copies to connect an agent. The program is named by the path it runs from, which always works;
// `permark` alone only works where it is on the PATH, so it is the fallback when the path is not known.

/** A Windows path: a drive letter (`C:\`, `C:/`) or a server share (`\\server\share`). */
const WINDOWS_PATH = /^(?:[A-Za-z]:|\\\\)/;
/** What a Unix shell takes as it is, in one word. */
const SAFE_UNIX_WORD = /^[\w./:@%+=,-]+$/;

/**
 * The program as one word of a command line. A Windows path is double-quoted when it has spaces, quotes or
 * backslashes (a Unix-style shell such as Git Bash would eat the backslashes); a Unix path is single-quoted unless it
 * is plain (single quotes keep `$` and backticks as they are).
 */
export function programWord(program: string | null): string {
  if (program === null || program === "") return "permark";
  if (WINDOWS_PATH.test(program)) {
    return /[\s"\\]/.test(program) ? `"${program.replaceAll('"', '\\"')}"` : program;
  }
  return SAFE_UNIX_WORD.test(program) ? program : `'${program.replaceAll("'", "'\\''")}'`;
}

/** Claude Code: adds PerMark as an MCP server (run in the project folder). */
export function claudeCodeCommand(program: string | null): string {
  return `claude mcp add permark -- ${programWord(program)} mcp`;
}

/** Any MCP client that reads a `mcpServers` object (Cursor, Claude Desktop, and others). */
export function mcpJson(program: string | null): string {
  return JSON.stringify(
    { mcpServers: { permark: { command: program || "permark", args: ["mcp"] } } },
    null,
    2,
  );
}

/** A Claude Code hook that puts what the person looks at in front of every prompt (`permark context`). */
export function hookJson(program: string | null): string {
  const command = `${programWord(program)} context`;
  return JSON.stringify(
    { hooks: { UserPromptSubmit: [{ hooks: [{ type: "command", command }] }] } },
    null,
    2,
  );
}
