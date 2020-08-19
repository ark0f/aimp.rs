pub mod actions;
pub mod core;
pub mod decoders;
mod error;
pub mod file;
pub mod internet;
pub mod test;
#[macro_use]
mod prop_list;
mod plugin;
pub mod stream;
pub mod threading;
mod util;

pub use crate::{
    core::{Core, CORE},
    plugin::{Plugin, PluginInfo},
};
pub use aimp_derive::test;
pub use error::{Error, ErrorKind, Result};
pub use iaimp::{CorePath, PluginCategory, IID};

use crate::{file::VirtualFile, util::ToWide};
use error::HresultExt;
use iaimp::{
    ComInterface, ComPtr, ComRc, IAIMPErrorInfo, IAIMPObjectList, IAIMPProgressCallback,
    IAIMPString, IAIMPVirtualFile, StringCase,
};
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Add, AddAssign},
    os::raw::c_int,
    slice,
};
use winapi::shared::winerror::E_NOINTERFACE;

#[doc(hidden)]
pub mod macro_export {
    pub use crate::{plugin::PluginWrapper, util::message_box};
    pub use aimp_derive::test_fns;
    pub use iaimp::{com_wrapper, ComRc, ComWrapper, IAIMPPlugin, IUnknown};
    pub use tester;
    pub use winapi::shared::winerror::{HRESULT, S_OK};
}

#[macro_export]
macro_rules! main {
    ($entry:ident) => {
        #[no_mangle]
        pub unsafe extern "stdcall" fn AIMPPluginGetHeader(
            header: *mut $crate::macro_export::ComRc<dyn $crate::macro_export::IAIMPPlugin>,
        ) -> $crate::macro_export::HRESULT {
            type Wrapper = $crate::macro_export::PluginWrapper::<$entry>;

            $crate::main!(@test $entry);

            let wrapper =
                $crate::macro_export::com_wrapper!(
                    Wrapper::new() => dyn $crate::macro_export::IAIMPPlugin
                )
                .into_com_rc();
            header.write(wrapper);

            $crate::macro_export::S_OK
        }
    };
    (@test TesterPlugin) => {
        $crate::test::TEST_FNS.with(|fns| {
            #[$crate::macro_export::test_fns]
            fn test_fns() -> Vec<$crate::macro_export::tester::TestDescAndFn> {
                unreachable!()
            }

            *fns.borrow_mut() = Some(test_fns());
        });
    };
    (@test $entry:ident) => {
        std::panic::set_hook(std::boxed::Box::new(|info| $crate::macro_export::message_box(info.to_string())));
    };
}

pub struct AimpString(pub ComRc<dyn IAIMPString>);

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
        self.to_string().fmt(f)
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

impl Extend<AimpString> for AimpString {
    fn extend<T: IntoIterator<Item = AimpString>>(&mut self, iter: T) {
        for item in iter {
            *self += item;
        }
    }
}

impl FromIterator<AimpString> for AimpString {
    fn from_iter<T: IntoIterator<Item = AimpString>>(iter: T) -> Self {
        let mut s = AimpString::default();
        s.extend(iter);
        s
    }
}

impl_prop_accessor!(AimpString);

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

    pub fn set(&mut self, content: &ErrorInfoContent) {
        unsafe {
            self.0.set_info(
                content.code,
                Clone::clone(&content.msg.0),
                content
                    .details
                    .as_ref()
                    .map(|details| Clone::clone(&details.0)),
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ErrorInfoContent {
    pub code: i32,
    pub msg: AimpString,
    pub details: Option<AimpString>,
}

pub trait Object: Sized {
    type Interface: ComInterface + ?Sized;

    fn from_com_rc(rc: ComRc<Self::Interface>) -> Self;

    fn into_com_rc(self) -> ComRc<Self::Interface>;
}

impl Object for AimpString {
    type Interface = dyn IAIMPString;

    fn from_com_rc(rc: ComRc<Self::Interface>) -> Self {
        Self(rc)
    }

    fn into_com_rc(self) -> ComRc<Self::Interface> {
        self.0
    }
}

impl Object for VirtualFile {
    type Interface = dyn IAIMPVirtualFile;

    fn from_com_rc(rc: ComRc<Self::Interface>) -> Self {
        VirtualFile::from_com_rc(rc)
    }

    fn into_com_rc(self) -> ComRc<Self::Interface> {
        self.prop_list.0
    }
}

// TODO: iterator, Index, etc
pub struct ObjectList(ComRc<dyn IAIMPObjectList>);

impl ObjectList {
    pub fn push<T: Object>(&mut self, obj: T) {
        unsafe {
            self.0.add(obj.into_com_rc().cast()).into_result().unwrap();
        }
    }

    pub fn remove<T: Object>(&mut self, idx: u16) {
        unsafe {
            self.0.delete(idx as i32).into_result().unwrap();
        }
    }

    pub fn insert<T: Object>(&mut self, idx: u16, obj: T) {
        unsafe {
            self.0
                .insert(idx as i32, obj.into_com_rc().cast())
                .into_result()
                .unwrap();
        }
    }

    pub fn set<T: Object>(&mut self, idx: u16, obj: T) {
        unsafe {
            self.0
                .set_object(idx as i32, obj.into_com_rc().cast())
                .into_result()
                .unwrap();
        }
    }

    pub fn get<T: Object>(&mut self, idx: u16) -> Option<T> {
        unsafe {
            let mut obj = MaybeUninit::uninit();
            let res =
                self.0
                    .get_object(idx as i32, &T::Interface::IID as *const _, obj.as_mut_ptr());
            if res == E_NOINTERFACE {
                None
            } else {
                res.into_result().unwrap();
                Some(T::from_com_rc(obj.assume_init().cast()))
            }
        }
    }

    pub fn clear(&mut self) {
        unsafe {
            self.0.clear().into_result().unwrap();
        }
    }

    pub fn len(&self) -> u16 {
        unsafe { self.0.get_count() as u16 }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ObjectList {
    fn default() -> Self {
        Self(CORE.get().create().unwrap())
    }
}

pub struct List<T> {
    inner: ObjectList,
    _t: PhantomData<T>,
}

impl<T: Object> List<T> {
    fn from_com_rc(rc: ComRc<dyn IAIMPObjectList>) -> Self {
        Self {
            inner: ObjectList(rc),
            _t: PhantomData,
        }
    }

    pub fn push(&mut self, obj: T) {
        self.inner.push(obj)
    }

    pub fn remove(&mut self, idx: u16) {
        self.inner.remove::<T>(idx)
    }

    pub fn insert(&mut self, idx: u16, obj: T) {
        self.inner.insert(idx, obj)
    }

    pub fn set(&mut self, idx: u16, obj: T) {
        self.inner.set(idx, obj)
    }

    pub fn get(&mut self, idx: u16) -> T {
        self.inner.get(idx).unwrap()
    }

    pub fn clear(&mut self) {
        self.inner.clear()
    }

    pub fn len(&self) -> u16 {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self {
            inner: ObjectList::default(),
            _t: PhantomData,
        }
    }
}

impl<T: Object> FromIterator<T> for List<T> {
    fn from_iter<U: IntoIterator<Item = T>>(iter: U) -> Self {
        let mut list = List::default();
        for item in iter {
            list.push(item);
        }
        list
    }
}

#[macro_export]
macro_rules! list {
    () => { List::default() };
    ($($x:expr),+ $(,)?) => {{
        let mut list = List::default();
        $(
            list.push($x);
        )+
        list
    }};
}

#[derive(Debug, Eq, PartialEq)]
pub struct ProgressCallback(pub(crate) ComPtr<dyn IAIMPProgressCallback>);

impl ProgressCallback {
    pub fn progress(&self, progress: f32) -> bool {
        unsafe {
            let mut canceled = MaybeUninit::uninit();
            self.0.process(progress, canceled.as_mut_ptr());
            canceled.assume_init()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as aimp;
    use crate::test::TesterPlugin;

    const STRING_DATA: &str = "This is a string data";

    #[crate::test]
    fn aimp_string_case() {
        fn word_capital_word(word: &str) -> String {
            let c = word.chars().next().unwrap();
            word.replace(c, &c.to_uppercase().to_string())
        }

        let mut s = AimpString::from(STRING_DATA);
        s.change_case(StringCase::Lower);
        assert_eq!(s.to_string(), STRING_DATA.to_lowercase());
        s.change_case(StringCase::FirstWordWithCapitalLetter);
        assert_eq!(s.to_string().as_bytes()[0], STRING_DATA.as_bytes()[0]);
        s.change_case(StringCase::AllWordsWithCapitalLetter);
        assert_eq!(
            s.to_string(),
            STRING_DATA
                .split(' ')
                .map(word_capital_word)
                .collect::<Vec<String>>()
                .join(" ")
        );
        s.change_case(StringCase::Upper);
        assert_eq!(s.to_string(), STRING_DATA.to_uppercase());
    }

    #[crate::test]
    fn aimp_string_insert_str() {
        let mut s = AimpString::from(STRING_DATA);
        s.insert_str("This ".len(), "are strings");
        assert!(s.to_string().starts_with("This are strings"))
    }

    #[crate::test]
    fn aimp_string_eq() {
        assert_eq!(AimpString::from(STRING_DATA).to_string(), STRING_DATA);
    }

    #[crate::test]
    fn aimp_string_extend() {
        let mut s = AimpString::default();
        s.extend(vec!["A".into(), "B".into(), "C".into()]);
        assert_eq!(s.to_string(), "ABC");
    }

    #[crate::test]
    fn error_info() {
        let content = ErrorInfoContent {
            code: 123,
            msg: AimpString::default(),
            details: Some(AimpString::default()),
        };
        let mut info = ErrorInfo::default();
        info.set(&content);
        let returned_content = info.get();
        assert_eq!(content, returned_content);
        let _ = info.get_formatted();
    }

    #[crate::test]
    fn object_list() {
        let mut list = ObjectList::default();

        assert!(list.is_empty());

        list.push(AimpString::from(STRING_DATA));
        list.push(AimpString::from("123"));

        let a: AimpString = list.get(0).unwrap();
        assert_eq!(a.to_string(), STRING_DATA);

        assert!(!list.is_empty());
        assert_eq!(list.len(), 2);

        list.remove::<AimpString>(0);
        let a: AimpString = list.get(0).unwrap();
        assert_eq!(a.to_string(), "123");
        assert!(!list.is_empty());
        assert_eq!(list.len(), 1);

        list.clear();
        assert!(list.is_empty());
    }

    crate::main!(TesterPlugin);
}
