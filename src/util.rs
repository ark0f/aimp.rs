use iaimp::{ComInterface, ComPtr};
use parking_lot::{lock_api::RawMutex as _, Mutex, MutexGuard, RawMutex};
use std::{
    cell::{Ref, RefCell, RefMut},
    ffi::{OsStr, OsString},
    fmt,
    ops::{Deref, DerefMut},
    os::windows::ffi::OsStrExt,
    ptr,
};
use winapi::um::winuser::{MessageBoxW, MB_OK};

#[macro_export]
macro_rules! msg_box {
    ($($arg:tt)*) => {
        $crate::macro_export::message_box(std::fmt::format(format_args!($($arg)*)))
    };
}

#[doc(hidden)]
pub fn message_box(msg: String) {
    let msg: Vec<u16> = OsString::from(msg).encode_wide().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(ptr::null_mut(), msg.as_ptr(), msg.as_ptr(), MB_OK);
    }
}

pub(crate) trait ToWide {
    fn to_wide(&self) -> Vec<u16>;

    fn to_wide_null(&self) -> Vec<u16>;
}

impl ToWide for str {
    fn to_wide(&self) -> Vec<u16> {
        OsStr::new(self).encode_wide().collect()
    }

    fn to_wide_null(&self) -> Vec<u16> {
        OsStr::new(self).encode_wide().chain(Some(0)).collect()
    }
}

impl ToWide for String {
    fn to_wide(&self) -> Vec<u16> {
        self.as_str().to_wide()
    }

    fn to_wide_null(&self) -> Vec<u16> {
        self.as_str().to_wide_null()
    }
}

pub struct BoxedError(Box<dyn std::error::Error>);

impl BoxedError {
    pub fn new<T: std::error::Error + 'static>(err: T) -> Self {
        Self(Box::new(err))
    }
}

impl std::error::Error for BoxedError {}

impl fmt::Debug for BoxedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for BoxedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[derive(Debug)]
pub enum Service<T> {
    NoLock(RefCell<Option<T>>),
    Lock(Mutex<Option<T>>),
}

impl<T> Service<T> {
    pub(crate) const fn new() -> Self {
        Self::Lock(Mutex::const_new(RawMutex::INIT, None))
    }

    pub(crate) const fn without_lock() -> Self {
        Self::NoLock(RefCell::new(None))
    }

    pub(crate) fn init<U>(&self, value: ComPtr<U>)
    where
        U: ComInterface + ?Sized,
        ComPtr<U>: Into<T>,
    {
        let mut init = ServiceInit::new(self);
        assert!(init.is_none(), "Service is already initialized");
        *init = Some(value.into());
    }

    pub(crate) fn deinit(&self) {
        *ServiceInit::new(self) = None;
    }

    pub fn is_available(&self) -> bool {
        ServiceInit::new(self).is_some()
    }

    pub fn get(&self) -> ServiceRef<'_, T> {
        match self {
            Service::NoLock(cell) => ServiceRef::NoLock(cell.borrow()),
            Service::Lock(mutex) => ServiceRef::Lock(mutex.lock()),
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get_mut(&self) -> ServiceMut<'_, T> {
        match self {
            Service::NoLock(cell) => ServiceMut::NoLock(cell.borrow_mut()),
            Service::Lock(mutex) => ServiceMut::Lock(mutex.lock()),
        }
    }
}

unsafe impl<T> Send for Service<T> {}
unsafe impl<T> Sync for Service<T> {}

#[derive(Debug)]
enum ServiceInit<'a, T> {
    NoLock(RefMut<'a, Option<T>>),
    Lock(MutexGuard<'a, Option<T>>),
}

impl<'a, T> ServiceInit<'a, T> {
    pub fn new(service: &'a Service<T>) -> Self {
        match service {
            Service::NoLock(cell) => ServiceInit::NoLock(cell.borrow_mut()),
            Service::Lock(mutex) => ServiceInit::Lock(mutex.lock()),
        }
    }
}

impl<T> Deref for ServiceInit<'_, T> {
    type Target = Option<T>;

    fn deref(&self) -> &Self::Target {
        match self {
            ServiceInit::NoLock(r) => r,
            ServiceInit::Lock(g) => g,
        }
    }
}

impl<T> DerefMut for ServiceInit<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            ServiceInit::NoLock(r) => r,
            ServiceInit::Lock(g) => g,
        }
    }
}

#[derive(Debug)]
pub enum ServiceRef<'a, T> {
    NoLock(Ref<'a, Option<T>>),
    Lock(MutexGuard<'a, Option<T>>),
}

impl<T> Deref for ServiceRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let opt = match self {
            ServiceRef::NoLock(r) => r.as_ref(),
            ServiceRef::Lock(g) => g.as_ref(),
        };
        opt.expect("Service was not initialized")
    }
}

#[derive(Debug)]
pub enum ServiceMut<'a, T> {
    NoLock(RefMut<'a, Option<T>>),
    Lock(MutexGuard<'a, Option<T>>),
}

impl<T> Deref for ServiceMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let opt = match self {
            ServiceMut::NoLock(r) => r.as_ref(),
            ServiceMut::Lock(g) => g.as_ref(),
        };
        opt.expect("Service was not initialized")
    }
}

impl<T> DerefMut for ServiceMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let opt = match self {
            ServiceMut::NoLock(r) => r.as_mut(),
            ServiceMut::Lock(g) => g.as_mut(),
        };
        opt.expect("Service was not initialized")
    }
}
