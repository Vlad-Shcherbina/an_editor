// Pretty much a subset of
// https://github.com/retep998/wio-rs/blob/master/src/com.rs

use std::ptr::NonNull;
use std::ops::Deref;
use winapi::Interface;
use winapi::um::unknwnbase::IUnknown;

pub struct ComPtr<T>(pub NonNull<T>) where T: Interface;

impl<T> ComPtr<T> where T: Interface {
    pub unsafe fn from_raw(ptr: *mut T) -> ComPtr<T> {
        ComPtr(NonNull::new(ptr).unwrap())
    }
}

impl<T> Drop for ComPtr<T> where T: Interface {
    fn drop(&mut self) {
        unsafe {
            (*(self.0.as_ptr() as *mut IUnknown)).Release();
        }
    }
}

impl<T> Deref for ComPtr<T> where T: Interface {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe {
            &*self.0.as_ptr()
        }
    }
}

impl<T> ComPtr<T> where T: Interface {
    pub fn as_raw(&self) -> *mut T {
        self.0.as_ptr()
    }

    pub fn as_up_raw<U: Interface>(&self) -> *mut U where T: Deref<Target=U> {
        self.as_raw() as *mut U
    }
}
