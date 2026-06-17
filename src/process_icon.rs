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

    let extracted = unsafe { ExtractIconExW(path.as_ptr(), 0, null_mut(), &mut icon, 1) };
    if extracted == 0 || icon.is_null() {
        return None;
    }

    let image =
        hicon_to_bmp(icon).map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Bmp, bytes)));

    unsafe {
        DestroyIcon(icon);
    }

    image
}

fn hicon_to_bmp(icon: HICON) -> Option<Vec<u8>> {
    let width = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let height = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(16);
    let byte_len = width.checked_mul(height)?.checked_mul(4)? as usize;

    let hdc = unsafe { CreateCompatibleDC(null_mut()) };
    if hdc.is_null() {
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let bitmap = unsafe { CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap as HGDIOBJ);
            }
            DeleteDC(hdc);
        }
        return None;
    }

    unsafe {
        std::ptr::write_bytes(bits, 0, byte_len);
    }

    let old_object = unsafe { SelectObject(hdc, bitmap as HGDIOBJ) };
    let drawn = unsafe { DrawIconEx(hdc, 0, 0, icon, width, height, 0, null_mut(), DI_NORMAL) };

    let pixels = if drawn != 0 {
        Some(unsafe { slice::from_raw_parts(bits.cast::<u8>(), byte_len).to_vec() })
    } else {
        None
    };

    unsafe {
        if !old_object.is_null() {
            SelectObject(hdc, old_object);
        }
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
