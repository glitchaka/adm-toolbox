//! Clipboard adapter invoked by the bundled editor through redirected pipes.
use std::{fs, io::{Read, Write}, path::Path, ptr::{null, null_mut}, time::Duration};
use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND}, System::{DataExchange::*, Memory::*},
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
};

pub fn helper(args: &[String]) -> Result<i32> {
    let operation = args.get(1).context("falta operación del portapapeles")?;
    let transfer = Path::new(args.get(2).context("falta archivo de transferencia")?);
    match operation.as_str() {
        "get" => {
            let bytes = match fs::read(transfer) {
                Ok(bytes) => { fs::remove_file(transfer)?; bytes }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => read_text()?.into_bytes(),
                Err(error) => return Err(error.into()),
            };
            std::io::stdout().write_all(&bytes)?;
        }
        "set" => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            write_text(&text)?;
        }
        _ => bail!("operación de portapapeles desconocida"),
    }
    Ok(0)
}

struct Clipboard(HWND);
impl Clipboard {
    fn open() -> Result<Self> {
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0,
                HWND_MESSAGE, null_mut(), null_mut(), null());
            if hwnd.is_null() { return Err(std::io::Error::last_os_error().into()); }
            for _ in 0..20 {
                if OpenClipboard(hwnd) != 0 { return Ok(Self(hwnd)); }
                std::thread::sleep(Duration::from_millis(5));
            }
            let error = std::io::Error::last_os_error();
            DestroyWindow(hwnd);
            Err(error).context("No se pudo abrir el portapapeles")
        }
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) { unsafe { CloseClipboard(); DestroyWindow(self.0); } }
}
fn read_text() -> Result<String> {
    let _clipboard = Clipboard::open()?;
    unsafe {
        let memory = GetClipboardData(13);
        if memory.is_null() { return Ok(String::new()); }
        let data = GlobalLock(memory) as *const u16;
        if data.is_null() { return Err(std::io::Error::last_os_error().into()); }
        let units = std::slice::from_raw_parts(data, GlobalSize(memory) / 2);
        let length = units.iter().position(|ch| *ch == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..length]);
        GlobalUnlock(memory);
        Ok(text)
    }
}
fn write_text(text: &str) -> Result<()> {
    let _clipboard = Clipboard::open()?;
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
        if memory.is_null() { return Err(std::io::Error::last_os_error().into()); }
        let data = GlobalLock(memory) as *mut u16;
        if data.is_null() { GlobalFree(memory); return Err(std::io::Error::last_os_error().into()); }
        std::ptr::copy_nonoverlapping(units.as_ptr(), data, units.len());
        GlobalUnlock(memory);
        if EmptyClipboard() == 0 || SetClipboardData(13, memory).is_null() {
            let error = std::io::Error::last_os_error();
            GlobalFree(memory);
            return Err(error.into());
        }
    }
    Ok(())
}
