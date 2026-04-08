use std::ffi::OsStr;
use std::iter::once;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SPDRP_FRIENDLYNAME,
    SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    SetupDiGetDeviceRegistryPropertyW,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_DEVICE_NOT_CONNECTED, ERROR_INVALID_HANDLE, ERROR_NO_MORE_ITEMS,
    GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, ReadFile, WriteFile,
};

const GUID_DEVINTERFACE_COMPORT: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x86e0d1e0,
    data2: 0x8089,
    data3: 0x11d0,
    data4: [0x9c, 0xe4, 0x08, 0x00, 0x3e, 0x30, 0x1f, 0x73],
};

pub struct SerialPort {
    handle: HANDLE,
}

impl Default for SerialPort {
    fn default() -> Self {
        Self {
            handle: INVALID_HANDLE_VALUE,
        }
    }
}

impl SerialPort {
    pub fn is_open(&self) -> bool {
        self.handle != null_mut() && self.handle != INVALID_HANDLE_VALUE
    }

    pub fn open(&mut self, path: &str, baud: u32) -> bool {
        let wide = to_wide(path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
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
        dcb.StopBits = ONESTOPBIT as u8;
        dcb.Parity = NOPARITY as u8;
        if unsafe { SetCommState(handle, &dcb) } == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return false;
        }

        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: 20,
            ReadTotalTimeoutConstant: 20,
            ReadTotalTimeoutMultiplier: 5,
            WriteTotalTimeoutConstant: 50,
            WriteTotalTimeoutMultiplier: 5,
        };

        unsafe {
            SetCommTimeouts(handle, &timeouts);
        }

        self.handle = handle;
        true
    }

    pub fn close(&mut self) {
        if self.is_open() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
        self.handle = INVALID_HANDLE_VALUE;
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Option<usize> {
        if !self.is_open() {
            return None;
        }

        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut read,
                null_mut(),
            )
        };

        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_INVALID_HANDLE || err == ERROR_DEVICE_NOT_CONNECTED {
                return None;
            }
            return Some(0);
        }

        Some(read as usize)
    }

    pub fn write(&mut self, buf: &[u8]) -> bool {
        if !self.is_open() {
            return false;
        }

        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                self.handle,
                buf.as_ptr().cast(),
                buf.len() as u32,
                &mut written,
                null_mut(),
            )
        };

        ok != 0 && written as usize == buf.len()
    }
}

impl Drop for SerialPort {
    fn drop(&mut self) {
        self.close();
    }
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
