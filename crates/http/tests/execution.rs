//! Image admission survives cancellation without blocking ordinary text work.

#![expect(
    clippy::missing_panics_doc,
    reason = "Tests report failure by panicking."
)]

use std::sync::mpsc;
use std::time::Duration;

use eliza_http::execution::image_work;

#[test]
fn canceled_jobs_retain_capacity_until_workers_finish() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut jobs = Vec::new();
            let mut releases = Vec::new();
            for _ in 0..2 {
                let (started, ready) = tokio::sync::oneshot::channel();
                let (release, wait) = mpsc::channel();
                jobs.push(tokio::spawn(image_work(true, move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                })));
                releases.push(release);
                ready.await.unwrap();
            }
            jobs[0].abort();
            assert!(image_work(true, || ()).await.is_err());
            assert_eq!(image_work(false, || 42).await.unwrap(), 42);
            for release in releases {
                release.send(()).unwrap();
            }
            for job in jobs {
                let _ = job.await;
            }
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if image_work(true, || ()).await.is_ok() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        });
}
