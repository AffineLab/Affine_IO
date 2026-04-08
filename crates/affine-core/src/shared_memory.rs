use std::ffi::OsStr;
use std::marker::PhantomData;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject,
};

pub struct SharedPage<T> {
    mapping: HANDLE,
    mutex: HANDLE,
    view: *mut T,
    _marker: PhantomData<T>,
}

unsafe impl<T> Send for SharedPage<T> {}
unsafe impl<T> Sync for SharedPage<T> {}

impl<T: Copy + Default> SharedPage<T> {
    pub fn create(mapping_name: &str, mutex_name: &str) -> Option<Self> {
        let mapping_name = to_wide(mapping_name);
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                null(),
                PAGE_READWRITE,
                0,
                size_of::<T>() as u32,
                mapping_name.as_ptr(),
            )
        };
        if mapping.is_null() {
            return None;
        }

        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        let mutex_name = to_wide(mutex_name);
        let mutex = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
        if mutex.is_null() {
            unsafe {
                CloseHandle(mapping);
            }
            return None;
        }

        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size_of::<T>()) };
        if view.Value.is_null() {
            unsafe {
                CloseHandle(mutex);
                CloseHandle(mapping);
            }
            return None;
        }

        let page = Self {
            mapping,
            mutex,
            view: view.Value.cast(),
            _marker: PhantomData,
        };

        if !existed {
            page.write(T::default());
        }

        Some(page)
    }

    pub fn read(&self) -> T {
        self.lock();
        let value = unsafe { *self.view };
        self.unlock();
        value
    }

    pub fn write(&self, value: T) {
        self.lock();
        unsafe {
            *self.view = value;
        }
        self.unlock();
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.lock();
        let result = unsafe { f(&mut *self.view) };
        self.unlock();
        result
    }

    fn lock(&self) {
        unsafe {
            WaitForSingleObject(self.mutex, INFINITE);
        }
    }

    fn unlock(&self) {
        unsafe {
            ReleaseMutex(self.mutex);
        }
    }
}

impl<T> Drop for SharedPage<T> {
    fn drop(&mut self) {
        unsafe {
            if !self.view.is_null() {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view.cast(),
                });
            }
            if !self.mutex.is_null() {
                CloseHandle(self.mutex);
            }
            if !self.mapping.is_null() {
                CloseHandle(self.mapping);
            }
        }
        self.view = null_mut();
        self.mutex = null_mut();
        self.mapping = null_mut();
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
