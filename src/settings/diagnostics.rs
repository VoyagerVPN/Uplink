use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_BYTES: u64 = 512 * 1024;

pub(super) fn append(log: &Path, lines: &str) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(log.with_extension("log.lock"))?;
    lock.lock()?;
    if fs::metadata(log).is_ok_and(|m| m.len().saturating_add(lines.len() as u64) > LOG_BYTES) {
        fs::rename(log, log.with_extension("previous.log"))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(lines.as_bytes())
}

pub(crate) fn failure(
    path: &Path,
    operation: &'static str,
    fields: std::fmt::Arguments<'_>,
    error: &io::Error,
) {
    let kind = error.kind();
    let win32 = error
        .raw_os_error()
        .map(|code| format!(" win32={code}"))
        .unwrap_or_default();
    // Сообщение ошибки может содержать данные профиля или ответ сервера.
    let reason = crate::i18n::diagnostic_reason(&error.to_string())
        .map(|reason| format!(" reason={reason}"))
        .unwrap_or_default();
    record(
        path,
        format_args!("operation={operation}{fields} result=failed kind={kind:?}{win32}{reason}"),
    );
}

pub(super) fn record(path: &Path, fields: std::fmt::Arguments<'_>) {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let _ = append(
        &path.with_file_name("transport.log"),
        &format!("{unix_ms} {fields}\n"),
    );
}

#[cfg(test)]
#[test]
fn rotation_keeps_only_the_latest_complete_previous_log() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("transport.log");
    let previous = dir.path().join("transport.previous.log");
    let first = format!("{}\n", "1".repeat(LOG_BYTES as usize - 1));
    let second = format!("{}\n", "2".repeat(LOG_BYTES as usize - 1));
    append(&log, &first).unwrap();
    append(&log, &second).unwrap();
    assert_eq!(fs::read_to_string(&previous).unwrap(), first);
    append(&log, "third\n").unwrap();
    assert_eq!(fs::read_to_string(&previous).unwrap(), second);
    assert_eq!(fs::read_to_string(&log).unwrap(), "third\n");
}
