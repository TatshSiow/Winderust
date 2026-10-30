use std::{
    ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut, slice,
    sync::Arc,
};

use windows_sys::Win32::{
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    },
    UI::{
        Shell::SHDefExtractIconW,
        WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON},
    },
};

// A shared 64px source avoids enlarging the 20px UI icons at common display scales.
const ICON_PIXELS: i32 = 64;

/// Framework-independent, top-down RGBA icon pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn load_process_icon(path: &Path) -> Option<Arc<ProcessIcon>> {
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut icon: HICON = null_mut();

    // SAFETY: path is terminated UTF-16 and icon is a writable out-pointer. The low word
    // requests a 64px large icon; no small icon is requested.
    let extracted = unsafe {
        SHDefExtractIconW(
            path.as_ptr(),
            0,
            0,
            &mut icon,
            null_mut(),
            ICON_PIXELS as u32,
        )
    };
    if extracted != 0 || icon.is_null() {
        return None;
    }

    let image = hicon_to_rgba(icon).map(Arc::new);

    // SAFETY: icon was returned by SHDefExtractIconW and is destroyed exactly once after rendering.
    unsafe {
        DestroyIcon(icon);
    }

    image
}

fn hicon_to_rgba(icon: HICON) -> Option<ProcessIcon> {
    let width = ICON_PIXELS;
    let height = ICON_PIXELS;
    let byte_len = width.checked_mul(height)?.checked_mul(4)? as usize;

    // SAFETY: A null source DC requests a memory DC compatible with the current screen.
    let hdc = unsafe { CreateCompatibleDC(null_mut()) };
    if hdc.is_null() {
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    // SAFETY: BITMAPINFO is a plain Win32 data structure for which all-zero is a valid baseline.
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    // SAFETY: hdc is live, bmi is fully initialized, bits is writable, and no file mapping is
    // supplied.
    let bitmap = unsafe { CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        // SAFETY: Any non-null bitmap and hdc were created above and are released exactly once.
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap as HGDIOBJ);
            }
            DeleteDC(hdc);
        }
        return None;
    }

    // SAFETY: CreateDIBSection returned at least byte_len writable bytes for this 32-bit bitmap.
    unsafe {
        std::ptr::write_bytes(bits, 0, byte_len);
    }

    // SAFETY: hdc and bitmap are live GDI objects owned by this function.
    let old_object = unsafe { SelectObject(hdc, bitmap as HGDIOBJ) };
    if old_object.is_null() {
        // SAFETY: bitmap and hdc are live, owned by this function, and released exactly once.
        unsafe {
            DeleteObject(bitmap as HGDIOBJ);
            DeleteDC(hdc);
        }
        return None;
    }
    // SAFETY: hdc has the bitmap selected, icon is live for the call, and dimensions are positive.
    let drawn = unsafe { DrawIconEx(hdc, 0, 0, icon, width, height, 0, null_mut(), DI_NORMAL) };

    let pixels = if drawn != 0 {
        // SAFETY: bits references byte_len initialized bytes in the live DIB section.
        Some(unsafe { slice::from_raw_parts(bits.cast::<u8>(), byte_len).to_vec() })
    } else {
        None
    };

    // SAFETY: The selected object is restored before the bitmap, then bitmap and hdc are each
    // released exactly once.
    unsafe {
        SelectObject(hdc, old_object);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(hdc);
    }

    pixels.map(|pixels| ProcessIcon {
        width: width as u32,
        height: height as u32,
        rgba: rgba_from_bgra(pixels),
    })
}

fn rgba_from_bgra(mut pixels: Vec<u8>) -> Vec<u8> {
    // Legacy icons have no alpha channel. Preserve their opaque color instead of
    // making the entire icon transparent; modern icons retain their alpha mask.
    let opaque = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if opaque {
            pixel[3] = 255;
        } else if pixel[3] != 0 {
            // DrawIconEx writes premultiplied channels. UI image APIs accept
            // straight RGBA and apply premultiplication when they upload it.
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_channels_and_modern_transparency_are_preserved() {
        assert_eq!(
            rgba_from_bgra(vec![20, 40, 60, 128, 0, 0, 0, 0]),
            vec![120, 80, 40, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn legacy_icons_without_an_alpha_channel_remain_visible() {
        assert_eq!(rgba_from_bgra(vec![1, 2, 3, 0]), vec![3, 2, 1, 255]);
    }
}
