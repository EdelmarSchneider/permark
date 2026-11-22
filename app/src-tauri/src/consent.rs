//! The system dialog before a program the person set up (the editor, the Assistant's own command) starts while not yet
//! allowed. Its words live here, in every app language: a page never words it (a page that could would hide which
//! program starts or swap the buttons). For the system's language the system's own list picks one of these
//! translations; the interface language a page reports counts only where the system names none.

use std::sync::Arc;

use permark_core::plugins::{GuardedConsent, ProgramConsent, ProgramConsentApi, ServiceLease};
use permark_core::process::{ProgramApprovals, ProgramCall};
use permark_core::settings::{is_format_char, is_unsafe_char, system_languages, LanguagePref};
use permark_core::state::StateStore;
use permark_core::workspace::CoreEnv;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::shell::Shell;

/// The dialog's words in one language; `{command}` = the program's full path, its arguments on the next line.
struct Words {
    title: &'static str,
    message: &'static str,
    allow: &'static str,
    deny: &'static str,
}

const ENGLISH: Words = Words {
    title: "Allow this program to start?",
    message: "PerMark is about to start a program you haven't allowed yet:\n\n{command}\n\nAllow it only if you set \
              it up yourself in PerMark's settings.",
    allow: "Allow",
    deny: "Don't allow",
};

/// Every app language (`LanguagePref` but `System`; `LOCALES` of `app/src/core/i18n/locales.ts`).
const WORDS: &[(LanguagePref, Words)] = &[
    (LanguagePref::En, ENGLISH),
    (
        LanguagePref::PtBr,
        Words {
            title: "Permitir que este programa seja iniciado?",
            message: "O PerMark vai iniciar um programa que você ainda não permitiu:\n\n{command}\n\nPermita apenas \
                      se você mesmo o configurou nas Configurações do PerMark.",
            allow: "Permitir",
            deny: "Não permitir",
        },
    ),
    (
        LanguagePref::Es,
        Words {
            title: "¿Permitir que se inicie este programa?",
            message: "PerMark va a iniciar un programa que aún no has permitido:\n\n{command}\n\nPermítelo solo si \
                      lo configuraste tú en la Configuración de PerMark.",
            allow: "Permitir",
            deny: "No permitir",
        },
    ),
    (
        LanguagePref::De,
        Words {
            title: "Start dieses Programms erlauben?",
            message: "PerMark möchte ein Programm starten, das du noch nicht erlaubt hast:\n\n{command}\n\nErlaube \
                      es nur, wenn du es selbst in den Einstellungen von PerMark eingerichtet hast.",
            allow: "Erlauben",
            deny: "Nicht erlauben",
        },
    ),
    (
        LanguagePref::ZhCn,
        Words {
            title: "允许启动此程序吗？",
            message: "PerMark 即将启动一个你尚未允许的程序：\n\n{command}\n\n仅当它是你自己在 PerMark 设置中配置的程序时才允许。",
            allow: "允许",
            deny: "不允许",
        },
    ),
    (
        LanguagePref::It,
        Words {
            title: "Consentire l'avvio di questo programma?",
            message: "PerMark sta per avviare un programma che non hai ancora consentito:\n\n{command}\n\n\
                      Consentilo solo se l'hai configurato tu nelle Impostazioni di PerMark.",
            allow: "Consenti",
            deny: "Non consentire",
        },
    ),
    (
        LanguagePref::Ja,
        Words {
            title: "このプログラムの起動を許可しますか？",
            message: "PerMark は、まだ許可されていないプログラムを起動しようとしています:\n\n{command}\n\n\
                      PerMark の設定でご自身が設定したものである場合にのみ許可してください。",
            allow: "許可",
            deny: "許可しない",
        },
    ),
    (
        LanguagePref::Ko,
        Words {
            title: "이 프로그램의 실행을 허용할까요?",
            message: "PerMark가 아직 허용하지 않은 프로그램을 실행하려고 합니다:\n\n{command}\n\nPerMark 설정에서 \
                      직접 설정한 프로그램인 경우에만 허용하세요.",
            allow: "허용",
            deny: "허용 안 함",
        },
    ),
    (
        LanguagePref::Fr,
        Words {
            title: "Autoriser le lancement de ce programme ?",
            message: "PerMark va lancer un programme que vous n'avez pas encore autorisé :\n\n{command}\n\n\
                      Autorisez-le uniquement si vous l'avez configuré vous-même dans les Paramètres de PerMark.",
            allow: "Autoriser",
            deny: "Ne pas autoriser",
        },
    ),
    (
        LanguagePref::PtPt,
        Words {
            title: "Permitir que este programa seja iniciado?",
            message: "O PerMark vai iniciar um programa que ainda não permitiu:\n\n{command}\n\nPermita apenas se o \
                      configurou pessoalmente nas Definições do PerMark.",
            allow: "Permitir",
            deny: "Não permitir",
        },
    ),
    (
        LanguagePref::Ca,
        Words {
            title: "Voleu permetre que s'iniciï aquest programa?",
            message: "El PerMark està a punt d'iniciar un programa que encara no heu permès:\n\n{command}\n\n\
                      Permeteu-ho només si l'heu configurat vós mateix a la Configuració del PerMark.",
            allow: "Permet",
            deny: "No permetis",
        },
    ),
    (
        LanguagePref::Gl,
        Words {
            title: "Permitir que se inicie este programa?",
            message: "PerMark vai iniciar un programa que aínda non permitiu:\n\n{command}\n\nPermítao só se o \
                      configurou vostede mesmo na Configuración de PerMark.",
            allow: "Permitir",
            deny: "Non permitir",
        },
    ),
    (
        LanguagePref::Ro,
        Words {
            title: "Permiți pornirea acestui program?",
            message: "PerMark urmează să pornească un program pe care nu l-ai permis încă:\n\n{command}\n\n\
                      Permite-l doar dacă l-ai configurat chiar tu în Setările PerMark.",
            allow: "Permite",
            deny: "Nu permite",
        },
    ),
    (
        LanguagePref::ZhTw,
        Words {
            title: "允許啟動此程式嗎？",
            message: "PerMark 即將啟動一個您尚未允許的程式：\n\n{command}\n\n僅在這是您自己在 PerMark「設定」中設定的程式時才允許。",
            allow: "允許",
            deny: "不允許",
        },
    ),
    (
        LanguagePref::Id,
        Words {
            title: "Izinkan program ini dijalankan?",
            message: "PerMark akan menjalankan program yang belum Anda izinkan:\n\n{command}\n\nIzinkan hanya jika \
                      Anda sendiri yang menyiapkannya di Pengaturan PerMark.",
            allow: "Izinkan",
            deny: "Jangan izinkan",
        },
    ),
    (
        LanguagePref::Vi,
        Words {
            title: "Cho phép khởi chạy chương trình này?",
            message: "PerMark sắp khởi chạy một chương trình bạn chưa cho phép:\n\n{command}\n\nChỉ cho phép nếu \
                      chính bạn đã thiết lập nó trong Cài đặt của PerMark.",
            allow: "Cho phép",
            deny: "Không cho phép",
        },
    ),
    (
        LanguagePref::Ru,
        Words {
            title: "Разрешить запуск этой программы?",
            message: "PerMark собирается запустить программу, которую вы ещё не разрешили:\n\n{command}\n\n\
                      Разрешайте, только если вы сами настроили её в настройках PerMark.",
            allow: "Разрешить",
            deny: "Не разрешать",
        },
    ),
    (
        LanguagePref::Tr,
        Words {
            title: "Bu programın başlatılmasına izin verilsin mi?",
            message: "PerMark, henüz izin vermediğiniz bir programı başlatmak üzere:\n\n{command}\n\nYalnızca bunu \
                      PerMark Ayarları'nda kendiniz ayarladıysanız izin verin.",
            allow: "İzin ver",
            deny: "İzin verme",
        },
    ),
    (
        LanguagePref::Pl,
        Words {
            title: "Zezwolić na uruchomienie tego programu?",
            message: "PerMark zamierza uruchomić program, na który jeszcze nie zezwolono:\n\n{command}\n\nZezwól \
                      tylko wtedy, gdy ten program został skonfigurowany przez Ciebie w Ustawieniach PerMark.",
            allow: "Zezwól",
            deny: "Nie zezwalaj",
        },
    ),
    (
        LanguagePref::Uk,
        Words {
            title: "Дозволити запуск цієї програми?",
            message: "PerMark збирається запустити програму, яку ви ще не дозволили:\n\n{command}\n\nДозволяйте, \
                      лише якщо ви самі налаштували її в налаштуваннях PerMark.",
            allow: "Дозволити",
            deny: "Не дозволяти",
        },
    ),
];

/// The dialog's language: the person's choice; for the system's language the first of `system` (best first) PerMark
/// has, and only when the system names none (macOS) the interface language a page `reported` (a page may not pick the
/// words over the system's own list).
fn consent_language<S: AsRef<str>>(
    choice: LanguagePref,
    system: &[S],
    reported: Option<&str>,
) -> LanguagePref {
    match reported {
        Some(tag) if system.is_empty() => choice.resolve(&[tag]),
        _ => choice.resolve(system),
    }
}

/// The words of `language` (English for one without, `System` included: resolve first).
fn words_of(language: LanguagePref) -> &'static Words {
    WORDS
        .iter()
        .find(|(of, _)| *of == language)
        .map_or(&ENGLISH, |(_, words)| words)
}

/// Longest argument | variable the dialog shows, in characters of its shown form; a longer one ends in `…`.
const MAX_SHOWN: usize = 300;

/// The dialog's message: the program's full path, its arguments on the next line, then its variables (`NAME=value`:
/// part of what is allowed) on one more.
fn message(words: &Words, call: &ProgramCall) -> String {
    let mut command = shown(&call.program.display().to_string(), usize::MAX);
    let lines = [
        call.args
            .iter()
            .map(|arg| shown(arg, MAX_SHOWN))
            .collect::<Vec<_>>(),
        call.env
            .iter()
            .map(|(name, value)| shown(&format!("{name}={value}"), MAX_SHOWN))
            .collect(),
    ];
    for line in lines.iter().filter(|line| !line.is_empty()) {
        command.push('\n');
        command.push_str(&line.join(" "));
    }
    words.message.replace("{command}", &command)
}

/// `text` with every character that draws nothing | moves the text around it as its code: controls, bidi and other
/// format characters (zero-width, direction marks), line | paragraph separators and every space but the plain one.
/// None of them may hide or reorder part of a path | an argument in the dialog. At most `max` characters (an escape
/// never cut), then `…`.
fn shown(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for c in text.chars() {
        let piece = if is_unsafe_char(c) || is_format_char(c) || (c.is_whitespace() && c != ' ') {
            format!("\\u{{{:x}}}", u32::from(c))
        } else {
            c.to_string()
        };
        count += piece.chars().count();
        if count > max {
            out.push('…');
            break;
        }
        out.push_str(&piece);
    }
    out
}

struct DialogConsent {
    handle: AppHandle,
}

impl ProgramConsent for DialogConsent {
    fn allow(&self, call: &ProgramCall) -> bool {
        let choice = crate::shell::state(&self.handle).env.settings().language;
        let reported = crate::views::words(&self.handle).language;
        let words = words_of(consent_language(
            choice,
            &system_languages(),
            reported.as_deref(),
        ));
        let mut dialog = self
            .handle
            .dialog()
            .message(message(words, call))
            .title(words.title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                words.allow.to_owned(),
                words.deny.to_owned(),
            ));
        // over the app's window, never behind it: the focused one, else the work window focused last, else the first
        let focused = self
            .handle
            .windows()
            .into_values()
            .find(|w| w.is_focused().unwrap_or(false));
        let parent = focused
            .or_else(|| {
                crate::shell::state(&self.handle)
                    .registry
                    .last_focused_work()
                    .and_then(|label| self.handle.get_window(&label))
            })
            .or_else(|| self.handle.get_window("main"));
        if let Some(parent) = &parent {
            dialog = dialog.parent(parent);
        }
        dialog.blocking_show()
    }
}

/// Offers the dialog for the app's whole run (one at a time, a no kept until the app ends: [`GuardedConsent`]).
pub fn provide(env: &CoreEnv, handle: &AppHandle) -> ServiceLease {
    let dialog = Arc::new(DialogConsent {
        handle: handle.clone(),
    });
    let consent: Arc<dyn ProgramConsent> = Arc::new(GuardedConsent::new(dialog));
    env.services().provide(Arc::new(ProgramConsentApi(consent)))
}

/// The approvals of the app's profile, asking through the dialog; a program inside an open project never starts.
pub fn approvals(shell: &Shell) -> ProgramApprovals {
    let consent = shell
        .env
        .services()
        .get::<ProgramConsentApi>()
        .map(|api| Arc::clone(&api.0));
    let projects = shell
        .registry
        .cores()
        .iter()
        .map(|core| core.root().path().to_path_buf())
        .collect();
    ProgramApprovals::of(&shell.app_store, consent).outside(projects)
}

/// First start with approvals: the editor (`editor` template) and the Assistant's own command set up before them count
/// as allowed. Runs before any window: a page never gets to set one up in time to be taken along.
pub fn adopt(store: &StateStore, editor: &str) {
    let mut calls: Vec<ProgramCall> = Vec::new();
    calls.extend(permark_core::document::editor::program_call(editor));
    #[cfg(feature = "plugin-acp")]
    calls.extend(permark_plugin_acp::own_command(store));
    ProgramApprovals::of(store, None).adopt(&calls);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's languages as the frontend lists them.
    const LOCALES_TS: &str = include_str!("../../src/core/i18n/locales.ts");

    #[test]
    fn every_app_language_has_its_own_words() {
        let block = LOCALES_TS
            .split("export const LOCALES = [")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap();
        let tags: Vec<&str> = block.split(['"', '\'']).skip(1).step_by(2).collect();
        assert_eq!(tags.len(), 20, "{tags:?}");
        for tag in tags {
            let language = LanguagePref::parse(tag).unwrap();
            let words = &WORDS
                .iter()
                .find(|(of, _)| *of == language)
                .unwrap_or_else(|| panic!("no words for {tag}"))
                .1;
            assert!(words.message.contains("{command}"), "{tag}");
            // the dialog tells its buttons apart by label
            assert_ne!(words.allow, words.deny, "{tag}");
            for word in [words.title, words.message, words.allow, words.deny] {
                assert!(!word.trim().is_empty(), "{tag}");
                if language != LanguagePref::En {
                    assert!(
                        ![ENGLISH.title, ENGLISH.message, ENGLISH.deny].contains(&word),
                        "{tag}: {word}"
                    );
                }
            }
        }
        assert_eq!(WORDS.len(), LanguagePref::ALL.len() - 1);
    }

    fn call(program: &str, args: &[&str], env: &[(&str, &str)]) -> ProgramCall {
        ProgramCall {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn the_message_names_the_full_path_and_shows_hidden_characters() {
        let text = message(&ENGLISH, &call("/opt/tools/code", &["-g", "{path}"], &[]));
        assert!(
            text.contains("\n\n/opt/tools/code\n-g {path}\n\n"),
            "{text}"
        );
        let hostile = message(
            &ENGLISH,
            &call("/opt/edit\u{202e}txt.exe", &["a\nb", "\u{7}"], &[]),
        );
        assert!(hostile.contains("/opt/edit\\u{202e}txt.exe"), "{hostile}");
        assert!(hostile.contains("\na\\u{a}b \\u{7}\n"), "{hostile}");
        let command = hostile.split("\n\n").nth(1).unwrap();
        assert_eq!(command.lines().count(), 2, "{command}");
        assert!(!command.chars().any(|c| c != '\n' && is_unsafe_char(c)));
        // the system's language (no window reported one yet) reads as English
        assert_eq!(words_of(LanguagePref::System).allow, ENGLISH.allow);
        assert_eq!(
            words_of(LanguagePref::System.resolve(&["de-AT"])).allow,
            "Erlauben"
        );
    }

    /// For the system's language the system's own list picks the words; a page's report counts only where the system
    /// names none, and a person's choice ignores both.
    #[test]
    fn the_dialog_speaks_the_systems_language_and_a_page_cannot_pick_it() {
        let austria = ["de-AT".to_owned()];
        assert_eq!(
            consent_language(LanguagePref::System, &austria, Some("ja")),
            LanguagePref::De
        );
        // a system language PerMark lacks reads as English, never as the page's
        assert_eq!(
            consent_language(LanguagePref::System, &["sv-SE"], Some("ja")),
            LanguagePref::En
        );
        // no list from the system (macOS): the page's report, else English
        assert_eq!(
            consent_language::<&str>(LanguagePref::System, &[], Some("pt-BR")),
            LanguagePref::PtBr
        );
        assert_eq!(
            consent_language::<&str>(LanguagePref::System, &[], None),
            LanguagePref::En
        );
        assert_eq!(
            consent_language(LanguagePref::Fr, &austria, Some("ja")),
            LanguagePref::Fr
        );
        assert_eq!(
            consent_language::<&str>(LanguagePref::Fr, &[], Some("ja")),
            LanguagePref::Fr
        );
    }

    /// Each class that draws nothing | moves text is shown as its code; a plain space stays.
    #[test]
    fn invisible_and_separating_characters_are_shown_as_codes() {
        for (c, code) in [
            ('\u{2028}', r"\u{2028}"),
            ('\u{2029}', r"\u{2029}"),
            ('\u{200e}', r"\u{200e}"),
            ('\u{200f}', r"\u{200f}"),
            ('\u{61c}', r"\u{61c}"),
            ('\u{200b}', r"\u{200b}"),
            ('\u{200d}', r"\u{200d}"),
            ('\u{2060}', r"\u{2060}"),
            ('\u{feff}', r"\u{feff}"),
            ('\u{ad}', r"\u{ad}"),
            ('\u{a0}', r"\u{a0}"),
            ('\u{3000}', r"\u{3000}"),
            ('\u{85}', r"\u{85}"),
            ('\t', r"\u{9}"),
        ] {
            assert_eq!(shown(&format!("a{c}b"), MAX_SHOWN), format!("a{code}b"));
        }
        assert_eq!(shown("a b", MAX_SHOWN), "a b");
    }

    /// An argument | a variable is cut after its first characters; the program's path is never cut.
    #[test]
    fn long_arguments_are_cut_and_variables_shown() {
        let long = "x".repeat(MAX_SHOWN + 50);
        let program = format!("/opt/{}", "p".repeat(MAX_SHOWN));
        let text = message(
            &ENGLISH,
            &call(
                &program,
                &[&long],
                &[("NODE_OPTIONS", "--require\u{200b}x")],
            ),
        );
        let command = text.split("\n\n").nth(1).unwrap();
        let lines: Vec<&str> = command.lines().collect();
        assert_eq!(lines[0], program);
        assert_eq!(lines[1], format!("{}…", "x".repeat(MAX_SHOWN)));
        assert_eq!(lines[2], r"NODE_OPTIONS=--require\u{200b}x");
        // escapes count toward the cap and are never cut in half
        let hidden = shown(&"\u{200b}".repeat(MAX_SHOWN), MAX_SHOWN);
        assert!(hidden.ends_with("}…"), "{hidden}");
        assert!(hidden.chars().count() <= MAX_SHOWN + 1);
        // no arguments, no variables: no line for them
        let plain = message(&ENGLISH, &call("/opt/a", &[], &[]));
        assert!(plain.contains("\n\n/opt/a\n\n"), "{plain}");
    }
}
