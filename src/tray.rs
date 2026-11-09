//! Native Windows notification icon. A blocking message queue has no idle timer.
use std::{ptr, sync::mpsc, thread};
use windows_sys::Win32::{
    Foundation::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};

const CALLBACK: u32 = WM_APP + 1;
struct State {
    restore: Box<dyn Fn() + Send>,
    exit: Box<dyn Fn() + Send>,
    data: NOTIFYICONDATAW,
    taskbar_created: u32,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if !state.is_null() {
            let state = &mut *state;
            if message == state.taskbar_created {
                Shell_NotifyIconW(NIM_ADD, &state.data);
                return 0;
            }
            if message == CALLBACK {
                match lparam as u32 {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => (state.restore)(),
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        let menu = CreatePopupMenu();
                        if !menu.is_null() {
                            AppendMenuW(menu, MF_STRING, 1, wide("Открыть Relay").as_ptr());
                            AppendMenuW(menu, MF_STRING, 2, wide("Выйти").as_ptr());
                            let mut point = POINT::default();
                            GetCursorPos(&mut point);
                            SetForegroundWindow(hwnd);
                            let action = TrackPopupMenu(
                                menu,
                                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                                point.x,
                                point.y,
                                0,
                                hwnd,
                                ptr::null(),
                            );
                            DestroyMenu(menu);
                            if action == 1 {
                                (state.restore)();
                            } else if action == 2 {
                                (state.exit)();
                            }
                            PostMessageW(hwnd, WM_NULL, 0, 0);
                        }
                    }
                    _ => (),
                }
                return 0;
            }
            if message == WM_DESTROY {
                Shell_NotifyIconW(NIM_DELETE, &state.data);
                PostQuitMessage(0);
                return 0;
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

pub struct Tray {
    hwnd: isize,
    thread: Option<thread::JoinHandle<()>>,
}
impl Tray {
    pub fn new(
        restore: impl Fn() + Send + 'static,
        exit: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class = wide("RelayNotificationWindow");
            let definition = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&definition);
            let mut state = Box::new(State {
                restore: Box::new(restore),
                exit: Box::new(exit),
                data: std::mem::zeroed(),
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            });
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("Relay Desktop").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                (&mut *state as *mut State).cast(),
            );
            if hwnd.is_null() {
                let _ = ready.send(Err(std::io::Error::last_os_error()));
                return;
            }
            let icon = make_icon(instance);
            state.data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            state.data.hWnd = hwnd;
            state.data.uID = 1;
            state.data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            state.data.uCallbackMessage = CALLBACK;
            state.data.hIcon = icon;
            let title = wide("Relay Desktop");
            state.data.szTip[..title.len()].copy_from_slice(&title);
            if Shell_NotifyIconW(NIM_ADD, &state.data) == 0 {
                let error = std::io::Error::last_os_error();
                DestroyWindow(hwnd);
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                let _ = ready.send(Err(error));
                return;
            }
            if ready.send(Ok(hwnd as isize)).is_err() {
                DestroyWindow(hwnd);
            }
            let mut message = MSG::default();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if !icon.is_null() {
                DestroyIcon(icon);
            }
            UnregisterClassW(class.as_ptr(), instance);
        });
        match receiver.recv() {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(std::io::Error::other("Tray initialization stopped")),
        }
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe fn make_icon(instance: HINSTANCE) -> HICON {
    // Rasterize a small vector paper plane at 4x for smooth tray edges.
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let mut color = [0u32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                    let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                    if (px - 16.0).powi(2) + (py - 16.0).powi(2) < 15.0f32.powi(2) {
                        let plane = px > 7.0
                            && px < 25.0
                            && py > 8.0 + (25.0 - px) * 0.37
                            && py < 24.0 - (25.0 - px) * 0.65;
                        let c = if plane {
                            [255, 255, 255, 255]
                        } else {
                            [221, 159, 48, 255]
                        };
                        for channel in 0..4 {
                            color[channel] += c[channel];
                        }
                    }
                }
            }
            for channel in 0..4 {
                pixels[(y * 32 + x) * 4 + channel] = (color[channel] / 16) as u8;
            }
        }
    }
    unsafe {
        CreateIcon(
            instance,
            32,
            32,
            1,
            32,
            [0u8; 128].as_ptr(),
            pixels.as_ptr(),
        )
    }
}
