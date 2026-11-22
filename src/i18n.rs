include!(concat!(env!("OUT_DIR"), "/languages.rs"));

pub fn supports(code: &str) -> bool {
    LANGUAGES.iter().any(|&(id, _)| id == code)
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
    format!("{title}\n{}\0", message(&code, &detail))
        .encode_utf16()
        .collect()
}
