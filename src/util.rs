use std::{
    ffi::{OsStr, OsString},
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

pub struct Static<T>(Option<T>);

impl<T> Static<T> {
    pub const fn new() -> Self {
        Self(None)
    }

    pub fn init(&mut self, value: T) {
        assert!(self.0.is_none(), "Cell initialized twice");
        self.0 = Some(value);
    }

    pub fn get(&self) -> &T {
        self.0.as_ref().expect("Cell was not initialized")
    }
}

unsafe impl<T> Send for Static<T> {}
unsafe impl<T> Sync for Static<T> {}
