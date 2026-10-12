//! `Local\WACCON_SHARED_BUFFER`, ABI 1.1 (samnyan/Waccon-IO `waccon-io/include/waccon/shared_memory.h`).
//!
//! Layout (packed, little-endian): magic u32 @0, major u16 @4, minor u16 @6, struct_size u32 @8, capabilities
//! u32 @12, input_sequence u32 @16, output_sequence u32 @20, input record @24 (262 bytes, see
//! [`crate::wcon::input_payload`]), LED output @286 (u32 unit_count, 480 x RGBA, u64 timestamp_us), server
//! endpoint @2218 and io endpoint @2250 (u32 pid, u16 major, u16 minor, u64 started_ms, u64 heartbeat_ms,
//! u64 input_sequence). Writers use a sequence lock: odd while writing, even when stable. waccon_io.dll accepts a
//! new input record when the even sequence differs from the last one it took and `0 < lease_ms <= 60000`, then
//! keeps it for `lease_ms` (all cells released afterwards) and echoes the sequence into `io.input_sequence`.

use crate::wcon::INPUT_PAYLOAD_LEN;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, Ordering, fence};
use thiserror::Error;

pub const NAME: &str = "Local\\WACCON_SHARED_BUFFER";
pub const MAGIC: u32 = 0x4E4F_4357; // "WCON"
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 1;
pub const SIZE: usize = 2282;
const INPUT_SEQ: usize = 16;
const OUTPUT_SEQ: usize = 20;
const INPUT: usize = 24;
const OUTPUT: usize = INPUT + INPUT_PAYLOAD_LEN;
pub const LED_LEN: usize = 4 + 480 * 4 + 8;
const SERVER: usize = OUTPUT + LED_LEN;
const IO: usize = SERVER + 32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShmError {
    #[error("view too small or misaligned")]
    View,
    #[error("ABI mismatch: magic {magic:#010x} version {major}.{minor} size {size}")]
    Abi {
        magic: u32,
        major: u16,
        minor: u16,
        size: u32,
    },
}

/// A view of the shared buffer. Reads/writes are volatile because other processes change it concurrently.
#[derive(Debug)]
pub struct ShmView {
    base: *mut u8,
}

impl ShmView {
    /// # Safety
    /// `base` must point to at least [`SIZE`] writable bytes, 4-byte aligned, valid for the view's lifetime.
    pub unsafe fn from_raw(base: *mut u8, len: usize) -> Result<Self, ShmError> {
        if base.is_null() || len < SIZE || !(base as usize).is_multiple_of(4) {
            return Err(ShmError::View);
        }
        Ok(Self { base })
    }

    fn seq(&self, offset: usize) -> &AtomicU32 {
        // SAFETY: offsets 16/20 are inside the view and 4-aligned because the base is.
        unsafe { AtomicU32::from_ptr(self.base.add(offset).cast()) }
    }

    fn read_bytes<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut out = [0; N];
        for (i, b) in out.iter_mut().enumerate() {
            // SAFETY: callers only pass offsets with offset + N <= SIZE.
            *b = unsafe { self.base.add(offset + i).read_volatile() };
        }
        out
    }

    fn u64_at(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.read_bytes(offset))
    }

    /// Checks magic, version and size the way waccon_io.dll does.
    pub fn check_header(&self) -> Result<(), ShmError> {
        let h: [u8; 12] = self.read_bytes(0);
        let magic = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let major = u16::from_le_bytes([h[4], h[5]]);
        let minor = u16::from_le_bytes([h[6], h[7]]);
        let size = u32::from_le_bytes([h[8], h[9], h[10], h[11]]);
        if magic == MAGIC && major == MAJOR && minor == MINOR && size as usize == SIZE {
            Ok(())
        } else {
            Err(ShmError::Abi {
                magic,
                major,
                minor,
                size,
            })
        }
    }

    /// Publishes one input record (as Waccon.Server's `PublishInput`); returns the new, even sequence.
    pub fn publish_input(&self, record: &[u8; INPUT_PAYLOAD_LEN]) -> u32 {
        let seq = self.seq(INPUT_SEQ);
        let odd = seq.load(Ordering::Acquire).wrapping_add(1) | 1;
        seq.store(odd, Ordering::Release);
        fence(Ordering::SeqCst);
        for (i, &b) in record.iter().enumerate() {
            // SAFETY: the input record lies inside the view.
            unsafe { self.base.add(INPUT + i).write_volatile(b) };
        }
        fence(Ordering::SeqCst);
        let even = odd.wrapping_add(1);
        seq.store(even, Ordering::Release);
        even
    }

    /// The current input record (as last published).
    pub fn input_record(&self) -> [u8; INPUT_PAYLOAD_LEN] {
        self.read_bytes(INPUT)
    }

    /// Test stand-in for waccon_io.dll taking the current record: echoes the input sequence into
    /// `io.input_sequence`.
    #[cfg(test)]
    pub(crate) fn simulate_dll_ack(&self) {
        let seq = u64::from(self.seq(INPUT_SEQ).load(Ordering::Acquire));
        for (i, b) in seq.to_le_bytes().into_iter().enumerate() {
            // SAFETY: the io endpoint lies inside the view.
            unsafe { self.base.add(IO + 24 + i).write_volatile(b) };
        }
    }

    /// Last input sequence waccon_io.dll consumed (its `io.input_sequence`); equal to the value returned by
    /// [`Self::publish_input`] once the game-side DLL has taken the record.
    pub fn io_consumed_sequence(&self) -> u64 {
        self.u64_at(IO + 24)
    }

    /// waccon_io.dll heartbeat, `GetTickCount64` milliseconds (0 = DLL never attached).
    pub fn io_heartbeat_ms(&self) -> u64 {
        self.u64_at(IO + 16)
    }

    /// Latest stable LED snapshot `(sequence, unit_count, rgba..., timestamp_us)`, `None` while being written.
    pub fn read_leds(&self) -> Option<(u32, [u8; LED_LEN])> {
        let seq = self.seq(OUTPUT_SEQ);
        let before = seq.load(Ordering::Acquire);
        if before & 1 != 0 {
            return None;
        }
        let data = self.read_bytes::<LED_LEN>(OUTPUT);
        fence(Ordering::SeqCst);
        (seq.load(Ordering::Acquire) == before).then_some((before, data))
    }
}

/// Heap-backed stand-in for the mapping (dry runs, tests): valid ABI 1.1 header, no DLL attached (never acks).
#[derive(Debug)]
pub struct MemBuffer {
    mem: Box<[UnsafeCell<u64>]>,
}

impl Default for MemBuffer {
    fn default() -> Self {
        let mem = (0..SIZE.div_ceil(8)).map(|_| UnsafeCell::new(0)).collect();
        let buf = Self { mem };
        let mut header = [0u8; 16];
        header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        header[4..6].copy_from_slice(&MAJOR.to_le_bytes());
        header[6..8].copy_from_slice(&MINOR.to_le_bytes());
        header[8..12].copy_from_slice(&(SIZE as u32).to_le_bytes());
        let base = buf.base();
        for (i, b) in header.into_iter().enumerate() {
            // SAFETY: the header lies inside the allocation, which nothing else references yet.
            unsafe { base.add(i).write(b) };
        }
        buf
    }
}

impl MemBuffer {
    fn base(&self) -> *mut u8 {
        // UnsafeCell makes writes through a shared reference sound; the slice is 8-aligned and >= SIZE bytes.
        UnsafeCell::raw_get(self.mem.as_ptr()).cast()
    }

    /// A view valid while `self` lives.
    pub fn view(&self) -> ShmView {
        ShmView { base: self.base() }
    }
}

/// An opened mapping of the shared buffer. Only opens an existing mapping (never creates one), so it fails
/// unless waccon_io.dll or Waccon.Server is running.
#[cfg(windows)]
#[derive(Debug)]
pub struct SharedBuffer {
    handle: *mut core::ffi::c_void,
    view: *mut core::ffi::c_void,
}

#[cfg(windows)]
mod ffi {
    use core::ffi::c_void;
    pub const FILE_MAP_WRITE: u32 = 0x2;
    pub const FILE_MAP_READ: u32 = 0x4;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        pub fn MapViewOfFile(
            h: *mut c_void,
            access: u32,
            hi: u32,
            lo: u32,
            len: usize,
        ) -> *mut c_void;
        pub fn UnmapViewOfFile(addr: *const c_void) -> i32;
        pub fn CloseHandle(h: *mut c_void) -> i32;
    }
}

#[cfg(windows)]
impl SharedBuffer {
    pub fn open_existing(name: &str) -> std::io::Result<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let access = ffi::FILE_MAP_READ | ffi::FILE_MAP_WRITE;
        // SAFETY: plain Win32 calls with a NUL-terminated name; results are checked.
        unsafe {
            let handle = ffi::OpenFileMappingW(access, 0, wide.as_ptr());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let view = ffi::MapViewOfFile(handle, access, 0, 0, SIZE);
            if view.is_null() {
                let e = std::io::Error::last_os_error();
                ffi::CloseHandle(handle);
                return Err(e);
            }
            Ok(Self { handle, view })
        }
    }

    pub fn view(&self) -> ShmView {
        // SAFETY: MapViewOfFile returned SIZE bytes, page aligned, alive until drop.
        ShmView {
            base: self.view.cast(),
        }
    }
}

#[cfg(windows)]
impl Drop for SharedBuffer {
    fn drop(&mut self) {
        // SAFETY: both came from successful Win32 calls in open_existing.
        unsafe {
            ffi::UnmapViewOfFile(self.view);
            ffi::CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::CELLS;
    use crate::wcon::input_payload;

    fn fake() -> (Vec<u64>, ShmView) {
        let mut mem = vec![0u64; SIZE.div_ceil(8)];
        let base = mem.as_mut_ptr().cast::<u8>();
        let bytes = unsafe { std::slice::from_raw_parts_mut(base, SIZE) };
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..6].copy_from_slice(&MAJOR.to_le_bytes());
        bytes[6..8].copy_from_slice(&MINOR.to_le_bytes());
        bytes[8..12].copy_from_slice(&(SIZE as u32).to_le_bytes());
        let view = unsafe { ShmView::from_raw(base, SIZE) }.unwrap();
        (mem, view)
    }

    /// Mirrors waccon_state.c `waccon_shm_read_input` + the echo into `io.input_sequence`.
    fn waccon_consume(view: &ShmView, last: &mut u32) -> Option<[u8; INPUT_PAYLOAD_LEN]> {
        let before = view.seq(INPUT_SEQ).load(Ordering::Acquire);
        if before & 1 != 0 || before == *last {
            return None;
        }
        let rec = view.read_bytes::<INPUT_PAYLOAD_LEN>(INPUT);
        let lease = u64::from_le_bytes(rec[254..262].try_into().unwrap());
        if !(1..=60_000).contains(&lease) {
            return None;
        }
        *last = before;
        for (i, b) in u64::from(before).to_le_bytes().into_iter().enumerate() {
            unsafe { view.base.add(IO + 24 + i).write_volatile(b) };
        }
        Some(rec)
    }

    #[test]
    fn layout_size() {
        assert_eq!(IO + 32, SIZE);
        assert_eq!(OUTPUT, 286);
        assert_eq!(SERVER, 2218);
    }

    #[test]
    fn header_check() {
        let (mut mem, view) = fake();
        assert_eq!(view.check_header(), Ok(()));
        mem[0] = 0;
        assert!(matches!(
            view.check_header(),
            Err(ShmError::Abi { magic: 0, .. })
        ));
    }

    #[test]
    fn publish_is_seen_once_and_acknowledged() {
        let (_mem, view) = fake();
        let mut cells = [0; CELLS];
        cells[14] = 1;
        let rec = input_payload(0, 0, &cells, 1, 0, 200);
        let mut last = 0;
        let s1 = view.publish_input(&rec);
        assert_eq!(s1 % 2, 0);
        assert_eq!(waccon_consume(&view, &mut last), Some(rec));
        assert_eq!(view.io_consumed_sequence(), u64::from(s1));
        assert_eq!(waccon_consume(&view, &mut last), None);
        // Re-publishing identical content still advances the sequence (needed to renew the lease).
        let s2 = view.publish_input(&rec);
        assert_ne!(s1, s2);
        assert!(waccon_consume(&view, &mut last).is_some());
    }

    #[test]
    fn zero_lease_is_ignored_by_waccon() {
        let (_mem, view) = fake();
        let rec = input_payload(0, 0, &[0; CELLS], 1, 0, 0);
        view.publish_input(&rec);
        assert_eq!(waccon_consume(&view, &mut 0), None);
    }

    #[test]
    fn leds_stable_only_on_even_sequence() {
        let (_mem, view) = fake();
        assert!(view.read_leds().is_some());
        view.seq(OUTPUT_SEQ).store(3, Ordering::Release);
        assert!(view.read_leds().is_none());
    }

    #[test]
    fn mem_buffer_has_valid_header_and_no_dll() {
        let buf = MemBuffer::default();
        let view = buf.view();
        assert_eq!(view.check_header(), Ok(()));
        assert_eq!(view.io_heartbeat_ms(), 0);
        let rec = input_payload(0, 0, &[1; CELLS], 1, 0, 100);
        let seq = view.publish_input(&rec);
        assert_eq!(view.input_record(), rec);
        assert_eq!(view.io_consumed_sequence(), 0);
        view.simulate_dll_ack();
        assert_eq!(view.io_consumed_sequence(), u64::from(seq));
    }

    #[test]
    fn rejects_bad_view() {
        assert_eq!(
            unsafe { ShmView::from_raw(std::ptr::null_mut(), SIZE) }.unwrap_err(),
            ShmError::View
        );
    }

    #[cfg(windows)]
    #[test]
    fn open_missing_mapping_fails() {
        // A name nothing creates: never touches the real game's buffer.
        assert!(SharedBuffer::open_existing("Local\\WACCA_SLIM_TEST_NO_SUCH_MAPPING").is_err());
    }
}
