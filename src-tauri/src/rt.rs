//! Waiting on async work from blocking code, safely from any thread.
//!
//! On a multi-thread runtime worker (where `#[tauri::command(async)]`
//! runs), a plain `block_on` panics with "Cannot start a runtime from
//! within a runtime". There this hands the thread to blocking work first
//! (`block_in_place`); anywhere else it is the same `block_on`.

use std::future::Future;

pub fn block_on<F: Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(|| h.block_on(fut)),
        _ => tauri::async_runtime::block_on(fut),
    }
}

#[cfg(test)]
mod tests {
    /// The exact failure: a blocking wait inside a task on a multi-thread
    /// runtime, as every `#[tauri::command(async)]` runs.
    #[test]
    fn waiting_inside_a_runtime_task_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let got = rt.block_on(async { tokio::spawn(async { super::block_on(async { 41 + 1 }) }).await.unwrap() });
        assert_eq!(got, 42);
        // And from a spawn_blocking thread, and from a plain thread.
        let got = rt.block_on(async { tokio::task::spawn_blocking(|| super::block_on(async { 7 })).await.unwrap() });
        assert_eq!(got, 7);
        assert_eq!(std::thread::spawn(|| super::block_on(async { 3 })).join().unwrap(), 3);
    }
}
