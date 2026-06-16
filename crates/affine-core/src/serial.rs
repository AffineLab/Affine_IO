use std::ffi::{OsStr, c_void};
use std::iter::once;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SPDRP_FRIENDLYNAME, SPDRP_HARDWAREID,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, SetupDiGetDeviceRegistryPropertyW,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_DEVICE_NOT_CONNECTED, ERROR_INVALID_HANDLE, ERROR_IO_PENDING,
    ERROR_NO_MORE_ITEMS, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent};

const GUID_DEVINTERFACE_COMPORT: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x86e0d1e0,
    data2: 0x8089,
    data3: 0x11d0,
    data4: [0x9c, 0xe4, 0x08, 0x00, 0x3e, 0x30, 0x1f, 0x73],
};

pub struct SerialPort {
    handle: AtomicPtr<c_void>,
    read_state: Mutex<OverlappedState>,
    write_state: Mutex<OverlappedState>,
}

unsafe impl Send for SerialPort {}
unsafe impl Sync for SerialPort {}

struct OverlappedState {
    event: HANDLE,
    overlapped: OVERLAPPED,
}

impl Default for OverlappedState {
    fn default() -> Self {
        Self {
            event: null_mut(),
            overlapped: unsafe { zeroed() },
        }
    }
}

impl Default for SerialPort {
    fn default() -> Self {
        Self {
            handle: AtomicPtr::new(INVALID_HANDLE_VALUE),
            read_state: Mutex::new(OverlappedState::default()),
            write_state: Mutex::new(OverlappedState::default()),
        }
    }
}

impl SerialPort {
    pub fn is_open(&self) -> bool {
        let handle = self.handle.load(Ordering::SeqCst) as HANDLE;
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    pub fn open(&mut self, path: &str, baud: u32) -> bool {
        self.close();

        let wide = to_wide(path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return false;
        }

        if !self.ensure_events() {
            unsafe {
                CloseHandle(handle);
            }
            return false;
        }

        let mut dcb: DCB = unsafe { zeroed() };
        dcb.DCBlength = size_of::<DCB>() as u32;

        if unsafe { GetCommState(handle, &mut dcb) } == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return false;
        }

        dcb.BaudRate = baud;
        dcb.ByteSize = 8;
        dcb.StopBits = ONESTOPBIT;
        dcb.Parity = NOPARITY;
        if unsafe { SetCommState(handle, &dcb) } == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return false;
        }

        if !Self::apply_timeouts(
            handle,
            COMMTIMEOUTS {
                ReadIntervalTimeout: 20,
                ReadTotalTimeoutConstant: 20,
                ReadTotalTimeoutMultiplier: 5,
                WriteTotalTimeoutConstant: 50,
                WriteTotalTimeoutMultiplier: 5,
            },
        ) {
            unsafe {
                CloseHandle(handle);
            }
            return false;
        }

        self.handle.store(handle, Ordering::SeqCst);
        true
    }

    pub fn set_timeouts(
        &self,
        read_interval_timeout: u32,
        read_total_timeout_constant: u32,
        read_total_timeout_multiplier: u32,
        write_total_timeout_constant: u32,
        write_total_timeout_multiplier: u32,
    ) -> bool {
        if !self.is_open() {
            return false;
        }

        let handle = self.handle.load(Ordering::SeqCst) as HANDLE;

        Self::apply_timeouts(
            handle,
            COMMTIMEOUTS {
                ReadIntervalTimeout: read_interval_timeout,
                ReadTotalTimeoutConstant: read_total_timeout_constant,
                ReadTotalTimeoutMultiplier: read_total_timeout_multiplier,
                WriteTotalTimeoutConstant: write_total_timeout_constant,
                WriteTotalTimeoutMultiplier: write_total_timeout_multiplier,
            },
        )
    }

    pub fn close(&self) {
        let handle = self.handle.swap(INVALID_HANDLE_VALUE, Ordering::SeqCst) as HANDLE;
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            unsafe {
                CancelIoEx(handle, null());
                CloseHandle(handle);
            }
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Option<usize> {
        let handle = self.handle.load(Ordering::SeqCst) as HANDLE;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut state = self.read_state.lock().unwrap();
        if !prepare_overlapped(&mut state) {
            return None;
        }

        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut read,
                &mut state.overlapped,
            )
        };

        if ok == 0 {
            let mut err = unsafe { GetLastError() };
            if err == ERROR_IO_PENDING {
                if unsafe {
                    GetOverlappedResult(handle, &mut state.overlapped, &mut read, 1)
                } != 0
                {
                    return Some(read as usize);
                }
                err = unsafe { GetLastError() };
            }

            if err == ERROR_INVALID_HANDLE
                || err == ERROR_DEVICE_NOT_CONNECTED
                || err == ERROR_OPERATION_ABORTED
            {
                return None;
            }
            return Some(0);
        }

        Some(read as usize)
    }

    pub fn write(&self, buf: &[u8]) -> bool {
        let handle = self.handle.load(Ordering::SeqCst) as HANDLE;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }

        let mut state = self.write_state.lock().unwrap();
        if !prepare_overlapped(&mut state) {
            return false;
        }

        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle,
                buf.as_ptr().cast(),
                buf.len() as u32,
                &mut written,
                &mut state.overlapped,
            )
        };

        if ok == 0 {
            let mut err = unsafe { GetLastError() };
            if err == ERROR_IO_PENDING {
                if unsafe {
                    GetOverlappedResult(handle, &mut state.overlapped, &mut written, 1)
                } != 0
                {
                    return written as usize == buf.len();
                }
                err = unsafe { GetLastError() };
            }

            if err == ERROR_INVALID_HANDLE
                || err == ERROR_DEVICE_NOT_CONNECTED
                || err == ERROR_OPERATION_ABORTED
            {
                return false;
            }
            return false;
        }

        ok != 0 && written as usize == buf.len()
    }

    fn apply_timeouts(handle: HANDLE, timeouts: COMMTIMEOUTS) -> bool {
        unsafe { SetCommTimeouts(handle, &timeouts) != 0 }
    }

    fn ensure_events(&self) -> bool {
        let mut read_state = self.read_state.lock().unwrap();
        if !ensure_event(&mut read_state) {
            return false;
        }
        drop(read_state);

        let mut write_state = self.write_state.lock().unwrap();
        ensure_event(&mut write_state)
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        self.close();

        if let Ok(mut read_state) = self.read_state.lock() {
            if !read_state.event.is_null() {
                unsafe {
                    CloseHandle(read_state.event);
                }
                read_state.event = null_mut();
            }
        }

        if let Ok(mut write_state) = self.write_state.lock() {
            if !write_state.event.is_null() {
                unsafe {
                    CloseHandle(write_state.event);
                }
                write_state.event = null_mut();
            }
        }
    }
}

fn ensure_event(state: &mut OverlappedState) -> bool {
    if state.event.is_null() {
        state.event = unsafe { CreateEventW(null_mut(), 1, 0, null()) };
        if state.event.is_null() {
            return false;
        }
    }

    state.overlapped = unsafe { zeroed() };
    state.overlapped.hEvent = state.event;
    true
}

fn prepare_overlapped(state: &mut OverlappedState) -> bool {
    if state.event.is_null() && !ensure_event(state) {
        return false;
    }

    unsafe {
        ResetEvent(state.event);
    }
    state.overlapped = unsafe { zeroed() };
    state.overlapped.hEvent = state.event;
    true
}

pub fn find_com_port(vid: u16, pid: u16) -> Option<String> {
    let info: isize = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVINTERFACE_COMPORT,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };

    if info == -1 {
        return None;
    }

    let mut index = 0;
    let mut found = None;

    loop {
        let mut if_data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..unsafe { zeroed() }
        };

        let ok = unsafe {
            SetupDiEnumDeviceInterfaces(
                info,
                null_mut(),
                &GUID_DEVINTERFACE_COMPORT,
                index,
                &mut if_data,
            )
        };

        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            index += 1;
            continue;
        }

        let mut dev_info = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..unsafe { zeroed() }
        };
        let mut required = 0u32;

        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                info,
                &if_data,
                null_mut(),
                0,
                &mut required,
                null_mut(),
            );
        }

        if required == 0 {
            index += 1;
            continue;
        }

        let mut detail_data = vec![0u8; required as usize];
        let detail = detail_data.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }

        let ok = unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                info,
                &if_data,
                detail,
                required,
                null_mut(),
                &mut dev_info,
            )
        };

        if ok != 0 {
            let mut reg_type = 0u32;
            let mut hwid = vec![0u8; 512];

            let hwid_ok = unsafe {
                SetupDiGetDeviceRegistryPropertyW(
                    info,
                    &dev_info,
                    SPDRP_HARDWAREID,
                    &mut reg_type,
                    hwid.as_mut_ptr(),
                    hwid.len() as u32,
                    null_mut(),
                )
            };

            if hwid_ok != 0 && match_hwid(&hwid, vid, pid) {
                let mut friendly = vec![0u8; 256];
                let prop_ok = unsafe {
                    SetupDiGetDeviceRegistryPropertyW(
                        info,
                        &dev_info,
                        SPDRP_FRIENDLYNAME,
                        &mut reg_type,
                        friendly.as_mut_ptr(),
                        friendly.len() as u32,
                        null_mut(),
                    )
                };

                if prop_ok != 0 {
                    found = parse_com_name(&friendly);
                    if found.is_some() {
                        break;
                    }
                }
            }
        }

        index += 1;
    }

    unsafe {
        SetupDiDestroyDeviceInfoList(info);
    }

    found
}

fn match_hwid(raw: &[u8], vid: u16, pid: u16) -> bool {
    let wide: Vec<u16> = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    let vid_marker = format!("VID_{vid:04X}");
    let pid_marker = format!("PID_{pid:04X}");

    let mut start = 0usize;

    while start < wide.len() {
        let end = wide[start..]
            .iter()
            .position(|&unit| unit == 0)
            .map(|offset| start + offset)
            .unwrap_or(wide.len());

        if end == start {
            break;
        }

        let part = String::from_utf16_lossy(&wide[start..end]);
        if part.contains(&vid_marker) && part.contains(&pid_marker) {
            return true;
        }

        start = end + 1;
    }

    false
}

fn parse_com_name(raw: &[u8]) -> Option<String> {
    let wide: Vec<u16> = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    let name = String::from_utf16_lossy(&wide);
    let index = name.find("COM")?;
    let suffix = &name[index..];
    let com_name: String = suffix
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect();

    if com_name.is_empty() {
        None
    } else {
        Some(format!("\\\\.\\{com_name}"))
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(once(0)).collect()
}
