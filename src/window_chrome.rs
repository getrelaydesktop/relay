//! Native move initiation for the custom Windows caption.
//! Winit 0.30.13 passes the address of POINTS as WM_NCLBUTTONDOWN's LPARAM.
//! Windows expects the signed screen coordinates packed by value, not a pointer.
#[cfg(windows)]
pub fn begin_move(window: &slint::winit_030::winit::window::Window) -> Result<(), String> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::ReleaseCapture,
            WindowsAndMessaging::{GetCursorPos, HTCAPTION, PostMessageW, WM_NCLBUTTONDOWN},
        },
    };
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("Not a Win32 window".into());
    };
    let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
    let mut point = POINT { x: 0, y: 0 };
    // Called on the window's UI thread. Post by value so no stack data escapes;
    // DefWindowProc retains native dragging, snapping and restore-from-maximized.
    unsafe {
        if GetCursorPos(&mut point) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        ReleaseCapture();
        if PostMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            HTCAPTION as usize,
            packed_position(point.x, point.y),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

/// Preserve both maximized state and the normal restore rectangle. Slint's
/// destroy-on-hide path retains only the current size/position, which is not
/// enough to restore a maximized window without losing its normal dimensions.
#[cfg(windows)]
#[derive(Clone, Copy)]
pub struct Placement(windows_sys::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT);

#[cfg(windows)]
impl Placement {
    pub fn capture(window: &slint::winit_030::winit::window::Window) -> Option<Self> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
        let hwnd = native_handle(window).ok()?;
        // WINDOWPLACEMENT contains only integers and POINT/RECT values.
        let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
            return None;
        }
        Some(Self(placement))
    }

    pub fn restore(&self, window: &slint::winit_030::winit::window::Window) -> Result<(), String> {
        use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPlacement;
        let hwnd = native_handle(window)?;
        if unsafe { SetWindowPlacement(hwnd, &self.0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn native_handle(
    window: &slint::winit_030::winit::window::Window,
) -> Result<windows_sys::Win32::Foundation::HWND, String> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as _),
        _ => Err("Not a Win32 window".into()),
    }
}

#[cfg(any(windows, test))]
fn packed_position(x: i32, y: i32) -> isize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caption_coordinates_are_values_and_preserve_negative_monitors() {
        for (x, y) in [(100, 13), (-1920, 40), (3840, -1080), (-32768, 32767)] {
            let packed = packed_position(x, y);
            assert_eq!(packed as u16 as i16 as i32, x);
            assert_eq!((packed >> 16) as u16 as i16 as i32, y);
        }
    }
}
