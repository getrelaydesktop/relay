//! Lifetimes for disposable media work. Cancelling a view invalidates both
//! running jobs and results already waiting in the UI mailbox.
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Default)]
struct Epoch {
    value: AtomicU64,
    changed: tokio::sync::Notify,
}

#[derive(Debug, Clone, Default)]
pub struct Scope(Arc<Epoch>);

#[derive(Debug, Clone)]
pub struct Ticket {
    epoch: Arc<Epoch>,
    value: u64,
}

impl Scope {
    pub fn current(&self) -> Ticket {
        Ticket {
            epoch: self.0.clone(),
            value: self.0.value.load(Ordering::Acquire),
        }
    }
    pub fn next(&self) -> Ticket {
        self.cancel();
        self.current()
    }
    pub fn cancel(&self) {
        self.0.value.fetch_add(1, Ordering::AcqRel);
        self.0.changed.notify_waiters();
    }
}

impl Ticket {
    pub fn is_current(&self) -> bool {
        self.value == self.epoch.value.load(Ordering::Acquire)
    }
    async fn cancelled(&self) {
        loop {
            let changed = self.epoch.changed.notified();
            tokio::pin!(changed);
            // Register before checking, so cancellation cannot be lost between
            // the atomic read and awaiting the notification.
            changed.as_mut().enable();
            if !self.is_current() {
                return;
            }
            changed.await;
        }
    }
    pub async fn run<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.cancelled() => None,
            result = work => self.is_current().then_some(result),
        }
    }
}

/// Immutable, Send + Sync RGBA storage prepared by a media worker. Turning
/// this into a Slint Image on the UI thread only clones a reference, not pixels.
#[derive(Debug, Clone)]
pub struct Pixels(slint::SharedPixelBuffer<slint::Rgba8Pixel>);
impl From<image::RgbaImage> for Pixels {
    fn from(pixels: image::RgbaImage) -> Self {
        Self(slint::SharedPixelBuffer::clone_from_slice(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
        ))
    }
}
impl Pixels {
    pub fn len(&self) -> usize {
        self.0.as_bytes().len()
    }
    pub fn width(&self) -> u32 {
        self.0.width()
    }
    pub fn height(&self) -> u32 {
        self.0.height()
    }
    pub fn image(&self) -> slint::Image {
        slint::Image::from_rgba8(self.0.clone())
    }
}

pub const MEDIA_MAILBOX_BYTES: u32 = 32 * 1024 * 1024;
pub const DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
// Hold this permit inside the blocking closure, not the async caller: dropping
// a timed-out future does not stop an already-running image decoder.
pub static DECODE_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub async fn decode_work<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let permit = DECODE_SLOTS.acquire().await?;
    Ok(tokio::task::spawn_blocking(move || {
        // Cancellation of the async caller does not release a running decoder.
        let _permit = permit;
        work()
    }).await?)
}

pub fn rgba_fits(width: u32, height: u32) -> bool {
    width != 0
        && height != 0
        && u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| n <= DECODED_IMAGE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_reject_results_from_closed_and_reopened_views() {
        let scope = Scope::default();
        let first = scope.next();
        assert!(first.is_current());
        scope.cancel();
        assert!(!first.is_current());
        let reopened = scope.next();
        assert!(reopened.is_current());
        assert!(!first.is_current());
    }
    #[test]
    fn rgba_budget_checks_output_size_not_compressed_or_grayscale_size() {
        assert!(rgba_fits(4096, 4096));
        assert!(!rgba_fits(8192, 8192));
        assert!(!rgba_fits(u32::MAX, u32::MAX));
        assert!(!rgba_fits(0, 1));
    }
    #[test]
    fn prepared_pixels_share_storage_with_the_ui_image() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Pixels>();
        let pixels: Pixels = image::RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 30, 255])).into();
        let clone = pixels.clone();
        let ui_buffer = pixels.image().to_rgba8().unwrap();
        assert_eq!(pixels.0.as_slice().as_ptr(), clone.0.as_slice().as_ptr());
        assert_eq!(pixels.0.as_slice().as_ptr(), ui_buffer.as_slice().as_ptr());
        assert_eq!(pixels.len(), 8 * 4 * 4);
    }

    #[tokio::test]
    async fn cancelling_a_blocking_decode_keeps_its_slot_until_the_worker_finishes() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finish, released) = std::sync::mpsc::channel();
        let caller = tokio::spawn(decode_work(move || {
            let _ = started.send(());
            let _ = released.recv();
        }));
        tokio::time::timeout(std::time::Duration::from_secs(2), ready).await.unwrap().unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        let available_while_running = DECODE_SLOTS.available_permits();
        // Always unblock the worker before an assertion could unwind the test.
        finish.send(()).unwrap();
        assert_eq!(available_while_running, 0);
        let permit = tokio::time::timeout(std::time::Duration::from_secs(2), DECODE_SLOTS.acquire())
            .await.unwrap().unwrap();
        drop(permit);
    }

    #[tokio::test]
    async fn cancelling_a_scope_interrupts_waiting_jobs() {
        let scope = Scope::default();
        let ticket = scope.next();
        let other = scope.clone();
        let (_, result) = tokio::join!(
            async move {
                tokio::task::yield_now().await;
                other.cancel();
            },
            ticket.run(std::future::pending::<()>()),
        );
        assert!(result.is_none());
    }
}
