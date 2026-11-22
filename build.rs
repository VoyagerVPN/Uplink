use std::{env, fmt::Write, fs, path::PathBuf};

// Часть каталога с контекстом native нужна также трею и CLI, где нет окна Slint.
fn build_language_catalog() {
    println!("cargo:rerun-if-changed=lang");
    let mut languages = vec![("en".to_owned(), "English".to_owned())];
    let mut entries = String::new();
    let mut sources = std::collections::BTreeSet::new();
    for dir in fs::read_dir("lang").unwrap() {
        let dir = dir.unwrap();
        if !dir.file_type().unwrap().is_dir() {
            continue;
        }
        let code = dir.file_name().into_string().unwrap();
        let file = dir.path().join("LC_MESSAGES/uplink.po");
        let po = rspolib::pofile(file.to_str().unwrap()).expect("Uplink language catalog");
        let name = po
            .metadata
            .get("X-Display-Name")
            .expect("Missing X-Display-Name");
        languages.push((code.clone(), name.clone()));
        for entry in po.entries {
            if entry.msgctxt.as_deref() == Some("native") && !entry.obsolete {
                if let Some(value) = entry.msgstr.filter(|s| !s.is_empty()) {
                    sources.insert(format!("{:?}", entry.msgid));
                    writeln!(entries, "({code:?}, {:?}) => {value:?},", entry.msgid).unwrap();
                }
            }
        }
    }
    languages.sort_by_key(|(code, _)| (code != "ru", code.clone()));
    let known = sources.into_iter().collect::<Vec<_>>().join(" | ");
    let source = format!(
        "fn known_source(source: &str) -> bool {{ matches!(source, {known}) }}\npub const LANGUAGES: &[(&str, &str)] = &{languages:?};\npub fn message<'a>(code: &str, source: &'a str) -> &'a str {{ match (code, source) {{ {entries} _ => source }} }}"
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("languages.rs"),
        source,
    )
    .unwrap();
}

fn main() {
    build_language_catalog();
    let config = slint_build::CompilerConfiguration::new()
        .with_default_translation_context(slint_build::DefaultTranslationContext::None)
        .with_bundled_translations("lang");
    slint_build::compile_with_config("ui/room.slint", config).expect("Uplink UI");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        for (id, icon) in [
            ("1", "uplink"),
            ("2", "light"),
            ("3", "dark"),
            ("4", "unknown"),
            ("5", "fair"),
            ("6", "poor"),
        ] {
            let path = format!("assets/icon/{icon}.ico");
            println!("cargo:rerun-if-changed={path}");
            resource.set_icon_with_id(&path, id);
        }
        resource.compile().expect("Uplink icons");
    }
}
