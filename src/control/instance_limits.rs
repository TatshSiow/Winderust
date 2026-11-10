//! Read-only Instance Limits acquisition. Unknown protected targets are never absence evidence.
use std::path::PathBuf;

use crate::{
    foreground::{
        ensure_process_action_target_access_on_handle, ProcessActionAccess, ProcessActionTarget,
    },
    platform::windows::{instance_limits as native, process::current_process_id},
};

pub(crate) use native::{paths_equal, validate_path, CountObservation};

pub(crate) fn collect(paths: &[String]) -> Vec<CountObservation> {
    native::collect(paths, |id, path, creation_time, process| {
        if id == current_process_id() || crate::crash_recovery::is_watchdog_process(id) {
            return Err("Winderust and its recovery helper cannot be monitored.".into());
        }
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "The process image name is unavailable.".to_owned())?;
        let target = ProcessActionTarget {
            id,
            name: name.to_owned(),
            executable_path: path,
            creation_time,
            session_id: None,
            is_service_account: None,
        };
        ensure_process_action_target_access_on_handle(
            &target,
            ProcessActionAccess::SafetyOnly,
            process,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, Write},
        os::windows::process::CommandExt,
        path::Path,
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    #[ignore = "Owned fixture entry point; run through native_counts_only_the_selected_image_path"]
    fn native_worker() {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            let _ = send.send(());
        });
        let _ = receive.recv_timeout(Duration::from_secs(30));
    }

    struct Worker(Child);

    impl Worker {
        fn start(path: &Path) -> Self {
            Self(
                Command::new(path)
                    .args([
                        "--ignored",
                        "--exact",
                        "control::instance_limits::tests::native_worker",
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(0x0800_0000)
                    .spawn()
                    .unwrap(),
            )
        }
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            if let Some(mut input) = self.0.stdin.take() {
                let _ = input.write_all(b"\n");
            }
            self.0.wait().unwrap();
        }
    }

    fn await_count(path: &str, expected: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let protected = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("csrss.exe")
            .to_str()
            .unwrap()
            .to_owned();
        loop {
            let result = collect(&[path.into(), protected.clone()]);
            if result[0] == CountObservation::Complete(expected) {
                assert!(
                    matches!(result[1], CountObservation::Incomplete { .. }),
                    "Protected matching targets cannot prove absence: {result:?}"
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Expected {expected}, got {result:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    #[ignore = "Starts bounded owned worker fixtures; requires ordinary host process access"]
    fn native_counts_only_the_selected_image_path() {
        let directory = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("instance-limits-native-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let other_directory = directory.join("other");
        std::fs::create_dir(&other_directory).unwrap();
        let selected = directory.join("worker \u{00e9} \u{6d4b}\u{8bd5}.exe");
        let named_alias = directory.join("other worker.exe");
        let other_path = other_directory.join(selected.file_name().unwrap());
        let source = std::env::current_exe().unwrap();
        for path in [&selected, &named_alias, &other_path] {
            std::fs::hard_link(&source, path).unwrap();
        }
        let target = selected.to_str().unwrap();
        let negative_name = Worker::start(&named_alias);
        let negative_directory = Worker::start(&other_path);
        await_count(target, 0);
        let first = Worker::start(&selected);
        await_count(target, 1);
        let second = Worker::start(&selected);
        await_count(target, 2);
        drop(first);
        await_count(target, 1);
        drop(second);
        await_count(target, 0);
        drop((negative_name, negative_directory));
        for path in [&selected, &named_alias, &other_path] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(other_directory).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
