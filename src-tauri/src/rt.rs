//! Waiting on async work from blocking code, safely from any thread.
//!
//! Since 0.2.3 most commands run on the "jelly-worker" runtime instead of
//! the main thread (lib.rs). A command that then waits with a plain
//! `tauri::async_runtime::block_on` (a model call, the vector index, a
//! Google request) panics on that thread: "Cannot start a runtime from
//! within a runtime". The command never returns, so its screen waits
//! forever (2026-10-22: "Writing…" stuck on a meeting's notes for five
//! minutes, three presses, three panics).
//!
//! On a runtime worker this hands the thread to blocking work first
//! (`block_in_place`), so the wait is allowed and the runtime keeps its
//! other tasks moving; anywhere else (the main thread, `spawn_blocking`,
//! a plain thread) it is the same `block_on` as before.

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
