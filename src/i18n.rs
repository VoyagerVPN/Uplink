include!(concat!(env!("OUT_DIR"), "/languages.rs"));

pub(crate) fn diagnostic_reason(source: &str) -> Option<&str> {
    known_source(source).then_some(source)
}

pub fn error_message<'a>(code: &str, source: &'a str) -> &'a str {
    if known_source(source) {
        message(code, source)
    } else {
        message(code, "Could not complete this action.")
    }
}

pub fn supports(code: &str) -> bool {
    LANGUAGES.iter().any(|&(id, _)| id == code)
}

#[cfg(test)]
#[test]
fn ui_errors_show_a_localized_message_for_known_and_external_failures() {
    assert_eq!(
        error_message("ru", "This setting is unavailable on this instance"),
        "Сервер не поддерживает эту настройку."
    );
    assert_eq!(
        error_message("ru", "unrecognized upstream diagnostic"),
        "Не удалось выполнить действие."
    );
    assert_eq!(
        error_message("en", "Waiting for virtual IP"),
        "Waiting for virtual IP"
    );
}

pub fn current() -> String {
    crate::settings::path()
        .and_then(|p| crate::settings::language(&p))
        .unwrap_or_else(|_| "ru".into())
}

pub fn launch_error(error: &dyn std::fmt::Display) -> Vec<u16> {
    let code = current();
    let title = message(&code, "Cannot open Uplink.");
    let detail = error.to_string();
    format!("{title}\n{}\0", error_message(&code, &detail))
        .encode_utf16()
        .collect()
}
