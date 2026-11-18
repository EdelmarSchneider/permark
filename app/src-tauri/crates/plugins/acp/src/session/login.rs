//! The sign-in methods an agent offers in its `initialize` answer, as the window shows them.

use agent_client_protocol_schema::v1::AuthMethodTerminal;
use serde_json::Value;

use crate::dto::{LoginKind, LoginMethod};

/// The sign-in methods of an `initialize` answer, read one by one (a method of a kind this client does not know is
/// left out; a typed decode would read it as an `agent` method and send it to `authenticate`). A terminal method's
/// line = `shown` (the agent's command) + its arguments.
pub(super) fn login_methods(raw: Option<&Value>, shown: &str) -> Vec<LoginMethod> {
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
                    (LoginKind::Terminal, Some(terminal_line(&terminal, shown)))
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

/// The line a person runs for a terminal sign-in: its variables (`NAME=value`, by name), the agent's command, the
/// method's arguments.
fn terminal_line(method: &AuthMethodTerminal, shown: &str) -> String {
    let mut env: Vec<(&String, &String)> = method.env.iter().collect();
    env.sort();
    env.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .chain(std::iter::once(shown.to_owned()))
        .chain(method.args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
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
