//! Code that runs on the Tokio worker threads.
//!
//! Nothing here calls Node-API. The only link back to the JavaScript thread
//! is the TSFN call in `CompletionQueue::push`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use napi::Env;
use napi::bindgen_prelude::ToNapiValue;
use napi::threadsafe_function::ThreadsafeFunctionCallMode;

use super::js_thread;
use super::shared::{BoxedTask, BridgedTask, CompletionQueue, DeferredPtr};
use crate::errors::ConvertedResult;

/// A future, the slot for its output, and the deferred that the output settles.
pub(super) struct StashingTask<F, T> {
    future: F,
    output: Option<ConvertedResult<T>>,
    deferred: Option<DeferredPtr>,
}

/// Boxes `future` together with the slot for its output.
/// This is the only allocation for each request, besides the Tokio task.
/// The deferred is set later, by `set_deferred`, when the Promise exists.
pub(super) fn stashing_task<F, T>(future: F) -> Pin<Box<StashingTask<F, T>>> {
    Box::pin(StashingTask {
        future,
        output: None,
        deferred: None,
    })
}

impl<F, T> StashingTask<F, T> {
    /// Runs on: the JavaScript thread, before the task is spawned.
    pub(super) fn set_deferred(self: Pin<&mut Self>, deferred: DeferredPtr) {
        // SAFETY: `deferred` is not pinned, so writing it does not move `future`.
        unsafe { self.get_unchecked_mut() }.deferred = Some(deferred);
    }
}

impl<F, T> BridgedTask for StashingTask<F, T>
where
    F: Future<Output = ConvertedResult<T>> + Send,
    T: ToNapiValue + Send,
{
    fn poll_stash(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // SAFETY: `future` is structurally pinned: it is never moved out of the
        // pinned struct. `output` and `deferred` are not pinned.
        let this = unsafe { self.get_unchecked_mut() };
        if this.output.is_some() {
            return Poll::Ready(());
        }
        // SAFETY: See above. `this` comes from a pinned reference.
        let future = unsafe { Pin::new_unchecked(&mut this.future) };
        match future.poll(cx) {
            Poll::Ready(output) => {
                this.output = Some(output);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }

    unsafe fn settle(self: Pin<&mut Self>, env: Env) {
        // SAFETY: Only the fields that are not pinned are moved out.
        let this = unsafe { self.get_unchecked_mut() };
        let (Some(output), Some(deferred)) = (this.output.take(), this.deferred.take()) else {
            // Only a task that completed is pushed to the queue, and each
            // task is settled once.
            unreachable!("Settling a bridged task that did not complete");
        };
        // SAFETY: The caller guarantees the thread and the environment.
        unsafe { js_thread::settle(env, deferred, output) }
    }
}

/// The body of each task that `FutureRegistry::spawn` starts.
///
/// Polls the boxed task until its output is in the slot, and then pushes
/// the same box to the queue. No other allocation happens on the worker.
pub(super) fn run_task(task: BoxedTask, queue: Arc<CompletionQueue>) -> RunTask {
    RunTask {
        task: Some(task),
        queue,
    }
}

pub(super) struct RunTask {
    task: Option<BoxedTask>,
    queue: Arc<CompletionQueue>,
}

impl Future for RunTask {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let task = self.task.as_mut().expect("RunTask polled after completion");
        match task.as_mut().poll_stash(cx) {
            Poll::Ready(()) => {
                let task = self.task.take().unwrap();
                self.queue.push(task);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl CompletionQueue {
    /// Called when a future completes.
    /// Only the first push after a drain calls the TSFN.
    fn push(&self, task: BoxedTask) {
        self.completed.lock().unwrap().push(task);
        if !self.signaled.swap(true, Ordering::AcqRel) {
            let guard = self.tsfn.lock().unwrap();
            if let Some(ref tsfn) = *guard {
                tsfn.call((), ThreadsafeFunctionCallMode::NonBlocking);
            } // Else branches can happen only during shutdown
        }
    }
}
