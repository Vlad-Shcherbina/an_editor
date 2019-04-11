// Pretty much a subset of
// https://github.com/retep998/wio-rs/blob/master/src/com.rs

use std::ptr::NonNull;
use std::ops::Deref;
use winapi::Interface;
use winapi::um::unknwnbase::IUnknown;

pub struct ComPtr<T>(NonNull<T>) where T: Interface;

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

impl<T> Clone for ComPtr<T> where T: Interface {
    fn clone(&self) -> Self {
        unsafe {
            (*(self.0.as_ptr() as *mut IUnknown)).AddRef();
            ComPtr::from_raw(self.as_raw())
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

    fn into_raw(self) -> *mut T {
        let p = self.0.as_ptr();
        std::mem::forget(self);
        p
    }

    pub fn up<U: Interface>(self) -> ComPtr<U> where T: Deref<Target=U> {
        unsafe { ComPtr::from_raw(self.into_raw() as *mut U) }
    }
}
