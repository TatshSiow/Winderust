//! Typed UI work executed outside the native event loop.
use std::{
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

type Worker = Box<dyn FnOnce() + Send>;
struct Job<M> {
    run: Box<dyn FnOnce() -> M + Send>,
    start_failed: Option<Box<dyn FnOnce(String) -> M + Send>>,
}
#[must_use]
pub(super) struct Task<M> {
    ready: Vec<M>,
    jobs: Vec<Job<M>>,
}
impl<M: Send + 'static> Task<M> {
    pub(super) fn none() -> Self {
        Self {
            ready: Vec::new(),
            jobs: Vec::new(),
        }
    }
    pub(super) fn done(message: M) -> Self {
        Self {
            ready: vec![message],
            jobs: Vec::new(),
        }
    }
    pub(super) fn batch(tasks: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::none();
        for task in tasks {
            result.ready.extend(task.ready);
            result.jobs.extend(task.jobs);
        }
        result
    }
    pub(super) fn map<N: Send + 'static>(
        self,
        map: impl Fn(M) -> N + Send + Sync + 'static,
    ) -> Task<N> {
        let map = Arc::new(map);
        Task {
            ready: self.ready.into_iter().map(|value| map(value)).collect(),
            jobs: self
                .jobs
                .into_iter()
                .map(|job| {
                    let Job { run, start_failed } = job;
                    let run_map = Arc::clone(&map);
                    let start_failed = start_failed.map(|failed| {
                        let map = Arc::clone(&map);
                        Box::new(move |error| map(failed(error)))
                            as Box<dyn FnOnce(String) -> N + Send>
                    });
                    Job {
                        run: Box::new(move || run_map(run())),
                        start_failed,
                    }
                })
                .collect(),
        }
    }
    pub(super) fn perform<T: Send + 'static>(
        future: impl Future<Output = T> + Send + 'static,
        map: impl FnOnce(T) -> M + Send + 'static,
    ) -> Self {
        Self {
            ready: Vec::new(),
            jobs: vec![Job {
                run: Box::new(move || map(block_on(future))),
                start_failed: None,
            }],
        }
    }
    pub(super) fn dispatch(
        self,
        deliver: impl Fn(M) + Send + Sync + 'static,
        failed: impl Fn(String) + Send + Sync + 'static,
    ) {
        self.dispatch_with_spawner(deliver, failed, |worker| {
            std::thread::Builder::new()
                .name("winderust-ui-query".into())
                .spawn(worker)
                .map(|_| ())
        });
    }

    fn dispatch_with_spawner(
        self,
        deliver: impl Fn(M) + Send + Sync + 'static,
        failed: impl Fn(String) + Send + Sync + 'static,
        mut spawn: impl FnMut(Worker) -> std::io::Result<()>,
    ) {
        let deliver = Arc::new(deliver);
        let failed = Arc::new(failed);
        for message in self.ready {
            deliver(message);
        }
        for job in self.jobs {
            let Job { run, start_failed } = job;
            let completed = Arc::clone(&deliver);
            let worker_failed = Arc::clone(&failed);
            if let Err(error) = spawn(Box::new(move || {
                match catch_unwind(AssertUnwindSafe(run)) {
                    Ok(message) => completed(message),
                    Err(_) => worker_failed("A UI query worker panicked.".into()),
                }
            })) {
                let error = format!("Could not start a UI query worker: {error}");
                match start_failed {
                    Some(start_failed) => deliver(start_failed(error)),
                    None => failed(error),
                }
            }
        }
    }
}
pub(super) fn run<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Task<Result<T, String>> {
    Task {
        ready: Vec::new(),
        jobs: vec![Job {
            run: Box::new(move || {
                catch_unwind(AssertUnwindSafe(job))
                    .map_err(|_| "A UI query worker panicked.".to_string())
            }),
            // Deliver the same typed completion as a failed running query, so the
            // owning model clears its pending flag through its normal handler.
            start_failed: Some(Box::new(Err)),
        }],
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocking_jobs_leave_dispatch_thread_free_and_preserve_mapped_results() {
        let caller = std::thread::current().id();
        let (release, wait) = std::sync::mpsc::channel();
        let (done, received) = std::sync::mpsc::channel();
        let task = run(move || {
            let worker = std::thread::current().id();
            wait.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            worker
        })
        .map(|r| r.unwrap());
        task.dispatch(
            move |value| {
                done.send(value).unwrap();
            },
            |error| panic!("{error}"),
        );
        release.send(()).unwrap();
        assert_ne!(
            received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            caller
        );
    }
    #[test]
    fn panicked_jobs_return_a_completion_error() {
        let (done, received) = std::sync::mpsc::channel();
        run(|| panic!("query failure")).dispatch(
            move |value| {
                done.send(value).unwrap();
            },
            |error| panic!("{error}"),
        );
        assert!(received
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .is_err());
    }

    #[test]
    fn query_start_failure_delivers_mapped_error_once_without_running_the_job() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let ran = Arc::new(AtomicBool::new(false));
        let job_ran = Arc::clone(&ran);
        let generic_failures = Arc::new(AtomicUsize::new(0));
        let failures = Arc::clone(&generic_failures);
        let (delivered, received) = std::sync::mpsc::channel();
        run(move || {
            job_ran.store(true, Ordering::Relaxed);
            7_usize
        })
        .map(|result| (1_u8, result))
        .map(|(tag, result)| (tag + 1, result))
        .dispatch_with_spawner(
            move |message| delivered.send(message).unwrap(),
            move |_| {
                failures.fetch_add(1, Ordering::Relaxed);
            },
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "injected worker exhaustion",
                ))
            },
        );

        let (tag, result) = received.try_recv().unwrap();
        assert_eq!(tag, 2);
        assert!(result.unwrap_err().contains("injected worker exhaustion"));
        assert!(received.try_recv().is_err(), "only one completion is sent");
        assert!(!ran.load(Ordering::Relaxed));
        assert_eq!(generic_failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn future_start_failure_uses_the_generic_failure_handler_once() {
        let (delivered, received) = std::sync::mpsc::channel();
        Task::perform(async { 7_u8 }, |value| value).dispatch_with_spawner(
            |_| panic!("a future that could not start cannot complete"),
            move |error| delivered.send(error).unwrap(),
            |_| Err(std::io::Error::other("injected startup failure")),
        );
        assert!(received
            .try_recv()
            .unwrap()
            .contains("injected startup failure"));
        assert!(received.try_recv().is_err());
    }
}
