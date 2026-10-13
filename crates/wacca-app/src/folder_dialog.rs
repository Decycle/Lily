//! The native Windows folder picker (`IFileOpenDialog` with `FOS_PICKFOLDERS`) through raw COM
//! calls: no extra crates. Blocking; call it on its own thread.

use std::path::PathBuf;

/// Shows the dialog owned by window `owner` (an `HWND`, 0 = none). `Ok(None)`: cancelled.
#[cfg(windows)]
pub fn pick_folder(owner: isize, title: &str) -> Result<Option<PathBuf>, String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::null_mut;

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    const CLSID_FILE_OPEN_DIALOG: Guid = Guid(
        0xDC1C_5A9C,
        0xE88A,
        0x4DDE,
        [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
    );
    const IID_IFILE_OPEN_DIALOG: Guid = Guid(
        0xD57C_7288,
        0xD4AD,
        0x4768,
        [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
    );
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
    /// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`.
    const CANCELLED: i32 = 0x8007_04C7_u32 as i32;
    // Vtable slots: IUnknown 0-2, IModalWindow::Show 3, IFileDialog::SetOptions 9,
    // GetOptions 10, SetTitle 17, GetResult 20; IShellItem::GetDisplayName 5.
    const RELEASE: usize = 2;
    const SHOW: usize = 3;
    const SET_OPTIONS: usize = 9;
    const GET_OPTIONS: usize = 10;
    const SET_TITLE: usize = 17;
    const GET_RESULT: usize = 20;
    const GET_DISPLAY_NAME: usize = 5;

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            context: u32,
            iid: *const Guid,
            out: *mut *mut c_void,
        ) -> i32;
        fn CoTaskMemFree(p: *mut c_void);
    }

    /// Method `i` of COM object `obj`.
    ///
    /// # Safety
    /// `obj` is a live COM interface pointer whose vtable has a method of type `F` at `i`.
    unsafe fn method<F: Copy>(obj: *mut c_void, i: usize) -> F {
        // SAFETY: per the contract; a COM object starts with its vtable pointer.
        unsafe {
            let vtable = *(obj as *const *const usize);
            std::mem::transmute_copy(&*vtable.add(i))
        }
    }
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;

    let check = |what: &str, hr: i32| {
        if hr < 0 {
            Err(format!("{what} failed (0x{:08X})", hr as u32))
        } else {
            Ok(())
        }
    };
    // SAFETY: plain COM calls on this thread; every pointer comes from the call before it and
    // is released once.
    unsafe {
        let init = CoInitializeEx(
            null_mut(),
            COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
        );
        check("CoInitializeEx", init)?;
        let result = (|| {
            let mut dialog = null_mut();
            check(
                "CoCreateInstance(FileOpenDialog)",
                CoCreateInstance(
                    &CLSID_FILE_OPEN_DIALOG,
                    null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_IFILE_OPEN_DIALOG,
                    &mut dialog,
                ),
            )?;
            let picked = (|| {
                let mut options = 0u32;
                let get: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32 =
                    method(dialog, GET_OPTIONS);
                check("GetOptions", get(dialog, &mut options))?;
                let set: unsafe extern "system" fn(*mut c_void, u32) -> i32 =
                    method(dialog, SET_OPTIONS);
                check(
                    "SetOptions",
                    set(dialog, options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM),
                )?;
                let wide: Vec<u16> = title.encode_utf16().chain([0]).collect();
                let set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
                    method(dialog, SET_TITLE);
                check("SetTitle", set_title(dialog, wide.as_ptr()))?;
                let show: unsafe extern "system" fn(*mut c_void, isize) -> i32 =
                    method(dialog, SHOW);
                let hr = show(dialog, owner);
                if hr == CANCELLED {
                    return Ok(None);
                }
                check("Show", hr)?;
                let mut item = null_mut();
                let get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
                    method(dialog, GET_RESULT);
                check("GetResult", get_result(dialog, &mut item))?;
                let mut name: *mut u16 = null_mut();
                let display: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32 =
                    method(item, GET_DISPLAY_NAME);
                let hr = display(item, SIGDN_FILESYSPATH, &mut name);
                method::<Release>(item, RELEASE)(item);
                check("GetDisplayName", hr)?;
                let len = (0..).take_while(|&i| *name.add(i) != 0).count();
                let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(name, len));
                CoTaskMemFree(name.cast());
                Ok(Some(PathBuf::from(path)))
            })();
            method::<Release>(dialog, RELEASE)(dialog);
            picked
        })();
        CoUninitialize();
        result
    }
}

#[cfg(not(windows))]
pub fn pick_folder(_owner: isize, _title: &str) -> Result<Option<PathBuf>, String> {
    Err("the folder picker is only available on Windows".into())
}
