use iaimp::{ComInterface, ComPtr};
use parking_lot::{lock_api::RawRwLock as _, RawRwLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    ffi::{OsStr, OsString},
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
    fn to_wide_null(&self) -> Vec<u16>;
}

impl ToWide for str {
    fn to_wide_null(&self) -> Vec<u16> {
        OsStr::new(self).encode_wide().chain(Some(0)).collect()
    }
}

pub struct Static<T>(RwLock<Option<T>>);

impl<T> Static<T> {
    pub const fn new() -> Self {
        Self(RwLock::const_new(RawRwLock::INIT, None))
    }

    pub fn init<U>(&self, value: ComPtr<U>)
    where
        U: ComInterface + ?Sized,
        ComPtr<U>: Into<T>,
    {
        let mut inner = self.0.write();
        assert!(inner.is_none(), "Static is already initialized");
        *inner = Some(value.into());
    }

    pub fn get(&self) -> StaticRef<'_, T> {
        StaticRef(self.0.read())
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get_mut(&self) -> StaticMut<'_, T> {
        StaticMut(self.0.write())
    }
}

unsafe impl<T> Send for Static<T> {}
unsafe impl<T> Sync for Static<T> {}

pub struct StaticRef<'a, T>(RwLockReadGuard<'a, Option<T>>);

impl<T> Deref for StaticRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("Static was not initialized")
    }
}

pub struct StaticMut<'a, T>(RwLockWriteGuard<'a, Option<T>>);

impl<T> Deref for StaticMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("Static was not initialized")
    }
}

impl<T> DerefMut for StaticMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("Static was not initialized")
    }
}
