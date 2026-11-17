//! Read-only Instance Limits acquisition. Unknown protected targets are never absence evidence.
use std::path::PathBuf;

use crate::{
    foreground::{
        ensure_process_action_target_access_on_handle, ProcessActionAccess, ProcessActionTarget,
    },
    platform::windows::{instance_limits as native, process::current_process_id},
};

pub(crate) use native::{
    member_state, paths_equal, validate_path, CountObservation, Member, MemberState,
    MembershipObservation,
};

pub(crate) fn collect(paths: &[String]) -> Vec<CountObservation> {
    collect_members(paths)
        .iter()
        .map(MembershipObservation::count)
        .collect()
}

/// Captures read-only exact members; callers must acquire fresh mutation authority separately.
pub(crate) fn collect_members(paths: &[String]) -> Vec<MembershipObservation> {
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

    fn await_members(path: &str, expected: &[u32]) -> Vec<Member> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let protected = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("csrss.exe")
            .to_str()
            .unwrap()
            .to_owned();
        loop {
            let mut result = collect_members(&[path.into(), protected.clone()]);
            let counts: Vec<_> = result.iter().map(MembershipObservation::count).collect();
            if counts[0] == CountObservation::Complete(expected.len() as u32) {
                assert!(
                    matches!(counts[1], CountObservation::Incomplete { .. }),
                    "Protected matching targets cannot prove absence: {counts:?}"
                );
                let MembershipObservation::Complete(members) = result.remove(0) else {
                    panic!("Complete counts must retain complete members")
                };
                let actual: std::collections::BTreeSet<_> =
                    members.iter().map(|member| member.id).collect();
                assert_eq!(actual, expected.iter().copied().collect());
                for member in &members {
                    assert_eq!(member_state(member), Ok(MemberState::Current));
                    assert!(native::paths_equal(&member.path, path).unwrap());
                    assert_eq!(
                        member.process.process_creation_time(),
                        Some(member.creation_time)
                    );
                }
                return members;
            }
            assert!(
                Instant::now() < deadline,
                "Expected {} members, got {counts:?}",
                expected.len()
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
        await_members(target, &[]);
        let first = Worker::start(&selected);
        let retained_first = await_members(target, &[first.0.id()]);
        let second = Worker::start(&selected);
        await_members(target, &[first.0.id(), second.0.id()]);
        drop(first);
        assert_eq!(member_state(&retained_first[0]), Ok(MemberState::Exited));
        drop(retained_first);
        await_members(target, &[second.0.id()]);
        drop(second);
        await_members(target, &[]);
        drop((negative_name, negative_directory));
        for path in [&selected, &named_alias, &other_path] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(other_directory).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
