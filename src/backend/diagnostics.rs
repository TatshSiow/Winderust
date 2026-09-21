use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{
        mpsc::{self, SyncSender},
        OnceLock,
    },
    time::Duration,
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const QUEUE_CAPACITY: usize = 32;
const FLUSH_TIMEOUT: Duration = Duration::from_millis(200);
static LOG: OnceLock<SyncSender<Command>> = OnceLock::new();

enum Command {
    Record(String, String),
    Flush(SyncSender<()>),
}

pub(crate) fn initialize(name: &str) {
    let path = match std::env::current_exe() {
        Ok(path) => path.with_file_name(format!("{name}.log")),
        Err(error) => {
            eprintln!("Cannot locate diagnostic log: {error}");
            return;
        }
    };
    let writer = match start_writer(move |header, message| append_record(&path, header, message)) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("Cannot start diagnostic writer: {error}");
            return;
        }
    };
    if LOG.set(writer).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        record("PANIC", &format!("{panic}\n{}", Backtrace::force_capture()));
        // Best effort only: a stalled writer must not prevent panic handling.
        finish();
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
    );
}

fn start_writer(
    mut write: impl FnMut(&str, &str) -> io::Result<()> + Send + 'static,
) -> io::Result<SyncSender<Command>> {
    let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
    std::thread::Builder::new()
        .name("winderust-diagnostics".into())
        .spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    Command::Record(header, message) => {
                        if let Err(error) = write(&header, &message) {
                            eprintln!("Cannot write diagnostic log: {error}");
                        }
                    }
                    Command::Flush(done) => {
                        let _ = done.try_send(());
                    }
                }
            }
        })?;
    Ok(sender)
}

pub(crate) fn error(message: &str) {
    record("ERROR", message);
}
pub(crate) fn event(message: &str) {
    record("INFO", message);
}

fn record(level: &str, message: &str) {
    let Some(log) = LOG.get() else { return };
    let header = format!(
        "{} [{level}] pid={} ",
        chrono::Utc::now().to_rfc3339(),
        std::process::id()
    );
    let mut end = message.len().min(MAX_RECORD_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    let mut message_copy = message[..end].to_owned();
    if end < message.len() {
        message_copy.push_str("\n[truncated]");
    }
    // Drop new records when full/disconnected; never wait for capacity or disk I/O.
    let _ = log.try_send(Command::Record(header, message_copy));
}

/// Called only at process exit or panic, never on recovery/UI submission paths.
/// No writer join: disk stalls must not hold the process open indefinitely.
pub(crate) fn finish() {
    if let Some(log) = LOG.get() {
        flush(log);
    }
}

fn flush(log: &SyncSender<Command>) {
    let (done, received) = mpsc::sync_channel(1);
    if log.try_send(Command::Flush(done)).is_ok() {
        let _ = received.recv_timeout(FLUSH_TIMEOUT);
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
    fn stalled_writer_keeps_submission_bounded_and_flush_has_a_deadline() {
        let (started, observed) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let writer = start_writer(move |_, _| {
            let _ = started.try_send(());
            let _ = blocked.recv_timeout(Duration::from_secs(5));
            Ok(())
        })
        .unwrap();
        writer
            .try_send(Command::Record(String::new(), "first".into()))
            .unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        // Even a queued flush returns while the sink remains blocked.
        let before = std::time::Instant::now();
        flush(&writer);
        assert!(before.elapsed() < Duration::from_secs(2));
        for _ in 1..QUEUE_CAPACITY {
            writer
                .try_send(Command::Record(String::new(), "queued".into()))
                .unwrap();
        }
        assert!(matches!(
            writer.try_send(Command::Record(String::new(), "overflow".into())),
            Err(mpsc::TrySendError::Full(_))
        ));
        flush(&writer); // A full queue is also nonblocking.
        release.send(()).unwrap();
        drop(release);
        drop(writer);
    }

    #[test]
    fn write_failures_do_not_prevent_later_records_or_flush() {
        let (seen, records) = mpsc::channel();
        let writer = start_writer(move |_, message| {
            seen.send(message.to_owned()).unwrap();
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture"))
        })
        .unwrap();
        for message in ["first", "second"] {
            writer
                .try_send(Command::Record(String::new(), message.into()))
                .unwrap();
        }
        flush(&writer);
        assert_eq!(
            records.recv_timeout(Duration::from_secs(2)).unwrap(),
            "first"
        );
        assert_eq!(
            records.recv_timeout(Duration::from_secs(2)).unwrap(),
            "second"
        );
    }

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
