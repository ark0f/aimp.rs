pub mod core;
mod error;
pub mod internet;
pub mod stream;
pub mod threading;
mod util;
mod wrapper;

pub use crate::core::{Core, CORE};
pub use error::{Error, ErrorKind, Result};
pub use iaimp::{CorePath, PluginCategory};

use crate::util::ToWide;
use error::HresultExt;
use iaimp::{
    ComInterface, ComPtr, ComRc, IAIMPErrorInfo, IAIMPPropertyList, IAIMPString, StringCase, IID,
};
use std::{
    cmp::Ordering,
    error::Error as StdError,
    fmt,
    hash::{Hash, Hasher},
    mem::MaybeUninit,
    ops::{Add, AddAssign},
    os::raw::c_int,
    result::Result as StdResult,
    slice,
};

#[doc(hidden)]
pub mod macro_export {
    pub use crate::{util::message_box, wrapper::Wrapper};
    pub use iaimp::{com_wrapper, ComWrapper, IAIMPPlugin, IUnknown};
    pub use winapi::shared::winerror::{HRESULT, S_OK};
}

#[macro_export]
macro_rules! main {
    ($entry:ident) => {
        #[no_mangle]
        pub unsafe extern "stdcall" fn AIMPPluginGetHeader(
            header: *mut *mut std::ffi::c_void,
        ) -> $crate::macro_export::HRESULT {
            use $crate::macro_export::IUnknown;

            type Wrapper = $crate::macro_export::Wrapper::<$entry>;

            std::panic::set_hook(Box::new(|info| msg_box!("{}", info)));

            let wrapper = $crate::macro_export::com_wrapper!(
                Wrapper::new() =>
                Wrapper: $crate::macro_export::IAIMPPlugin
            );
            wrapper.add_ref();
            *header = Box::into_raw(Box::new(wrapper)) as _;

            $crate::macro_export::S_OK
        }
    };
}

pub trait Plugin: Sized {
    const INFO: PluginInfo;

    type Error: StdError;

    fn new() -> StdResult<Self, Self::Error>;

    fn finish(self) -> StdResult<(), Self::Error>;
}

pub struct PluginInfo {
    pub name: &'static str,
    pub author: &'static str,
    pub short_description: &'static str,
    pub full_description: Option<&'static str>,
    pub category: PluginCategory,
}

pub struct AimpString(ComRc<dyn IAIMPString>);

impl AimpString {
    /// # Safety
    ///
    /// This method is unsafe because caller can make UTF-16 data invalid
    pub unsafe fn as_bytes_mut(&mut self) -> &mut [u16] {
        slice::from_raw_parts_mut(self.0.get_data(), self.0.get_length() as _)
    }

    pub fn as_bytes(&self) -> &[u16] {
        unsafe { slice::from_raw_parts(self.0.get_data(), self.0.get_length() as _) }
    }

    pub fn change_case(&mut self, case: StringCase) {
        unsafe { self.0.change_case(case).into_result().unwrap() }
    }

    /// # Safety
    ///
    /// Valid UTF-16 encoded data is required
    pub unsafe fn add2(&mut self, bytes: &[u16]) -> Result<()> {
        self.0.add2(bytes.as_ptr(), bytes.len() as _).into_result()
    }

    pub fn compare(&self, other: &Self, ignore_case: bool) -> Ordering {
        unsafe {
            let other = <ComRc<_> as Clone>::clone(&other.0);
            let mut res = MaybeUninit::uninit();
            self.0
                .compare(other, res.as_mut_ptr(), ignore_case)
                .into_result()
                .unwrap();
            let res = res.assume_init();
            Self::match_comparison(res)
        }
    }

    /// # Safety
    ///
    /// Valid UTF-16 encoded data is required
    pub unsafe fn compare2(&self, data: &[u16], ignore_case: bool) -> Result<Ordering> {
        let mut res = MaybeUninit::uninit();
        self.0
            .compare2(
                data.as_ptr(),
                data.len() as _,
                res.as_mut_ptr(),
                ignore_case,
            )
            .into_result()?;
        let res = res.assume_init();
        Ok(Self::match_comparison(res))
    }

    fn match_comparison(res: c_int) -> Ordering {
        match res {
            -1 => Ordering::Greater,
            0 => Ordering::Equal,
            1 => Ordering::Less,
            _ => unreachable!(),
        }
    }

    /// # Safety
    ///
    /// Valid UTF-16 encoded data is required
    pub unsafe fn set_data(&mut self, data: &[u16]) -> Result<()> {
        self.0
            .set_data(data.as_ptr(), data.len() as _)
            .into_result()
    }

    pub fn insert_str<T: Into<AimpString>>(&mut self, idx: usize, string: T) {
        unsafe {
            self.0
                .insert(idx as _, string.into().0)
                .into_result()
                .unwrap()
        }
    }
}

impl Default for AimpString {
    fn default() -> Self {
        Self(CORE.get().create::<dyn IAIMPString>().unwrap())
    }
}

impl From<ComRc<dyn IAIMPString>> for AimpString {
    fn from(rc: ComRc<dyn IAIMPString>) -> Self {
        Self(rc)
    }
}

impl From<&str> for AimpString {
    fn from(s: &str) -> Self {
        let data: Vec<u16> = s.to_wide();
        let mut s = Self::default();
        unsafe { s.set_data(&data).unwrap() }
        s
    }
}

impl From<String> for AimpString {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl AsRef<[u16]> for AimpString {
    fn as_ref(&self) -> &[u16] {
        self.as_bytes()
    }
}

impl fmt::Display for AimpString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = self.as_bytes();
        let s = String::from_utf16(s).unwrap();
        s.fmt(f)
    }
}

impl fmt::Debug for AimpString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("AimpString").field(&self.0.as_raw()).finish()
    }
}

impl Clone for AimpString {
    fn clone(&self) -> Self {
        unsafe {
            let mut s = MaybeUninit::uninit();
            IAIMPString::clone(&self.0, s.as_mut_ptr())
                .into_result()
                .unwrap();
            Self(s.assume_init())
        }
    }
}

impl Hash for AimpString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state)
    }
}

impl PartialEq for AimpString {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for AimpString {}

impl PartialOrd for AimpString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AimpString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other, false)
    }
}

impl Add for AimpString {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self.add_assign(rhs);
        self
    }
}

impl AddAssign for AimpString {
    fn add_assign(&mut self, rhs: Self) {
        unsafe { self.0.add(rhs.0).into_result().unwrap() }
    }
}

#[derive(Debug)]
pub struct ErrorInfo(ComRc<dyn IAIMPErrorInfo>);

impl Default for ErrorInfo {
    fn default() -> Self {
        Self(CORE.get().create::<dyn IAIMPErrorInfo>().unwrap())
    }
}

impl ErrorInfo {
    pub fn get(&self) -> ErrorInfoContent {
        unsafe {
            let mut code = MaybeUninit::uninit();
            let mut msg = MaybeUninit::uninit();
            let mut details = MaybeUninit::uninit();

            self.0
                .get_info(code.as_mut_ptr(), msg.as_mut_ptr(), details.as_mut_ptr())
                .into_result()
                .unwrap();

            ErrorInfoContent {
                code: code.assume_init(),
                msg: AimpString(msg.assume_init()),
                details: details.assume_init().map(AimpString),
            }
        }
    }

    pub fn set(&mut self, content: ErrorInfoContent) {
        unsafe {
            self.0.set_info(
                content.code,
                content.msg.0,
                content.details.map(|details| details.0),
            )
        }
    }

    pub fn get_formatted(&self) -> AimpString {
        unsafe {
            let mut s = MaybeUninit::<ComRc<dyn IAIMPString>>::uninit();
            self.0
                .get_info_formatted(s.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(s.assume_init())
        }
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.get_formatted().fmt(f)
    }
}

#[derive(Debug)]
pub struct ErrorInfoContent {
    pub code: i32,
    pub msg: AimpString,
    pub details: Option<AimpString>,
}

pub struct PropertyList(ComPtr<dyn IAIMPPropertyList>);

impl PropertyList {
    pub fn get<T: PropertyListAccessor>(&self, id: i32) -> T {
        T::get(id, self)
    }

    pub fn update(&mut self) -> PropertyListGuard {
        unsafe {
            self.0.begin_update();
        }
        PropertyListGuard(self)
    }
}

impl<T: ComInterface + IAIMPPropertyList + ?Sized> From<ComPtr<T>> for PropertyList {
    fn from(ptr: ComPtr<T>) -> Self {
        unsafe { Self(ptr.cast()) }
    }
}

pub struct PropertyListGuard<'a>(&'a mut PropertyList);

impl PropertyListGuard<'_> {
    pub fn set<T: PropertyListAccessor>(self, id: i32, prop: T) -> Self {
        prop.set(id, self.0);
        self
    }
}

impl Drop for PropertyListGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.0).0.end_update();
        }
    }
}

pub trait PropertyListAccessor: Sized {
    fn get(id: i32, list: &PropertyList) -> Self;

    fn set(self, id: i32, list: &mut PropertyList);
}

macro_rules! impl_prop_get_set {
    ($prop:ty, $get:ident, $set:ident) => {
        impl PropertyListAccessor for $prop {
            fn get(id: i32, list: &PropertyList) -> Self {
                unsafe {
                    let mut value = MaybeUninit::uninit();
                    (list.0).$get(id, value.as_mut_ptr()).into_result().unwrap();
                    value.assume_init()
                }
            }

            fn set(self, id: i32, list: &mut PropertyList) {
                unsafe {
                    (list.0).$set(id, self).into_result().unwrap();
                }
            }
        }
    };
}

impl_prop_get_set!(f64, get_value_as_float, set_value_as_float);
impl_prop_get_set!(i32, get_value_as_int32, set_value_as_int32);
impl_prop_get_set!(i64, get_value_as_int64, set_value_as_int64);

impl<T: ComInterface + ?Sized> PropertyListAccessor for ComRc<T> {
    fn get(id: i32, list: &PropertyList) -> Self {
        unsafe {
            let mut value = MaybeUninit::uninit();
            list.0
                .get_value_as_object(id, &T::IID as *const IID as *const _, value.as_mut_ptr())
                .into_result()
                .unwrap();
            value.assume_init().cast()
        }
    }

    fn set(self, id: i32, list: &mut PropertyList) {
        unsafe {
            list.0
                .set_value_as_object(id, self.cast())
                .into_result()
                .unwrap();
        }
    }
}

impl PropertyListAccessor for AimpString {
    fn get(id: i32, list: &PropertyList) -> Self {
        Self(ComRc::<dyn IAIMPString>::get(id, list))
    }

    fn set(self, id: i32, list: &mut PropertyList) {
        self.0.set(id, list)
    }
}
