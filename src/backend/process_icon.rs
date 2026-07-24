use std::{
    ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut, slice,
    sync::Arc,
};

use gpui::{Image, ImageFormat};
use windows_sys::Win32::{
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    },
    UI::{
        Shell::ExtractIconExW,
        WindowsAndMessaging::{
            DestroyIcon, DrawIconEx, GetSystemMetrics, DI_NORMAL, HICON, SM_CXSMICON, SM_CYSMICON,
        },
    },
};

const BMP_FILE_HEADER_SIZE: u32 = 14;
const BMP_INFO_HEADER_SIZE: u32 = 40;

pub fn load_process_icon(path: &Path) -> Option<Arc<Image>> {
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut icon: HICON = null_mut();

    // SAFETY: path is terminated UTF-16, icon is a writable out-pointer, and one icon slot is
    // requested.
    let extracted = unsafe { ExtractIconExW(path.as_ptr(), 0, null_mut(), &mut icon, 1) };
    if extracted == 0 || icon.is_null() {
        return None;
    }

    let image =
        hicon_to_bmp(icon).map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Bmp, bytes)));

    // SAFETY: icon was returned by ExtractIconExW and is destroyed exactly once after rendering.
    unsafe {
        DestroyIcon(icon);
    }

    image
}

fn hicon_to_bmp(icon: HICON) -> Option<Vec<u8>> {
    // SAFETY: GetSystemMetrics has no pointer or lifetime requirements.
    let width = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    // SAFETY: GetSystemMetrics has no pointer or lifetime requirements.
    let height = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(16);
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

    pixels.map(|pixels| bmp_from_bgra_pixels(width, height, pixels))
}

fn bmp_from_bgra_pixels(width: i32, height: i32, pixels: Vec<u8>) -> Vec<u8> {
    let pixel_offset = BMP_FILE_HEADER_SIZE + BMP_INFO_HEADER_SIZE;
    let file_size = pixel_offset + pixels.len() as u32;
    let mut bytes = Vec::with_capacity(file_size as usize);

    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&pixel_offset.to_le_bytes());
    bytes.extend_from_slice(&BMP_INFO_HEADER_SIZE.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&(-height).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&BI_RGB.to_le_bytes());
    bytes.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&pixels);

    bytes
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_describes_the_supplied_pixels() {
        let pixels = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let bytes = bmp_from_bgra_pixels(2, 1, pixels.clone());

        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 62);
        assert_eq!(u32::from_le_bytes(bytes[10..14].try_into().unwrap()), 54);
        assert_eq!(i32::from_le_bytes(bytes[18..22].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(bytes[22..26].try_into().unwrap()), -1);
        assert_eq!(&bytes[54..], pixels);
    }
}
