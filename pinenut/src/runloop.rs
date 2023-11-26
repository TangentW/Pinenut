//! An event loop implementation.

use std::{any::Any, sync::mpsc, thread, thread::JoinHandle};

use thiserror::Error;

/// The runloop error.
///
/// When this error is occurred, either the runloop is actively stopped or the
/// thread is panicked.
#[derive(Error, Debug)]
#[error("sending event on a stopped runloop")]
pub struct Error;

/// A handler for handling received events in a runloop.
pub(crate) trait Handle {
    type Event;

    /// Handles the received event.
    fn handle(&mut self, event: Self::Event, context: &mut Context);

    /// Starts a new associated runloop.
    #[inline]
    fn run(self) -> Runloop<Self::Event>
    where
        Self: Sized + Send + 'static,
        Self::Event: Send + 'static,
    {
        Runloop::run(self)
    }
}

/// A context that is passed during the runloop run.
#[derive(Debug)]
pub(crate) struct Context {
    is_stopped: bool,
}

impl Context {
    #[inline]
    fn new() -> Self {
        Self { is_stopped: false }
    }

    /// Stop the runloop.
    #[inline]
    pub(crate) fn stop(&mut self) {
        self.is_stopped = true;
    }

    /// Whether the runloop has stopped.
    #[inline]
    pub(crate) fn is_stopped(&self) -> bool {
        self.is_stopped
    }
}

pub(crate) struct Runloop<Event> {
    sender: mpsc::Sender<Event>,
    thread_handle: JoinHandle<()>,
}

impl<Event> Runloop<Event>
where
    Event: Send + 'static,
{
    /// Starts a new runloop with handler.
    pub(crate) fn run<H>(mut handler: H) -> Self
    where
        H: Handle<Event = Event> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();

        let thread_handle = thread::spawn(move || {
            let mut context = Context::new();
            while !context.is_stopped() && let Ok(event) = receiver.recv() {
                handler.handle(event, &mut context);
            }
        });

        Self { sender, thread_handle }
    }

    /// Sends an event to the runloop.
    ///
    /// When the runloop has stopped, it returns [`Err`].
    #[inline]
    pub(crate) fn on(&self, event: Event) -> Result<(), Error> {
        self.sender.send(event).map_err(|_| Error)
    }

    /// Waits for the runloop to finish.
    ///
    /// If the associated thread in runloop panics, [`Err`] is returned with the
    /// parameter given to [`panic!`].
    #[inline]
    pub(crate) fn join(self) -> Result<(), Box<dyn Any + Send + 'static>> {
        self.thread_handle.join()
    }
}
