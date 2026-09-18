use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
static LOG: OnceLock<Mutex<std::path::PathBuf>> = OnceLock::new();

pub(crate) fn initialize(name: &str) {
    let path = match std::env::current_exe() {
        Ok(path) => path.with_file_name(format!("{name}.log")),
        Err(error) => {
            eprintln!("Cannot locate diagnostic log: {error}");
            return;
        }
    };
    if LOG.set(Mutex::new(path)).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // A panic inside logging must not wait on its own lock.
        record(
            "PANIC",
            &format!("{panic}\n{}", Backtrace::force_capture()),
            true,
        );
        previous(panic);
    }));
    record(
        "START",
        &format!(
            "Winderust {} ({}, {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        false,
    );
}

pub(crate) fn error(message: &str) {
    record("ERROR", message, false);
}

pub(crate) fn event(message: &str) {
    record("INFO", message, false);
}

fn record(level: &str, message: &str, panicking: bool) {
    let Some(log) = LOG.get() else { return };
    let path = if panicking {
        match log.try_lock() {
            Ok(path) => path,
            Err(_) => {
                eprintln!("Diagnostic log busy: {message}");
                return;
            }
        }
    } else {
        match log.lock() {
            Ok(path) => path,
            Err(poisoned) => poisoned.into_inner(),
        }
    };
    let header = format!(
        "{} [{level}] pid={} ",
        chrono::Utc::now().to_rfc3339(),
        std::process::id()
    );
    if let Err(error) = append_record(&path, &header, message) {
        eprintln!("Cannot write diagnostic log {}: {error}", path.display());
    }
}

fn append_record(path: &Path, header: &str, message: &str) -> io::Result<()> {
    let mut end = message.len().min(MAX_RECORD_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    let suffix = if end < message.len() {
        "\n[truncated]\n"
    } else {
        "\n"
    };
    let record = format!("{header}{}{suffix}", &message[..end]);
    let size = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if size + record.len() as u64 > MAX_LOG_BYTES {
        let backup = path.with_extension("previous.log");
        match fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(path, backup)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(record.as_bytes())?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_hook_persists_a_report_in_a_disposable_test_process() {
        const CHILD: &str = "WINDERUST_DIAGNOSTIC_TEST";
        if let Ok(name) = std::env::var(CHILD) {
            initialize(&name);
            panic!("diagnostic panic fixture");
        }
        let name = format!("winderust-panic-test-{}", std::process::id());
        let executable = std::env::current_exe().unwrap();
        let output = std::process::Command::new(&executable)
            .args(["--exact", "backend::diagnostics::tests::panic_hook_persists_a_report_in_a_disposable_test_process", "--nocapture"])
            .env(CHILD, &name)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let path = executable.with_file_name(format!("{name}.log"));
        let log = fs::read_to_string(&path).unwrap();
        assert!(log.contains(env!("CARGO_PKG_VERSION")));
        assert!(log.contains("[PANIC]"));
        assert!(log.contains("diagnostic panic fixture"));
        assert!(log.contains("diagnostics.rs"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn diagnostics_bound_unicode_records_and_retain_one_backup() {
        let dir =
            std::env::temp_dir().join(format!("winderust-diagnostics-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.log");
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        let backup = path.with_extension("previous.log");
        fs::write(&backup, "old backup").unwrap();
        append_record(&path, "ERROR ", &"\u{1f980}".repeat(MAX_RECORD_BYTES)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("ERROR "));
        assert!(text.ends_with("[truncated]\n"));
        assert!(text.len() < MAX_RECORD_BYTES + 100);
        assert_eq!(fs::metadata(&backup).unwrap().len(), MAX_LOG_BYTES);
        append_record(&path, "INFO ", "next entry").unwrap();
        assert!(fs::read_to_string(&path)
            .unwrap()
            .ends_with("INFO next entry\n"));
        fs::remove_file(path).unwrap();
        fs::remove_file(backup).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
