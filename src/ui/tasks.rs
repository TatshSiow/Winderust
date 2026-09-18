use iced::Task;

pub(super) fn run<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Task<Result<T, String>> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    match std::thread::Builder::new()
        .name("winderust-ui-query".into())
        .spawn(move || {
            // Closing the app can drop the receiver while a query completes.
            let _ = sender.send(job());
        }) {
        Ok(_) => Task::perform(
            async move { receiver.await.map_err(|error| error.to_string()) },
            |result| result,
        ),
        Err(error) => Task::done(Err(error.to_string())),
    }
}

#[test]
fn blocking_jobs_leave_the_calling_thread_free() {
    let caller = std::thread::current().id();
    let (started, worker) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let task = run(move || {
        started.send(std::thread::current().id()).unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(5))
    });
    assert_ne!(
        worker
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        caller
    );
    release.send(()).unwrap();
    drop(task);
}
