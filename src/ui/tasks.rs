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
