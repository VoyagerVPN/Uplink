use super::JoinStage;
use std::{
    cell::{Cell, RefCell},
    fmt, fs,
    io::{self, Write},
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Attempt {
    file: fs::File,
    start: Instant,
    operation: &'static str,
}

thread_local! {
    // Привязка к потоку попытки исключает запросы фоновой синхронизации из её журнала.
    static ACTIVE: RefCell<Option<Attempt>> = const { RefCell::new(None) };
}

impl Attempt {
    fn open(prefs: &Path) -> Option<Self> {
        let parent = prefs.parent()?;
        fs::create_dir_all(parent).ok()?;
        // При одновременном входе из CLI и GUI предыдущий процесс допишет свой отдельный файл.
        let file = tempfile::NamedTempFile::new_in(parent)
            .ok()?
            .persist(parent.join("last-join.log"))
            .ok()?;
        Some(Self {
            file,
            start: Instant::now(),
            operation: "start",
        })
    }

    fn write(&mut self, fields: fmt::Arguments<'_>) {
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            self.file,
            "{unix_ms} elapsed_ms={} {fields}",
            self.start.elapsed().as_millis()
        );
    }
}

pub(crate) fn operation(name: &'static str) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(log) = slot
            && log.operation != name
        {
            log.operation = name;
            log.write(format_args!("operation={name}"));
        }
    });
}

pub(crate) fn number(field: &'static str, value: i64) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(log) = slot {
            let op = log.operation;
            log.write(format_args!("operation={op} {field}={value}"));
        }
    });
}

fn failure(event: &'static str, error: &io::Error) {
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(log) = slot {
            let op = log.operation;
            let kind = error.kind();
            let code = error
                .raw_os_error()
                .map(|n| format!(" win32={n}"))
                .unwrap_or_default();
            // Только сообщения из каталога: произвольный текст ошибки может содержать секрет.
            let text = error.to_string();
            let reason = crate::i18n::diagnostic_reason(&text)
                .map(|s| format!(" reason={s}"))
                .unwrap_or_default();
            log.write(format_args!(
                "{event} operation={op} kind={kind:?}{code}{reason}"
            ));
        }
    });
}

pub(crate) fn rollback(cleanup: impl FnOnce() -> io::Result<()>) {
    let previous = ACTIVE.with_borrow(|slot| slot.as_ref().map(|log| log.operation));
    if let Err(error) = cleanup() {
        failure("cleanup=failed", &error);
    }
    if let Some(name) = previous {
        operation(name);
    }
}

pub(crate) fn run(
    path: &Path,
    method: &'static str,
    progress: &dyn Fn(JoinStage),
    join: impl FnOnce(&dyn Fn(JoinStage)) -> io::Result<()>,
) -> io::Result<()> {
    struct Restore(Option<Attempt>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.replace(self.0.take());
        }
    }
    let _guard = Restore(ACTIVE.replace(Attempt::open(path)));
    ACTIVE.with_borrow_mut(|slot| {
        if let Some(log) = slot {
            log.write(format_args!(
                "version={} method={method}",
                env!("CARGO_PKG_VERSION")
            ));
        }
    });
    let last = Cell::new(0);
    let result = join(&|stage| {
        if last.replace(stage as i32) == stage as i32 {
            return;
        }
        let name = match stage {
            JoinStage::Access => "access",
            JoinStage::Device => "device",
            JoinStage::Approval => "approval",
            JoinStage::Address => "address",
        };
        ACTIVE.with_borrow_mut(|slot| {
            if let Some(log) = slot {
                log.write(format_args!("stage={name}"));
            }
        });
        progress(stage);
    });
    match &result {
        Err(error) => failure("result=failed", error),
        Ok(()) => ACTIVE.with_borrow_mut(|slot| {
            if let Some(log) = slot {
                log.write(format_args!("result=ok"));
            }
        }),
    }
    result
}
