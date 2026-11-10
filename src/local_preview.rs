//! Bounded local attachment decoding. Install on the UI thread; the parent's
//! image-release callback must call `cancel()` on close, hide, logout, and
//! attachment path/generation changes (including changes during UI debounce).

use crate::{Desktop, details};
use slint::ComponentHandle;
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};

type Preview = Result<Option<crate::media::Pixels>, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    generation: i32,
    path: String,
}

struct UiState {
    request: Request,
    auth_required: bool,
    file_open: bool,
    window_visible: bool,
}

impl UiState {
    fn capture(ui: &Desktop) -> Self {
        Self {
            request: Request {
                generation: ui.get_attachment_generation(),
                path: ui.get_file_path().to_string(),
            },
            auth_required: ui.get_auth_required(),
            file_open: ui.get_file_open(),
            window_visible: ui.get_window_visible(),
        }
    }

    fn accepts(&self, request: &Request) -> bool {
        !self.auth_required
            && self.file_open
            && self.window_visible
            && !request.path.is_empty()
            && self.request == *request
    }
}

struct Job {
    ui: slint::Weak<Desktop>,
    request: Request,
}

// Only the worker can take a job. Its active job includes the UI delivery, so
// even a stalled UI event loop cannot accumulate decoded rasters in callbacks.
struct Queue<T> {
    serial: u64,
    pending: Option<(u64, T)>,
    completed: Option<(u64, Preview)>,
}

impl<T> Queue<T> {
    fn new() -> Self {
        Self {
            serial: 0,
            pending: None,
            completed: None,
        }
    }

    fn cancel(&mut self) {
        self.serial = self
            .serial
            .checked_add(1)
            .expect("preview serial exhausted");
        self.pending = None;
        self.completed = None;
    }

    fn replace(&mut self, job: T) -> u64 {
        self.cancel();
        self.pending = Some((self.serial, job));
        self.serial
    }

    fn complete(&mut self, serial: u64, preview: Preview) -> bool {
        if serial != self.serial {
            return false;
        }
        self.completed = Some((serial, preview));
        true
    }

    fn take_completion(&mut self, serial: u64) -> Option<Preview> {
        if serial != self.serial {
            return None;
        }
        self.completed.take().map(|(_, preview)| preview)
    }
}

struct Worker {
    queue: Mutex<Queue<Job>>,
    ready: Condvar,
}

impl Worker {
    fn next(&self) -> (u64, Job) {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if let Some(job) = queue.pending.take() {
                return job;
            }
            queue = self.ready.wait(queue).unwrap();
        }
    }

    fn run(self: Arc<Self>) {
        loop {
            let (serial, job) = self.next();
            let preview =
                details::file_preview(&job.request.path).map(|preview| preview.map(Into::into));
            if !self.queue.lock().unwrap().complete(serial, preview) {
                continue;
            }
            let worker = Arc::clone(&self);
            let (done, finished) = mpsc::channel();
            let _ = job.ui.upgrade_in_event_loop(move |ui| {
                // Pull pixels from a single mailbox, not from the event-loop
                // closure: cancel/replacement can free them immediately.
                let preview = worker.queue.lock().unwrap().take_completion(serial);
                if let Some(preview) = preview {
                    if UiState::capture(&ui).accepts(&job.request) {
                        // Never hold the queue mutex during UI notifications.
                        match preview {
                            Ok(Some(pixels)) => {
                                ui.set_file_preview(pixels.image());
                                ui.set_has_file_preview(true);
                            }
                            Ok(None) => {
                                ui.set_file_preview(Default::default());
                                ui.set_has_file_preview(false);
                            }
                            Err(error) => {
                                ui.set_file_preview(Default::default());
                                ui.set_has_file_preview(false);
                                ui.set_file_preview_error(error.into());
                            }
                        }
                    }
                }
                let _ = done.send(());
            });
            // If upgrading/enqueuing fails, the dropped sender also wakes us.
            // Do not decode another image until this delivery has been handled.
            let _ = finished.recv();
            self.queue.lock().unwrap().take_completion(serial);
        }
    }
}

static WORKER: OnceLock<Result<Arc<Worker>, String>> = OnceLock::new();

fn worker() -> Result<&'static Arc<Worker>, &'static str> {
    WORKER
        .get_or_init(|| {
            let worker = Arc::new(Worker {
                queue: Mutex::new(Queue::new()),
                ready: Condvar::new(),
            });
            let background = Arc::clone(&worker);
            std::thread::Builder::new()
                .name("local-attachment-preview".into())
                .spawn(move || background.run())
                .map(|_| worker)
                .map_err(|error| format!("Не удалось запустить превью: {error}"))
        })
        .as_ref()
        .map_err(String::as_str)
}

/// Invalidate the active request and drop queued jobs/decoded results. Call
/// from the parent's UI-thread image-release callback before clearing images.
/// A decoder blocked on filesystem IO cannot be interrupted; it remains the
/// sole worker, and its eventual result cannot repopulate the UI.
pub fn cancel() {
    if let Some(Ok(worker)) = WORKER.get() {
        worker.queue.lock().unwrap().cancel();
    }
}

/// Replace main's thread-per-preview callback with this installer.
pub fn install(ui: &Desktop) {
    let weak = ui.as_weak();
    ui.on_preview_file(move || {
        let Some(ui) = weak.upgrade() else { return };
        cancel();
        ui.set_file_preview(Default::default());
        ui.set_has_file_preview(false);
        let state = UiState::capture(&ui);
        if !state.accepts(&state.request) {
            return;
        }
        ui.set_file_preview_error(Default::default());
        match worker() {
            Ok(worker) => {
                worker.queue.lock().unwrap().replace(Job {
                    ui: ui.as_weak(),
                    request: state.request,
                });
                worker.ready.notify_one();
            }
            Err(error) => ui.set_file_preview_error(error.into()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str) -> Request {
        Request {
            generation: 7,
            path: path.into(),
        }
    }

    #[test]
    fn bursts_replace_the_single_pending_job_without_changing_the_active_job() {
        let mut queue = Queue::new();
        let active_serial = queue.replace("active".to_owned());
        let active = queue.pending.take().unwrap();
        for index in 0..10_000 {
            let serial = queue.replace(index.to_string());
            assert_eq!(queue.pending.as_ref().unwrap().0, serial);
        }
        assert_eq!(active, (active_serial, "active".to_owned()));
        assert_eq!(queue.pending.as_ref().unwrap().1, "9999");
        assert!(!queue.complete(active_serial, Ok(None)));
        assert!(queue.completed.is_none());
    }

    #[test]
    fn a_b_a_rejects_old_a_even_with_identical_path_and_generation() {
        let mut queue = Queue::new();
        let first = queue.replace(request("A"));
        let (_, active) = queue.pending.take().unwrap();
        queue.replace(request("B"));
        let latest = queue.replace(request("A"));
        assert_eq!(active, queue.pending.as_ref().unwrap().1);
        assert!(!queue.complete(first, Ok(None)));
        assert!(queue.complete(latest, Ok(None)));
        assert!(queue.take_completion(first).is_none());
        assert!(queue.take_completion(latest).is_some());
    }

    #[test]
    fn cancellation_drops_pending_and_completed_and_invalidates_active_results() {
        let mut queue = Queue::new();
        let serial = queue.replace(request("A"));
        queue.pending.take();
        assert!(queue.complete(serial, Ok(Some(image::RgbaImage::new(2, 2).into()))));
        queue.cancel();
        assert!(queue.completed.is_none());
        assert!(queue.take_completion(serial).is_none());
        assert!(!queue.complete(serial, Ok(None)));
        queue.replace(request("B"));
        queue.cancel();
        assert!(queue.pending.is_none());
        let latest = queue.replace(request("A"));
        assert!(queue.complete(latest, Ok(None)));
    }

    #[test]
    fn changed_generation_path_logout_closed_or_hidden_are_not_publishable() {
        let active = request("A");
        let mut state = UiState {
            request: active.clone(),
            auth_required: false,
            file_open: true,
            window_visible: true,
        };
        assert!(state.accepts(&active));
        state.request.generation += 1;
        assert!(!state.accepts(&active));
        state.request = request("B");
        assert!(!state.accepts(&active));
        state.request = active.clone();
        state.auth_required = true;
        assert!(!state.accepts(&active));
        state.auth_required = false;
        state.file_open = false;
        assert!(!state.accepts(&active));
        state.file_open = true;
        state.window_visible = false;
        assert!(!state.accepts(&active));
        state.window_visible = true;
        state.request = request("");
        assert!(!state.accepts(&state.request));
    }
}
