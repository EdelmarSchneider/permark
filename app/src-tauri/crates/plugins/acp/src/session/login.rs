//! The sign-in methods an agent offers in its `initialize` answer, as the window shows them.

use std::collections::BTreeMap;

use agent_client_protocol_schema::v1::AuthMethodTerminal;
use permark_core::settings::is_unsafe_char;
use serde_json::Value;

use crate::agents::Plan;
use crate::dto::{LoginKind, LoginMethod};

/// The sign-in methods of an `initialize` answer, read one by one (a method of a kind this client does not know is
/// left out; a typed decode would read it as an `agent` method and send it to `authenticate`). A terminal method's
/// lines: [`terminal_line`] for the agent's `plan` (a method whose line cannot be written safely is left out).
pub(super) fn login_methods(raw: Option<&Value>, plan: &Plan) -> Vec<LoginMethod> {
    raw.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|method| {
            let id = method.get("id")?.as_str()?.to_owned();
            let text = |key: &str| method.get(key).and_then(Value::as_str).map(str::to_owned);
            let (kind, hint) = match method.get("type").and_then(Value::as_str) {
                None | Some("agent") => (LoginKind::Agent, None),
                Some("terminal") => {
                    let terminal =
                        serde_json::from_value::<AuthMethodTerminal>(method.clone()).ok()?;
                    let line = terminal_line(&terminal, plan, cfg!(windows))?;
                    (LoginKind::Terminal, Some(line))
                }
                Some("env_var") => (LoginKind::Env, Some(variable_names(method))),
                Some(_) => return None,
            };
            Some(LoginMethod {
                name: text("name").unwrap_or_else(|| id.clone()),
                description: text("description"),
                id,
                kind,
                hint,
            })
        })
        .collect()
}

/// What a person pastes into a terminal for a terminal sign-in: the variables (`plan`'s, then the method's, which win
/// on the same name; by name) on a line of their own in the shell the computer's terminal opens (`windows`:
/// PowerShell; else a POSIX shell), then the agent's command (`plan.shown`) + the method's arguments, each word read
/// back as given. `None`: a word holds a line break | a control character, or a variable name no shell can set (the
/// pasted text would run something else).
fn terminal_line(method: &AuthMethodTerminal, plan: &Plan, windows: bool) -> Option<String> {
    let mut env: BTreeMap<&str, &str> = plan
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    env.extend(
        method
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    let mut words = std::iter::once(plan.shown.as_str())
        .chain(method.args.iter().map(String::as_str))
        .chain(env.iter().flat_map(|(name, value)| [*name, *value]));
    if words.any(|word| word.chars().any(is_unsafe_char)) || !env.keys().all(|n| variable_name(n)) {
        return None;
    }
    let command = std::iter::once(plan.shown.clone())
        .chain(method.args.iter().map(|arg| quoted(arg, windows)))
        .collect::<Vec<_>>()
        .join(" ");
    if env.is_empty() {
        return Some(command);
    }
    let set = if windows {
        // always quoted: a bare value after `=` is read as a command
        env.iter()
            .map(|(name, value)| format!("$env:{name}={}", single_quoted(value, true)))
            .collect::<Vec<_>>()
            .join("; ")
    } else {
        let pairs = env
            .iter()
            .map(|(name, value)| format!("{name}={}", quoted(value, false)))
            .collect::<Vec<_>>();
        format!("export {}", pairs.join(" "))
    };
    Some(format!("{set}\n{command}"))
}

/// A name both shells set as written: a letter | `_`, then letters, digits, `_`.
fn variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `word` as a shell reads it back: as it is when plain, else in single quotes.
fn quoted(word: &str, windows: bool) -> String {
    let plain = !word.is_empty()
        && !word.starts_with('@')
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
    if plain {
        word.to_owned()
    } else {
        single_quoted(word, windows)
    }
}

/// `word` in single quotes, literal in PowerShell and POSIX shells: PowerShell doubles an inner quote (its typographic
/// ones count as quotes too); a POSIX shell closes, escapes and reopens.
fn single_quoted(word: &str, windows: bool) -> String {
    let mut out = String::from("'");
    for c in word.chars() {
        match c {
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' if windows => {
                out.push(c);
                out.push(c);
            }
            '\'' => out.push_str("'\\''"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// The variable names an environment sign-in asks for (`vars[].name` | `varName`), comma separated.
fn variable_names(method: &Value) -> String {
    let listed = method
        .get("vars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|var| var.get("name").and_then(Value::as_str));
    let single = method.get("varName").and_then(Value::as_str);
    listed.chain(single).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn plan_with(env: &[(&str, &str)]) -> Plan {
        Plan {
            program: PathBuf::from("vtcode"),
            args: vec!["--acp".to_owned()],
            env: env
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            shown: "vtcode --acp".to_owned(),
        }
    }

    fn method(value: &Value) -> AuthMethodTerminal {
        serde_json::from_value(value.clone()).unwrap()
    }

    /// The variables (the agent's start ones, then the method's, which win) go on a line of their own in the shell
    /// a person meets on the computer: PowerShell on Windows, a POSIX shell elsewhere. Every word reads back as given.
    #[test]
    fn a_terminal_line_sets_the_variables_first_in_the_computers_shell() {
        let plan = plan_with(&[("VT_ACP_ENABLED", "1"), ("MODE", "plan")]);
        let login = method(&json!({
            "type": "terminal", "id": "login", "name": "Sign in",
            "args": ["auth", "login", "my team", "$HOME", ""],
            "env": { "MODE": "it's mine", "TOKEN_DIR": "C:\\Users\\a b" },
        }));
        assert_eq!(
            terminal_line(&login, &plan, true).as_deref(),
            Some(
                "$env:MODE='it''s mine'; $env:TOKEN_DIR='C:\\Users\\a b'; $env:VT_ACP_ENABLED='1'\n\
                 vtcode --acp auth login 'my team' '$HOME' ''"
            )
        );
        assert_eq!(
            terminal_line(&login, &plan, false).as_deref(),
            Some(
                "export MODE='it'\\''s mine' TOKEN_DIR='C:\\Users\\a b' VT_ACP_ENABLED=1\n\
                 vtcode --acp auth login 'my team' '$HOME' ''"
            )
        );
        // no variables: the command alone
        let bare = method(
            &json!({ "type": "terminal", "id": "login", "name": "Sign in", "args": ["--login"] }),
        );
        assert_eq!(
            terminal_line(&bare, &plan_with(&[]), true).as_deref(),
            Some("vtcode --acp --login")
        );
    }

    /// A word that holds a line break | a control character, or a variable name the shell cannot set, would make a
    /// line that runs something else: the method is left out.
    #[test]
    fn a_method_whose_line_cannot_be_written_safely_is_left_out() {
        let methods = json!([
            { "type": "terminal", "id": "break", "name": "A", "args": ["login\nrm -rf ~"] },
            { "type": "terminal", "id": "bell", "name": "B", "args": ["x"], "env": { "A": "\u{7}" } },
            { "type": "terminal", "id": "name", "name": "C", "env": { "A;B": "1" } },
            { "type": "terminal", "id": "fine", "name": "D", "args": ["login"] },
        ]);
        let listed: Vec<String> = login_methods(Some(&methods), &plan_with(&[("X-Y", "1")]))
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(
            listed,
            Vec::<String>::new(),
            "the start's own variable is no name either"
        );
        let listed: Vec<String> = login_methods(Some(&methods), &plan_with(&[]))
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(listed, ["fine"]);
    }
}
