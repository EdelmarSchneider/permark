//! Prompt data (D-2): one set for every model (engine renders the chat markup per prompt profile). Plain text out, no
//! JSON (D-1). Tuned on the eval set (U6): change these consts only with an eval run.
//!
//! - [`messages`]: system rules + 2 few-shot turns + the raw piece as last user turn.
//! - [`max_tokens`]: answer cap = ceil(1.3 x piece tokens) + 32 (a correction is about as long as its input).
//! - [`SAMPLING`]: greedy, fixed seed (same piece -> same answer).
//! - [`CorrectionJob`]: the above as the job of `local-ai`'s piece runner (model of job "correction", [`CONTEXT`]).

use permark_plugin_local_ai::engine::{ChatMessage, Role, Sampling};
use permark_plugin_local_ai::pieces::PieceJob;
use permark_plugin_local_ai::Task;

/// Context window asked for a correction session, tokens.
pub const CONTEXT: u32 = 4096;

/// Correction as a piece job: this module's prompt, answer cap and sampling; no per-run parameters.
pub struct CorrectionJob;

impl PieceJob for CorrectionJob {
    type Params = ();

    const TASK: Task = Task::Correction;
    const CONTEXT: u32 = CONTEXT;
    const SAMPLING: Sampling = SAMPLING;

    fn messages((): &(), text: &str) -> Vec<ChatMessage> {
        messages(text)
    }

    fn preamble((): &()) -> String {
        preamble()
    }

    fn max_tokens(piece_tokens: u32) -> u32 {
        max_tokens(piece_tokens)
    }
}

/// Rules of the task (English; the model answers in the language of the text).
pub const SYSTEM: &str = "\
You correct spelling, grammar and punctuation. Nothing else.
- Write in the same language as the text. Never translate.
- Change as little as possible. Keep the wording, the tone and the regional spelling (for example pt-BR stays pt-BR, \
pt-PT stays pt-PT, en-US stays en-US, en-GB stays en-GB).
- Keep every line break, every Markdown symbol and every marker like ⟦1⟧ exactly as written.
- Do not add final punctuation to headings or list items.
- If there are no errors, return the text unchanged.
- Output only the corrected text, with no comment.";

/// Few-shot turns (user text, expected answer): (a) clean English with a marker -> unchanged; (b) pt-BR with 2 errors
/// + a marker -> fixed.
pub const FEW_SHOT: [(&str, &str); 2] = [
    (
        "The meeting moved to Thursday, see ⟦1⟧ for the new agenda.",
        "The meeting moved to Thursday, see ⟦1⟧ for the new agenda.",
    ),
    (
        "Nós vamos revizar o documento amanha, conforme combinado em ⟦1⟧.",
        "Nós vamos revisar o documento amanhã, conforme combinado em ⟦1⟧.",
    ),
];

/// Greedy decoding, fixed seed.
pub const SAMPLING: Sampling = Sampling {
    temperature: 0.0,
    top_p: 1.0,
    seed: Some(SEED),
};

const SEED: u64 = 42;

/// Answer cap growth over the piece, tenths (13 = 1.3x).
const GROWTH_TENTHS: u64 = 13;
/// Answer cap slack, tokens.
const SLACK: u32 = 32;

/// System rules + few-shot turns, then `piece` as the last user turn.
#[must_use]
pub fn messages(piece: &str) -> Vec<ChatMessage> {
    let turn = |role, content: &str| ChatMessage {
        role,
        content: content.to_owned(),
    };
    let mut messages = vec![turn(Role::System, SYSTEM)];
    for (user, answer) in FEW_SHOT {
        messages.push(turn(Role::User, user));
        messages.push(turn(Role::Assistant, answer));
    }
    messages.push(turn(Role::User, piece));
    messages
}

/// The text before the piece ([`SYSTEM`] + [`FEW_SHOT`]), for its token count.
#[must_use]
pub fn preamble() -> String {
    let mut text = SYSTEM.to_owned();
    for (user, answer) in FEW_SHOT {
        text.push('\n');
        text.push_str(user);
        text.push('\n');
        text.push_str(answer);
    }
    text
}

/// Answer cap for a piece of `piece_tokens` tokens: ceil(1.3 x n) + 32.
#[must_use]
pub fn max_tokens(piece_tokens: u32) -> u32 {
    let grown = (u64::from(piece_tokens) * GROWTH_TENTHS).div_ceil(10);
    u32::try_from(grown)
        .unwrap_or(u32::MAX)
        .saturating_add(SLACK)
}
