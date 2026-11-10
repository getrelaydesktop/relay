use crate::{AttachmentRow, Desktop};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, Timer, VecModel};
use std::{
    cell::RefCell,
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
    sync::{OnceLock, mpsc},
    time::Duration,
};

#[derive(Debug)]
struct FileInfo {
    // Canonical, losslessly representable paths also identify duplicates when
    // separate drop events finish. Never send a to_string_lossy() replacement.
    path: String,
    name: String,
    bytes: u64,
}

// Filesystem access belongs exclusively to the validation worker (or tests).
fn inspect(paths: Vec<PathBuf>) -> (Vec<FileInfo>, Vec<String>) {
    let mut inputs = HashSet::new();
    let mut identities = HashSet::new();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for input in paths {
        if !inputs.insert(input.clone()) {
            continue;
        }
        let reject_encoding = || {
            format!(
                "{} — путь нельзя передать без потери символов",
                input.display()
            )
        };
        if input.to_str().is_none() {
            errors.push(reject_encoding());
            continue;
        }
        let path = match input.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!("{} — файл недоступен: {error}", input.display()));
                continue;
            }
        };
        let Some(path_string) = path.to_str() else {
            errors.push(reject_encoding());
            continue;
        };
        if !identities.insert(path.clone()) {
            continue;
        }
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => files.push(FileInfo {
                path: path_string.to_owned(),
                name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path_string)
                    .to_owned(),
                bytes: metadata.len(),
            }),
            Ok(_) => errors.push(format!(
                "{} — выберите обычный файл, а не папку или специальный файл",
                input.display()
            )),
            Err(error) => {
                errors.push(format!("{} — файл недоступен: {error}", input.display()));
            }
        }
    }
    (files, errors)
}

fn merge_files(rows: &mut Vec<AttachmentRow>, files: Vec<FileInfo>) {
    let mut known: HashSet<_> = rows.iter().map(|row| row.path.to_string()).collect();
    for file in files {
        if known.insert(file.path.clone()) {
            rows.push(AttachmentRow {
                path: file.path.into(),
                name: file.name.into(),
                size: size_label(file.bytes).into(),
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Destination {
    chat: i32,
    generation: i32,
}

impl Destination {
    fn capture(ui: &Desktop) -> Self {
        Self {
            chat: ui.get_active_chat().id,
            generation: ui.get_attachment_generation(),
        }
    }

    fn is_current(self, ui: &Desktop) -> bool {
        self == Self::capture(ui)
    }
}

fn can_stage(ui: &Desktop) -> bool {
    !ui.get_auth_required()
        && ui.get_has_chat()
        && ui.get_chat_can_write()
        && !ui.get_upload_busy()
        && !ui.get_message_busy()
        && ui.get_edit_message_id() == 0
}

fn finish_pending(ui: &Desktop) {
    ui.set_attachment_pending(ui.get_attachment_pending().saturating_sub(1).max(0));
}

fn append_error(ui: &Desktop, error: &str) {
    if error.is_empty() {
        return;
    }
    let previous = ui.get_file_preview_error();
    ui.set_file_preview_error(if previous.is_empty() {
        error.into()
    } else {
        format!("{previous}\n{error}").into()
    });
}

fn set_rows(ui: &Desktop, rows: Vec<AttachmentRow>) {
    let first = rows.first().map(|row| row.path.clone()).unwrap_or_default();
    if first.is_empty() || ui.get_file_path() != first {
        ui.set_has_file_preview(false);
        ui.set_file_preview(Default::default());
    }
    ui.set_file_path(first);
    ui.set_attachments(ModelRc::new(VecModel::from(rows)));
}

fn invalidate(ui: &Desktop) {
    ui.set_attachment_generation(ui.get_attachment_generation().wrapping_add(1));
    ui.set_attachment_pending(0);
}

/// Call on chat/account changes and explicit cancellation, not merely when
/// hiding the preview for an upload. This also invalidates native picker results.
pub fn clear(ui: &Desktop) {
    invalidate(ui);
    set_rows(ui, Vec::new());
    ui.set_file_preview_error(Default::default());
    ui.set_attachment_caption(Default::default());
    ui.set_file_open(false);
    ui.set_file_drag_hover(false);
    // Only the picker completion releases picker_busy: an old native dialog
    // can still be open, and clearing the selection must not permit a second one.
}

struct StageJob {
    ui: slint::Weak<Desktop>,
    destination: Destination,
    paths: Vec<PathBuf>,
}

fn worker() -> Result<&'static mpsc::SyncSender<StageJob>, &'static str> {
    static QUEUE: OnceLock<Result<mpsc::SyncSender<StageJob>, String>> = OnceLock::new();
    QUEUE
        .get_or_init(|| {
            // One bounded worker handles bursts without a thread per dropped
            // file. try_send never waits for a slow/network filesystem.
            let (tx, rx) = mpsc::sync_channel::<StageJob>(64);
            std::thread::Builder::new()
                .name("attachment-metadata".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let (files, errors) = inspect(job.paths);
                        let error = errors.join("\n");
                        let _ = job.ui.upgrade_in_event_loop(move |ui| {
                            if !job.destination.is_current(&ui) {
                                return;
                            }
                            finish_pending(&ui);
                            if !can_stage(&ui) || !ui.get_file_open() {
                                return;
                            }
                            // Never restore a snapshot: other drops may already
                            // have appended rows while this job was in flight.
                            let mut rows: Vec<_> = ui.get_attachments().iter().collect();
                            merge_files(&mut rows, files);
                            set_rows(&ui, rows);
                            append_error(&ui, &error);
                        });
                    }
                })
                .map(|_| tx)
                .map_err(|error| format!("Не удалось запустить проверку файлов: {error}"))
        })
        .as_ref()
        .map_err(String::as_str)
}

pub fn stage(ui: &Desktop, paths: impl IntoIterator<Item = PathBuf>) {
    if !can_stage(ui) {
        return;
    }
    let destination = Destination::capture(ui);
    let paths: Vec<_> = paths.into_iter().collect();
    if paths.is_empty() {
        return;
    }
    let fresh = ui.get_attachment_pending() == 0;
    // Include deferred work in the count so Send cannot race the next UI turn.
    ui.set_attachment_pending(ui.get_attachment_pending().saturating_add(1));
    let weak = ui.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        let Some(ui) = weak.upgrade() else { return };
        if !destination.is_current(&ui) {
            return;
        }
        if !can_stage(&ui) {
            finish_pending(&ui);
            return;
        }
        // Do not rebuild the overlay from inside Winit's drop-event dispatch.
        ui.set_file_open(true);
        if fresh {
            ui.set_file_preview_error(Default::default());
        }
        let job = StageJob {
            ui: ui.as_weak(),
            destination,
            paths,
        };
        let result = match worker() {
            Ok(worker) => worker.try_send(job).map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    "Очередь проверки файлов занята. Добавьте эти файлы ещё раз.".to_owned()
                }
                mpsc::TrySendError::Disconnected(_) => {
                    "Проверка файлов остановлена. Перезапустите приложение.".to_owned()
                }
            }),
            Err(error) => Err(error.to_owned()),
        };
        if let Err(error) = result {
            finish_pending(&ui);
            append_error(&ui, &error);
        }
    });
}

pub fn install(ui: &Desktop) {
    let weak = ui.as_weak();
    ui.on_pick_files(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_picker_busy() || !can_stage(&ui) {
            return;
        }
        ui.set_picker_busy(true);
        let destination = Destination::capture(&ui);
        let weak = ui.as_weak();
        // Accessing the Winit parent while its event callback is borrowed can
        // re-enter the backend. Build the dialog only after that event returns.
        Timer::single_shot(Duration::ZERO, move || {
            let Some(ui) = weak.upgrade() else { return };
            if !destination.is_current(&ui) || !can_stage(&ui) {
                ui.set_picker_busy(false);
                return;
            }
            let dialog = ui
                .window()
                .with_winit_window(|window| {
                    rfd::FileDialog::new()
                        .set_parent(window)
                        .set_title("Выберите файлы для отправки")
                })
                .unwrap_or_else(|| rfd::FileDialog::new().set_title("Выберите файлы для отправки"));
            let weak = ui.as_weak();
            let result = std::thread::Builder::new()
                .name("attachment-picker".into())
                .spawn(move || {
                    let paths = dialog.pick_files();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_picker_busy(false);
                        if destination.is_current(&ui) {
                            if let Some(paths) = paths {
                                stage(&ui, paths);
                            }
                        }
                    });
                });
            if let Err(error) = result {
                ui.set_picker_busy(false);
                ui.set_file_open(true);
                append_error(&ui, &format!("Не удалось открыть выбор файлов: {error}"));
            }
        });
    });

    let weak = ui.as_weak();
    let removals = Rc::new(RefCell::new(Vec::new()));
    ui.on_remove_attachment(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_upload_busy() {
            return;
        }
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let Some(row) = ui.get_attachments().row_data(index) else {
            return;
        };
        let schedule = {
            let mut removals = removals.borrow_mut();
            let schedule = removals.is_empty();
            removals.push((Destination::capture(&ui), row.path.to_string()));
            schedule
        };
        if !schedule {
            return;
        }
        let weak = ui.as_weak();
        let removals = Rc::clone(&removals);
        Timer::single_shot(Duration::ZERO, move || {
            // Batch rapid clicks by captured path, not by an index that shifts.
            // Release the RefCell before any model/property notifications.
            let removals = std::mem::take(&mut *removals.borrow_mut());
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_upload_busy() {
                return;
            }
            let paths: HashSet<_> = removals
                .into_iter()
                .filter(|(destination, _)| destination.is_current(&ui))
                .map(|(_, path)| path)
                .collect();
            let model = ui.get_attachments();
            let rows: Vec<_> = model
                .iter()
                .filter(|row| !paths.contains(row.path.as_str()))
                .collect();
            if rows.len() == model.row_count() {
                return;
            }
            if rows.is_empty() {
                clear(&ui);
            } else {
                let cancelled = ui.get_attachment_pending() > 0 || ui.get_picker_busy();
                // Older jobs (including aliases of the removed path) must not
                // resurrect it. Keep the other already-validated rows intact.
                invalidate(&ui);
                set_rows(&ui, rows);
                ui.set_file_preview_error(if cancelled {
                    "Проверка новых вложений отменена. Добавьте файлы снова.".into()
                } else {
                    Default::default()
                });
            }
        });
    });
}

pub fn size_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} Б")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} КБ", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} МБ", bytes as f64 / 1048576.0)
    } else {
        format!("{:.2} ГБ", bytes as f64 / 1073741824.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "relay-attachments-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn validates_files_and_reports_directories_and_missing_paths_once() {
        let directory = TempDir::new();
        let path = directory.file("документ № 1.txt", b"hello");
        let empty = directory.file("empty.txt", b"");
        let missing = directory.0.join("missing");
        let (files, errors) = inspect(vec![
            path.clone(),
            path,
            empty,
            directory.0.clone(),
            missing.clone(),
            missing,
        ]);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "документ № 1.txt");
        assert_eq!(files[0].bytes, 5);
        assert_eq!(files[1].bytes, 0);
        assert!(files.iter().all(|file| Path::new(&file.path).is_absolute()));
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("выберите обычный файл"));
        assert!(errors[1].contains("файл недоступен"));
    }

    #[test]
    fn normalizes_aliases_without_merging_distinct_files_with_the_same_name() {
        let directory = TempDir::new();
        let path = directory.file("hello.txt", b"hello");
        let child = directory.0.join("child");
        fs::create_dir(&child).unwrap();
        let other = child.join("hello.txt");
        fs::write(&other, b"other").unwrap();
        let (files, errors) = inspect(vec![
            path.clone(),
            directory.0.join(".").join("hello.txt"),
            child.join("..").join("hello.txt"),
            other,
        ]);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 2);
        assert_ne!(files[0].path, files[1].path);
        assert_eq!(files[0].name, files[1].name);
        assert_eq!(Path::new(&files[0].path), path.canonicalize().unwrap());
    }

    #[test]
    fn completed_batches_merge_in_order_and_do_not_duplicate_existing_rows() {
        let directory = TempDir::new();
        let first = directory.file("first.txt", b"1");
        let second = directory.file("second.txt", b"22");
        let mut rows = Vec::new();
        let (files, errors) = inspect(vec![first.clone()]);
        assert!(errors.is_empty());
        merge_files(&mut rows, files);
        let (files, errors) = inspect(vec![first, second.clone(), second]);
        assert!(errors.is_empty());
        merge_files(&mut rows, files);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "first.txt");
        assert_eq!(rows[0].size, "1 Б");
        assert_eq!(rows[1].name, "second.txt");
        assert_eq!(rows[1].size, "2 Б");
    }

    #[test]
    fn accepts_a_large_selection_without_a_silent_file_limit() {
        let directory = TempDir::new();
        let paths = (0..128)
            .map(|index| directory.file(&format!("{index}.txt"), b""))
            .collect();
        let (files, errors) = inspect(paths);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 128);
        assert_eq!(files[0].name, "0.txt");
        assert_eq!(files[127].name, "127.txt");
    }

    #[test]
    fn empty_selection_is_a_no_op() {
        let (files, errors) = inspect(Vec::new());
        assert!(files.is_empty());
        assert!(errors.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn follows_file_symlinks_for_duplicate_detection() {
        let directory = TempDir::new();
        let target = directory.file("target.txt", b"target");
        let link = directory.0.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let (files, errors) = inspect(vec![link, target]);
        assert!(errors.is_empty());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "target.txt");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_paths_instead_of_sending_a_different_path() {
        use std::os::unix::ffi::OsStringExt;
        let directory = TempDir::new();
        let path = directory
            .0
            .join(std::ffi::OsString::from_vec(b"bad-\xff.txt".to_vec()));
        fs::write(&path, b"file").unwrap();
        let (files, errors) = inspect(vec![path]);
        assert!(files.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("без потери символов"));
    }

    #[test]
    fn sizes_are_human_readable_at_unit_boundaries() {
        for (bytes, expected) in [
            (0, "0 Б"),
            (1023, "1023 Б"),
            (1024, "1.0 КБ"),
            (1536, "1.5 КБ"),
            (1048576, "1.0 МБ"),
            (1572864, "1.5 МБ"),
            (1073741824, "1.00 ГБ"),
            (1342177280, "1.25 ГБ"),
        ] {
            assert_eq!(size_label(bytes), expected);
        }
    }
}
