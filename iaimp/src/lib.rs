#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

use std::{
    cell::Cell,
    fmt,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    os::raw::{c_double, c_float, c_int, c_uchar, c_void},
    ptr,
    ptr::NonNull,
    time::Duration,
};

use bitflags::bitflags;
use std::time::SystemTime;
use winapi::{
    shared::{
        basetsd::DWORD_PTR,
        guiddef::GUID as WinGUID,
        minwindef::{BOOL, DWORD, HMODULE, WORD},
        windef::{HBITMAP, HDC, HWND, RECT, SIZE},
        winerror::{E_NOINTERFACE, HRESULT as WinHRESULT, NOERROR},
    },
    um::{
        oaidl::VARIANT,
        wingdi::RGBQUAD,
        winnt::{PWCHAR, WCHAR},
        winuser::*,
    },
}; // keys

// COM code based on https://github.com/microsoft/com-rs

pub struct GUID(WinGUID);
pub type IID = GUID;
pub type REFIID = *const IID;

impl PartialEq for GUID {
    fn eq(&self, other: &Self) -> bool {
        self.0.Data1 == other.0.Data1
            && self.0.Data2 == other.0.Data2
            && self.0.Data3 == other.0.Data3
            && self.0.Data4 == other.0.Data4
    }
}

impl Eq for GUID {}

impl Deref for GUID {
    type Target = WinGUID;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait ComInterface {
    const IID: IID;
    type Super: ComInterface + ?Sized;
    type VTable;

    fn check_inheritance_chain(riid: &IID) -> bool {
        riid == &Self::IID
            || (Self::IID != <dyn IUnknown as ComInterface>::IID
                && <Self::Super as ComInterface>::check_inheritance_chain(riid))
    }

    fn check_inheritance_chain_by_ref(&self, riid: &IID) -> bool {
        Self::check_inheritance_chain(riid)
    }
}

#[repr(transparent)]
pub struct ComPtr<T: ComInterface + ?Sized> {
    inner: NonNull<*mut T::VTable>,
}

impl<T: ComInterface + ?Sized> ComPtr<T> {
    pub fn from_ptr(ptr: *mut *mut T::VTable) -> Self {
        Self {
            inner: NonNull::new(ptr).expect("Pointer is not null"),
        }
    }

    pub unsafe fn cast<U: ComInterface + ?Sized>(self) -> ComPtr<U> {
        mem::transmute(self)
    }
}

impl<T: ComInterface + ?Sized> fmt::Debug for ComPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl<T: ComInterface + ?Sized> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}

impl<T: ComInterface + ?Sized> PartialEq for ComPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.as_ptr() == other.inner.as_ptr()
    }
}

impl<T: ComInterface + ?Sized> Eq for ComPtr<T> {}

impl<T: ComInterface + ?Sized> ComInterface for ComPtr<T> {
    const IID: IID = T::IID;
    type Super = T::Super;
    type VTable = T::VTable;
}

pub struct ComRc<T: ComInterface + ?Sized>(ComPtr<T>);

impl<T: ComInterface + ?Sized> ComRc<T> {
    pub fn from_ptr(ptr: *mut *mut T::VTable) -> Self {
        Self(ComPtr::from_ptr(ptr))
    }

    pub unsafe fn cast<U: ComInterface + ?Sized>(self) -> ComRc<U> {
        mem::transmute(self)
    }

    pub fn as_raw(&self) -> ComPtr<T> {
        self.0.clone()
    }
}

impl<T: ComInterface + ?Sized> PartialEq for ComRc<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: ComInterface + ?Sized> Eq for ComRc<T> {}

impl<T: ComInterface + ?Sized> fmt::Debug for ComRc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<T: ComInterface + ?Sized> Clone for ComRc<T> {
    fn clone(&self) -> Self {
        let ptr = self.0.clone();
        unsafe {
            ptr.add_ref();
        }
        Self(ptr)
    }
}

impl<T: ComInterface + ?Sized> Drop for ComRc<T> {
    fn drop(&mut self) {
        unsafe {
            self.0.release();
        }
    }
}

impl<T: ComInterface + ?Sized> ComInterface for ComRc<T> {
    const IID: IID = T::IID;
    type Super = T::Super;
    type VTable = T::VTable;
}

impl<T: ComInterface + ?Sized> From<ComPtr<T>> for ComRc<T> {
    fn from(ptr: ComPtr<T>) -> Self {
        Self(ptr)
    }
}

pub trait ComProdInterface<T, P, O> {
    type VTable;

    fn new_vtable() -> Self::VTable;
}

pub trait ComVTable {
    type Interface: ComInterface + ?Sized;
}

pub trait ComOffset {
    const VALUE: usize;
}

macro_rules! com_offset {
    ($name:ident = $value:tt) => {
        pub struct $name;

        impl ComOffset for $name {
            const VALUE: usize = $value;
        }
    };
}

com_offset!(ZeroOffset = 0);
com_offset!(OneOffset = 1);
com_offset!(TwoOffset = 2);
com_offset!(ThreeOffset = 3);
com_offset!(FourOffset = 4);
com_offset!(FiveOffset = 5);
com_offset!(SixOffset = 6);

pub trait ComPointers: fmt::Debug + Sized {
    fn query_interface(&self, riid: &IID) -> Option<*mut c_void>;

    fn dealloc(&self);
}

pub trait ComPointersAlloc<Type>: ComPointers {
    fn alloc() -> Self;
}

macro_rules! com_pointers {
    ($( $fields:tt: $generics:ident => $offset:ident ),+) => {
        impl<$( $generics: ComVTable ),+> ComPointers for ($( *mut $generics, )+) {
            fn query_interface(&self, riid: &IID) -> Option<*mut c_void> {
                if <dyn IUnknown as ComInterface>::IID == *riid {
                    Some(&self.0 as *const _ as *mut c_void)
                } else
                $(
                    if $generics::Interface::check_inheritance_chain(riid) {
                        Some(&self.$fields as *const _ as *mut c_void)
                    } else
                )+ {
                    None
                }
            }

            fn dealloc(&self) {
                $(
                    unsafe { Box::from_raw(self.$fields,) };
                )+
            }
        }

        impl<Type, $( $generics ),+> ComPointersAlloc<Type> for ($( *mut $generics, )+)
        where
            $( $generics: ComVTable, )+
            $( $generics::Interface: ComProdInterface<Type, Self, $offset>, )+
        {
            fn alloc() -> Self {
                (
                    $(
                        Box::into_raw(Box::new(<$generics::Interface as $crate::ComProdInterface<Type, Self, $offset>>::new_vtable())) as *mut _,
                    )+
                )
            }
        }
    };
}

com_pointers!(0: T => ZeroOffset);
com_pointers!(0: T => ZeroOffset, 1: U => OneOffset);
com_pointers!(0: A => ZeroOffset, 1: B => OneOffset, 2: C => TwoOffset);
com_pointers!(0: A => ZeroOffset, 1: B => OneOffset, 2: C => TwoOffset, 3: D => ThreeOffset);
com_pointers!(0: A => ZeroOffset, 1: B => OneOffset, 2: C => TwoOffset, 3: D => ThreeOffset, 4: E => FourOffset);
com_pointers!(0: A => ZeroOffset, 1: B => OneOffset, 2: C => TwoOffset, 3: D => ThreeOffset, 4: E => FourOffset, 5: F => FiveOffset);
com_pointers!(0: A => ZeroOffset, 1: B => OneOffset, 2: C => TwoOffset, 3: D => ThreeOffset, 4: E => FourOffset, 5: F => FiveOffset, 6: G => SixOffset);

#[repr(C)]
pub struct ComWrapper<T, U> {
    pointers: U,
    counter: Cell<u32>,
    inner: T,
}

impl<T, U> ComWrapper<T, U>
where
    T: ComInterfaceQuerier,
    U: ComPointersAlloc<T>,
{
    pub fn new(inner: T) -> Self {
        Self {
            pointers: U::alloc(),
            counter: Cell::new(0),
            inner,
        }
    }

    pub unsafe fn into_com_rc<O: ComInterface + ?Sized>(self) -> ComRc<O> {
        self.add_ref();
        let ptr = Box::into_raw(Box::new(self));
        mem::transmute(ptr)
    }
}

impl<T, U> IUnknown for ComWrapper<T, U>
where
    T: ComInterfaceQuerier,
    U: ComPointers,
{
    unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void) -> WinHRESULT {
        let riid = &*riid;
        if let Some(ptr) = self
            .pointers
            .query_interface(riid)
            .filter(|_| self.inner.query_interface(riid))
        {
            *ppv = ptr;
        } else {
            *ppv = ptr::null_mut();
            return E_NOINTERFACE;
        }
        self.add_ref();
        NOERROR
    }

    unsafe fn add_ref(&self) -> u32 {
        let mut value = self.counter.get();
        value += 1;
        self.counter.set(value);
        value
    }

    unsafe fn release(&self) -> u32 {
        let mut value = self.counter.get();
        value -= 1;
        self.counter.set(value);
        // We will see panic because of integer overflow if release() called on *deleted* object
        #[cfg(not(debug_assertions))]
        if value == 0 {
            self.pointers.dealloc();
            Box::from_raw(self as *const Self as *mut Self);
        }
        value
    }
}

impl<T, U: fmt::Debug> fmt::Debug for ComWrapper<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ComWrapper")
            .field("pointers", &self.pointers)
            .field("counter", &self.counter)
            .finish()
    }
}

pub trait ComInterfaceQuerier {
    fn query_interface(&self, _riid: &IID) -> bool {
        true
    }
}

#[macro_export(local_inner_macros)]
macro_rules! com_wrapper {
    ($value:expr => $( $traits:ty ),+) => {{
        type Pointers = ( $( *mut <$traits as $crate::ComInterface>::VTable, )+ );
        let wrapper = $crate::ComWrapper::<_, Pointers>::new($value);
        wrapper
    }};
}

#[macro_export(local_inner_macros)]
macro_rules! com_trait {
    (
        pub trait $trait_name:ident : $base:ident {
            const IID = {
                $data1:literal,
                $data2:literal,
                $data3:literal,
                $data40:literal,
                $data41:literal,
                $data42:literal,
                $data43:literal,
                $data44:literal,
                $data45:literal,
                $data46:literal,
                $data47:literal
            };

            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        com_trait!(
            @trait $base;
            pub trait $trait_name {
                $( unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret; )*
            }
        );

        com_trait!(
            @rest $trait_name: $base;
            impl ComPtr {
                $( unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret; )*
            }
        );

        impl ComInterface for dyn $trait_name {
            const IID: IID = GUID(WinGUID {
                Data1: $data1,
                Data2: $data2,
                Data3: $data3,
                Data4: [
                    $data40,
                    $data41,
                    $data42,
                    $data43,
                    $data44,
                    $data45,
                    $data46,
                    $data47,
                ],
            });
            type Super = dyn $base;

            paste::item! {
                type VTable = [< $trait_name VTable >];
            }
        }

        paste::item! {
            impl ComVTable for [< $trait_name VTable >] {
                type Interface = dyn $trait_name;
            }
        }
    };
    (
        @trait IUnknown;
        pub trait $trait_name:ident {
            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        pub trait $trait_name {
            $( unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret; )*
        }
    };
    (
        @trait $base:ident;
        pub trait $trait_name:ident {
            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        pub trait $trait_name: $base {
            $( unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret; )*
        }
    };
    (
        @rest IUnknown: $base:ident;
        impl ComPtr {
            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        #[repr(C)]
        pub struct IUnknownVTable {
            $( $func: unsafe extern "stdcall" fn(this: *mut *const Self, $( $arg_ty ),*) -> $ret, )*
        }

        impl IUnknownVTable {
            $(
                unsafe extern "stdcall" fn $func<T: ComInterfaceQuerier, U: ComPointers, O: ComOffset>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
                    let this = this.sub(O::VALUE) as *mut ComWrapper<T, U>;
                    (*this).$func($( $arg_name ),*)
                }
            )*
        }

        impl fmt::Debug for IUnknownVTable {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("IUnknownVTable")
                    $(
                        .field(&std::stringify!($func), &(self.$func as *const c_void))
                    )*
                    .finish()
            }
        }

        impl<T: ComInterfaceQuerier, U: ComPointers, O: ComOffset> ComProdInterface<T, U, O> for dyn IUnknown {
            type VTable = IUnknownVTable;

            fn new_vtable() -> Self::VTable {
                Self::VTable {
                    $( $func: Self::VTable::$func::<T, U, O>, )*
                }
            }
        }

        impl<T: ComInterface + ?Sized> IUnknown for ComPtr<T> {
            unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void) -> i32 {
                let vptr = self.inner.as_ptr() as *mut *const IUnknownVTable;
                ((**vptr).query_interface)(vptr, riid, ppv)
            }

            unsafe fn add_ref(&self) -> u32 {
                let vptr = self.inner.as_ptr() as *mut *const IUnknownVTable;
                ((**vptr).add_ref)(vptr)
            }

            unsafe fn release(&self) -> u32 {
                let vptr = self.inner.as_ptr() as *mut *const IUnknownVTable;
                ((**vptr).release)(vptr)
            }
        }

        impl<T: ComInterface + ?Sized> IUnknown for ComRc<T> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    IUnknown::$func(&self.0, $( $arg_name, )*)
                }
            )*
        }
    };
    (
        @rest $trait_name:ident: $base:ident;
        impl ComPtr {
            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        impl<T: $trait_name + ComInterfaceQuerier, U> $trait_name for ComWrapper<T, U> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    $trait_name::$func(&self.inner, $( $arg_name, )*)
                }
            )*
        }

        impl<T: ComInterface + $trait_name + ?Sized> $trait_name for ComPtr<T> {
            $(
                paste::item! {
                    unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                        let vptr = self.inner.as_ptr() as *mut *const [< $trait_name VTable >];
                        ((**vptr).$func)(vptr, $( $arg_name, )*)
                    }
                }
            )*
        }

        impl<T: ComInterface + $trait_name + ?Sized> $trait_name for ComRc<T> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    $trait_name::$func(&self.0, $( $arg_name, )*)
                }
            )*
        }

        paste::item! {
            #[repr(C)]
            pub struct [< $trait_name VTable >] {
                _base: [< $base VTable >],
                $( $func: unsafe extern "stdcall" fn(this: *mut *const Self, $( $arg_ty ),*) -> $ret, )*
            }

            impl [< $trait_name VTable >] {
                $(
                    unsafe extern "stdcall" fn $func<T: $trait_name + ComInterfaceQuerier, U: ComPointers, O: ComOffset>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
                        let this = this.sub(O::VALUE) as *mut ComWrapper<T, U>;
                        $trait_name::$func(&*this, $( $arg_name ),*)
                    }
                )*
            }

            impl fmt::Debug for [< $trait_name VTable >] {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.debug_struct(&std::stringify!([< $trait_name VTable >]))
                        .field("base", &self._base)
                        $(
                            .field(&std::stringify!($func), &(self.$func as *const c_void))
                        )*
                        .finish()
                }
            }

            impl<T: $trait_name + ComInterfaceQuerier, U: ComPointers, O: ComOffset> ComProdInterface<T, U, O> for dyn $trait_name {
                type VTable = [< $trait_name VTable >];

                fn new_vtable() -> Self::VTable {
                    Self::VTable {
                        _base: <dyn $base as ComProdInterface<T, U, O>>::new_vtable(),
                        $( $func: Self::VTable::$func::<T, U, O>, )*
                    }
                }
            }
        }
    };
}

// we define our HRESULT with `must_use` attribute to not forget to handle it
#[repr(transparent)]
#[must_use]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct HRESULT(pub WinHRESULT);

impl PartialEq<WinHRESULT> for HRESULT {
    fn eq(&self, other: &i32) -> bool {
        self.0 == *other
    }
}

impl Deref for HRESULT {
    type Target = WinHRESULT;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// workaround issue #60553
#[repr(C)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct EnumWrapper<T, U> {
    value: U,
    _enum_ty: PhantomData<T>,
}

impl<T, U> EnumWrapper<T, U>
where
    U: Copy,
{
    pub fn get_raw(&self) -> U {
        self.value
    }
}

macro_rules! issue_60553 {
    (
        #[repr($int:ident)]
        #[non_exhaustive]
        #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
        pub enum $name:ident {
            $( $variant:ident = $discriminant:literal, )*
        }
    ) => {
        #[repr($int)]
        #[non_exhaustive]
        #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
        pub enum $name {
            $( $variant, )*
        }

        paste::item! {
            pub type [< $name Wrapper >] = EnumWrapper<$name, $int>;
        }

        impl EnumWrapper<$name, $int> {
            pub fn into_inner(self) -> Option<$name> {
                match self.value {
                    $( $discriminant => Some($name::$variant), )*
                    _ => None,
                }
            }
        }
    };
}

com_trait! {
    pub trait IUnknown: IUnknown {
        const IID = {0x00000000, 0x0000, 0x0000, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46};

        unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void,) -> WinHRESULT;

        unsafe fn add_ref(&self,) -> u32;

        unsafe fn release(&self,) -> u32;
    }
}

// Delphi types

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct TDateTime(pub f64);

impl TDateTime {
    const UNIX_START_DATE: f64 = 25569.0;

    pub fn unix_start() -> Self {
        Self(Self::UNIX_START_DATE)
    }
}

impl Deref for TDateTime {
    type Target = f64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TDateTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// conversions from: https://www.swissdelphicenter.ch/en/showcode.php?id=844
impl From<TDateTime> for SystemTime {
    fn from(date_time: TDateTime) -> Self {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs((date_time.0 - TDateTime::UNIX_START_DATE).round() as u64 / 86400)
    }
}

impl From<SystemTime> for TDateTime {
    fn from(time: SystemTime) -> Self {
        let secs = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self((secs / 86400) as f64 + Self::UNIX_START_DATE)
    }
}

// Base interfaces

com_trait! {
    pub trait IAIMPCore: IUnknown {
        const IID = {0x41494D50, 0x436F, 0x7265, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn create_object(&self, iid: REFIID, obj: *mut *mut c_void,) -> HRESULT;

        unsafe fn get_path(&self, path_id: CorePath, value: *mut ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn register_extension(
            &self,
            service_iid: REFIID,
            extension: ComRc<dyn IUnknown>,
        ) -> HRESULT;

        unsafe fn register_service(&self, service: ComRc<dyn IUnknown>,) -> HRESULT;

        unsafe fn unregister_extension(&self, extension: ComRc<dyn IUnknown>,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum CorePath {
    AudioLibrary = 6,
    Encoders = 8,
    Help = 9,
    Icons = 5,
    Langs = 2,
    Playlists = 1,
    Plugins = 4,
    Profile = 0,
    Skins = 3,
    SkinsCommon = 11,
}

com_trait! {
    pub trait IAIMPPlugin: IUnknown {
        const IID = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};

        unsafe fn info_get(&self, index: PluginInfoWrapper,) -> PWCHAR;

        unsafe fn info_get_categories(&self,) -> PluginCategory;

        unsafe fn initialize(&self, core: ComPtr<dyn IAIMPCore>,) -> WinHRESULT;

        unsafe fn finalize(&self,) -> WinHRESULT;

        unsafe fn system_notification(
            &self,
            notify_id: SystemNotificationWrapper,
            data: Option<ComPtr<dyn IUnknown>>,
        ) -> ();
    }
}

issue_60553! {
    #[repr(i32)]
    #[non_exhaustive]
    #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
    pub enum PluginInfo {
        Name = 0x0,
        Author = 0x1,
        ShortDescription = 0x2,
        FullDescription = 0x3,
    }
}

bitflags! {
    pub struct PluginCategory: DWORD {
        const ADDONS = 0x1;
        const DECODERS = 0x2;
        const VISUALS = 0x4;
        const DSP = 0x8;
    }
}

issue_60553! {
    #[repr(i32)]
    #[non_exhaustive]
    #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
    pub enum SystemNotification {
        ServiceAdded = 0x1,
        ServiceRemoved = 0x2,
        ExtensionRemoved = 0x3,
    }
}

com_trait! {
    pub trait IAIMPExternalSettingsDialog: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7472, 0x6E, 0x4F, 0x70, 0x74, 0x44, 0x6C, 0x67, 0x00};

        unsafe fn show(&self, parent_window: HWND,) -> ();
    }
}

#[macro_export]
macro_rules! plugin_get_header {
    ($func:expr) => {
        #[no_mangle]
        pub extern "stdcall" fn AIMPPluginGetHeader(header: *mut *mut c_void) -> HRESULT {
            $func(header)
        }
    };
}

// Primitives

com_trait! {
    pub trait IAIMPConfig: IUnknown {
        const IID = {0x41494D50, 0x436F, 0x6E66, 0x69, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn delete(&self, key_path: ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn get_value_as_float(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: *mut c_double,
        ) -> HRESULT;

        unsafe fn get_value_as_int32(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: *mut i32,
        ) -> HRESULT;

        unsafe fn get_value_as_int64(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: *mut i64,
        ) -> HRESULT;

        unsafe fn get_value_as_stream(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: *mut ComRc<dyn IAIMPStream>,
        ) -> HRESULT;

        unsafe fn get_value_as_string(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn set_value_as_float(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: c_double,
        ) -> HRESULT;

        unsafe fn set_value_as_int32(&self, key_path: ComRc<dyn IAIMPString>, value: i32,) -> HRESULT;

        unsafe fn set_value_as_int64(&self, key_path: ComRc<dyn IAIMPString>, value: i64,) -> HRESULT;

        unsafe fn set_value_as_stream(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: ComRc<dyn IAIMPStream>,
        ) -> HRESULT;

        unsafe fn set_value_as_string(
            &self,
            key_path: ComRc<dyn IAIMPString>,
            value: ComRc<dyn IAIMPString>,
        ) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPDPIAware: IUnknown {
        const IID = {0x41494D50, 0x4450, 0x4941, 0x77, 0x61, 0x72, 0x65, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_dpi(&self,) -> DPI;

        unsafe fn set_dpi(&self, value: DPI,) -> HRESULT;
    }
}

#[repr(transparent)]
pub struct DPI(c_int);

impl DPI {
    pub fn new(value: c_int) -> Option<Self> {
        if value >= 40 && value <= 250 {
            Some(Self(value))
        } else {
            None
        }
    }
}

com_trait! {
    pub trait IAIMPErrorInfo: IUnknown {
        const IID = {0x41494D50, 0x4572, 0x7249, 0x6E, 0x66, 0x6F, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_info(
            &self,
            error_code: *mut c_int,
            message: *mut ComRc<dyn IAIMPString>,
            details: *mut Option<ComRc<dyn IAIMPString>>,
        ) -> HRESULT;

        unsafe fn get_info_formatted(&self, s: *mut ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn set_info(
            &self,
            error_code: c_int,
            message: ComRc<dyn IAIMPString>,
            details: Option<ComRc<dyn IAIMPString>>,
        ) -> ();
    }
}

com_trait! {
    pub trait IAIMPFileStream: IAIMPStream {
        const IID = {0x41494D50, 0x4669, 0x6C65, 0x53, 0x74, 0x72, 0x65, 0x61, 0x6D, 0x00, 0x00};

        unsafe fn get_clipping(&self, offset: *mut i64, size: *mut i64,) -> HRESULT;

        unsafe fn get_file_name(&self, s: *mut ComRc<dyn IAIMPString>,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPHashCode: IUnknown {
        const IID = {0x41494D50, 0x4861, 0x7368, 0x43, 0x6F, 0x64, 0x65, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_hash_code(&self,) -> c_int;

        unsafe fn recalculate(&self,) -> ();
    }
}

com_trait! {
    pub trait IAIMPImage: IUnknown {
        const IID = {0x41494D50, 0x496D, 0x6167, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn load_from_file(&self, file_name: ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn load_from_stream(&self, stream: ComRc<dyn IAIMPStream>,) -> HRESULT;

        unsafe fn save_to_file(
            &self,
            file_name: ComRc<dyn IAIMPString>,
            format_id: ImageFormat,
        ) -> HRESULT;

        unsafe fn save_to_stream(
            &self,
            stream: ComRc<dyn IAIMPStream>,
            format_id: ImageFormat,
        ) -> HRESULT;

        unsafe fn get_format_id(&self,) -> ImageFormat;

        unsafe fn get_size(&self, size: *mut SIZE,) -> HRESULT;

        unsafe fn clone(&self, image: *mut ComRc<dyn IAIMPImage>,) -> HRESULT;

        unsafe fn draw(&self, dc: HDC, r: RECT, flags: ImageDraw, attrs: *mut c_void,) -> HRESULT;

        unsafe fn resize(&self, width: c_int, height: c_int,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ImageFormat {
    Unknown = 0,
    Bmp = 1,
    Gif = 2,
    Jpg = 3,
    Png = 4,
}

#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ImageDrawStretchMode {
    Stretch = 0,
    Fill = 1,
    Fit = 2,
    Tile = 4,
}

#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ImageDrawQuality {
    Default = 0,
    Low = 8,
    High = 16,
}

impl Default for ImageDrawQuality {
    fn default() -> Self {
        Self::Default
    }
}

#[repr(transparent)]
pub struct ImageDraw(DWORD);

impl ImageDraw {
    pub fn new(stretch_mode: ImageDrawStretchMode, quality: ImageDrawQuality) -> Self {
        Self(stretch_mode as DWORD | quality as DWORD)
    }
}

com_trait! {
    pub trait IAIMPImage2: IAIMPImage {
        const IID = {0x41494D50, 0x496D, 0x6167, 0x65, 0x32, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn load_from_resource(
            &self,
            res_instance: HMODULE,
            res_name: *mut WCHAR,
            res_type: *mut WCHAR,
        ) -> HRESULT;

        unsafe fn load_from_bitmap(&self, bitmap: HBITMAP,) -> HRESULT;

        unsafe fn load_from_bits(&self, bits: *mut RGBQUAD, width: c_int, height: c_int,) -> HRESULT;

        unsafe fn copy_to_clipboard(&self,) -> HRESULT;

        unsafe fn can_paste_from_clipboard(&self,) -> HRESULT;

        unsafe fn paste_from_clipboard(&self,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPImageContainer: IUnknown {
        const IID = {0x41494D50, 0x496D, 0x6167, 0x65, 0x43, 0x6F, 0x6E, 0x74, 0x6E, 0x72, 0x00};

        unsafe fn create_image(&self, image: *mut ComRc<dyn IAIMPImage>,) -> HRESULT;

        unsafe fn get_info(&self, size: *mut SIZE, format_id: ImageFormat,) -> HRESULT;

        unsafe fn get_data(&self,) -> *mut u8;

        unsafe fn get_data_size(&self,) -> DWORD;

        unsafe fn set_data_size(&self, value: DWORD,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPMemoryStream: IAIMPStream {
        const IID = {0x41494D50, 0x4D65, 0x6D53, 0x74, 0x72, 0x65, 0x61, 0x6D, 0x00, 0x00, 0x00};

        unsafe fn get_data(&self,) -> *mut u8;
    }
}

com_trait! {
    pub trait IAIMPObjectList: IUnknown {
        const IID = {0x41494D50, 0x4F62, 0x6A4C, 0x69, 0x73, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn add(&self, obj: ComRc<dyn IUnknown>,) -> HRESULT;

        unsafe fn clear(&self,) -> HRESULT;

        unsafe fn delete(&self, index: c_int,) -> HRESULT;

        unsafe fn insert(&self, index: c_int, obj: ComRc<dyn IUnknown>,) -> HRESULT;

        unsafe fn get_count(&self,) -> c_int;

        unsafe fn get_object(&self, index: c_int, iid: REFIID, obj: *mut ComRc<dyn IUnknown>,) -> HRESULT;

        unsafe fn set_object(&self, index: c_int, obj: ComRc<dyn IUnknown>,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPProgressCallback: IUnknown {
        const IID = {0x41494D50, 0x5072, 0x6F67, 0x72, 0x65, 0x73, 0x73, 0x43, 0x42, 0x00, 0x00};

        unsafe fn process(&self, progress: c_float, canceled: *mut bool,) -> ();
    }
}

com_trait! {
    pub trait IAIMPPropertyList: IUnknown {
        const IID = {0x41494D50, 0x5072, 0x6F70, 0x4C, 0x69, 0x73, 0x74, 0x00, 0x00, 0x00, 0x00};

        unsafe fn begin_update(&self,) -> ();

        unsafe fn end_update(&self,) -> ();

        unsafe fn reset(&self,) -> HRESULT;

        unsafe fn get_value_as_float(&self, property_id: c_int, value: *mut c_double,) -> HRESULT;

        unsafe fn get_value_as_int32(&self, property_id: c_int, value: *mut i32,) -> HRESULT;

        unsafe fn get_value_as_int64(&self, property_id: c_int, value: *mut i64,) -> HRESULT;

        unsafe fn get_value_as_object(
            &self,
            property_id: c_int,
            iid: REFIID,
            value: *mut ComRc<dyn IUnknown>,
        ) -> HRESULT;

        unsafe fn set_value_as_float(&self, property_id: c_int, value: c_double,) -> HRESULT;

        unsafe fn set_value_as_int32(&self, property_id: c_int, value: i32,) -> HRESULT;

        unsafe fn set_value_as_int64(&self, property_id: c_int, value: i64,) -> HRESULT;

        unsafe fn set_value_as_object(&self, property_id: c_int, value: ComRc<dyn IUnknown>,)
            -> HRESULT;
    }
}

impl dyn IAIMPPropertyList {
    pub const CUSTOM_PROPID_BASE: c_int = 1000;
}

com_trait! {
    pub trait IAIMPPropertyList2: IUnknown {
        const IID = {0x41494D50, 0x5072, 0x6F70, 0x4C, 0x69, 0x73, 0x74, 0x32, 0x00, 0x00, 0x00};

        unsafe fn get_value_as_variant(&self, property_id: c_int, value: *mut *mut VARIANT,) -> HRESULT;

        unsafe fn set_value_as_variant(&self, property_id: c_int, value: *mut VARIANT,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPString: IUnknown {
        const IID = {0x41494D50, 0x5374, 0x7269, 0x6E, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_char(&self, index: c_int, char: *mut WCHAR,) -> HRESULT;

        unsafe fn get_data(&self,) -> *mut WCHAR;

        unsafe fn get_length(&self,) -> c_int;

        unsafe fn get_hash_code(&self,) -> c_int;

        unsafe fn set_char(&self, index: c_int, char: WCHAR,) -> HRESULT;

        unsafe fn set_data(&self, chars: *const WCHAR, chars_count: c_int,) -> HRESULT;

        unsafe fn add(&self, s: ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn add2(&self, chars: *const WCHAR, chars_count: c_int,) -> HRESULT;

        unsafe fn change_case(&self, mode: StringCase,) -> HRESULT;

        unsafe fn clone(&self, s: *mut ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn compare(
            &self,
            s: ComRc<dyn IAIMPString>,
            compare_result: *mut c_int,
            ignore_case: bool,
        ) -> HRESULT;

        unsafe fn compare2(
            &self,
            chars: *const WCHAR,
            chars_count: c_int,
            compare_result: *mut c_int,
            ignore_case: bool,
        ) -> HRESULT;

        unsafe fn delete(&self, index: c_int, count: c_int,) -> HRESULT;

        unsafe fn find(
            &self,
            s: ComRc<dyn IAIMPString>,
            index: *mut c_int,
            flags: StringFind,
            start_from_index: c_int,
        ) -> HRESULT;

        unsafe fn find2(
            &self,
            chars: *mut WCHAR,
            chars_count: c_int,
            index: *mut c_int,
            flags: StringFind,
            start_from_index: c_int,
        ) -> HRESULT;

        unsafe fn insert(&self, index: c_int, s: ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn insert2(&self, index: c_int, chars: *const WCHAR, chars_count: c_int,) -> HRESULT;

        unsafe fn replace(
            &self,
            old_pattern: ComRc<dyn IAIMPString>,
            new_pattern: ComRc<dyn IAIMPString>,
            flags: StringFind,
        ) -> HRESULT;

        unsafe fn replace2(
            &self,
            old_pattern_chars: ComRc<dyn IAIMPString>,
            old_pattern_chars_count: c_int,
            new_pattern_chars: *mut WCHAR,
            new_pattern_chars_count: c_int,
            flags: StringFind,
        ) -> HRESULT;

        unsafe fn sub_string(
            &self,
            index: c_int,
            count: c_int,
            s: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum StringCase {
    Lower = 1,
    Upper = 2,
    AllWordsWithCapitalLetter = 3,
    FirstWordWithCapitalLetter = 4,
}

bitflags! {
    pub struct StringFind: c_int {
        const NONE = 0;
        const IGNORE_CASE = 1;
        const WHOLE_WORD = 2;
    }
}

com_trait! {
    pub trait IAIMPStream: IUnknown {
        const IID = {0x41494D50, 0x5374, 0x7265, 0x61, 0x6D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_size(&self,) -> i64;

        unsafe fn set_size(&self, value: i64,) -> HRESULT;

        unsafe fn get_position(&self,) -> i64;

        unsafe fn seek(&self, offset: i64, mode: StreamSeekFrom,) -> HRESULT;

        unsafe fn read(&self, buffer: *mut c_uchar, count: DWORD,) -> c_int;

        unsafe fn write(&self, buffer: *const c_uchar, count: DWORD, written: *mut DWORD,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum StreamSeekFrom {
    Beginning = 0,
    Current = 1,
    End = 2,
}

// Threading

com_trait! {
    pub trait IAIMPServiceThreads: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7654, 0x68, 0x72, 0x65, 0x61, 0x64, 0x73, 0x00, 0x00};

        unsafe fn execute_in_main_thread(
            &self,
            task: ComRc<dyn IAIMPTask>,
            flags: ServiceThreadsFlags,
        ) -> HRESULT;

        unsafe fn execute_in_thread(
            &self,
            task: ComRc<dyn IAIMPTask>,
            task_handle: *mut DWORD_PTR,
        ) -> HRESULT;

        unsafe fn cancel(&self, task_handle: DWORD_PTR, flags: ServiceThreadsFlags,) -> HRESULT;

        unsafe fn wait_for(&self, task_handle: DWORD_PTR,) -> HRESULT;
    }
}

bitflags! {
    pub struct ServiceThreadsFlags: DWORD {
        const NONE = 0;
        const WAIT_FOR = 0x1;
    }
}

com_trait! {
    pub trait IAIMPTask: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7654, 0x68, 0x72, 0x65, 0x61, 0x64, 0x73, 0x00, 0x00};

        unsafe fn execute(&self, owner: ComPtr<dyn IAIMPTaskOwner>,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPTaskOwner: IUnknown {
        const IID = {0x41494D50, 0x5461, 0x736B, 0x4F, 0x77, 0x6E, 0x65, 0x72, 0x32, 0x00, 0x00};

        unsafe fn is_canceled(&self,) -> BOOL;
    }
}

com_trait! {
    pub trait IAIMPTaskPriority: IUnknown {
        const IID = {0x41494D50, 0x5461, 0x736B, 0x50, 0x72, 0x69, 0x6F, 0x72, 0x69, 0x74, 0x79};

        unsafe fn get_priority(&self,) -> TaskPriority;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum TaskPriority {
    Normal,
    Low,
    High,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

// Internet

com_trait! {
    pub trait IAIMPServiceConnectionSettings: IAIMPPropertyList {
        const IID = {0x4941494D, 0x5053, 0x7276, 0x43, 0x6F, 0x6E, 0x6E, 0x43, 0x66, 0x67, 0x00};
    }
}

issue_60553! {
    #[repr(i32)]
    #[non_exhaustive]
    #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
    pub enum ConnectionType {
        Direct = 0,
        Proxy = 1,
        SystemDefaults = 2,
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ConnectionSettingsProp {
    ConnectionType = 1,
    ProxyServer = 2,
    ProxyPort = 3,
    ProxyUsername = 4,
    ProxyUserPass = 5,
    Timeout = 6,
    UserAgent = 7,
}

com_trait! {
    pub trait IAIMPServiceHTTPClient: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7648, 0x74, 0x74, 0x70, 0x43, 0x6C, 0x74, 0x00, 0x00};

        unsafe fn get(
            &self,
            url: ComRc<dyn IAIMPString>,
            flags: HttpClientFlags,
            answer_data: ComPtr<dyn IAIMPStream>,
            event_handler: ComRc<dyn IAIMPHTTPClientEvents>,
            params: Option<ComRc<dyn IAIMPConfig>>,
            task_id: *mut *const c_void,
        ) -> HRESULT;

        unsafe fn post(
            &self,
            url: ComRc<dyn IAIMPString>,
            flags: HttpClientFlags,
            answer_data: ComPtr<dyn IAIMPString>,
            post_data: Option<ComRc<dyn IAIMPStream>>,
            event_handler: ComRc<dyn IAIMPHTTPClientEvents>,
            params: Option<ComRc<dyn IAIMPConfig>>,
            task_id: *mut *const c_void,
        ) -> HRESULT;

        unsafe fn cancel(&self, task_id: *const c_void, flags: HttpClientFlags,) -> HRESULT;
    }
}

bitflags! {
    pub struct HttpClientRestFlags: DWORD {
        const NONE = 0;
        const WAIT_FOR = 1;
        const UTF8 = 2;
    }
}

#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum HttpClientPriorityFlags {
    Normal = 0,
    Low = 4,
    High = 8,
}

impl Default for HttpClientPriorityFlags {
    fn default() -> Self {
        Self::Normal
    }
}

pub struct HttpClientFlags(DWORD);

impl HttpClientFlags {
    pub fn new(rest: HttpClientRestFlags, priority: HttpClientPriorityFlags) -> Self {
        Self(rest.bits as DWORD | priority as DWORD)
    }
}

com_trait! {
    pub trait IAIMPHTTPClientEvents: IUnknown {
        const IID = {0x41494D50, 0x4874, 0x7470, 0x43, 0x6C, 0x74, 0x45, 0x76, 0x74, 0x73, 0x00};

        unsafe fn on_accept(&self, content_type: ComRc<dyn IAIMPString>, content_size: i64, allow: *mut BOOL,) -> ();

        unsafe fn on_complete(&self, error_info: Option<ComRc<dyn IAIMPErrorInfo>>, canceled: BOOL,) -> ();

        unsafe fn on_progress(&self, downloaded: i64, total: i64,) -> ();
    }
}

com_trait! {
    pub trait IAIMPServiceHTTPClient2: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7648, 0x74, 0x74, 0x70, 0x43, 0x6C, 0x74, 0x32, 0x00};

        unsafe fn request(
            &self,
            url: ComRc<dyn IAIMPString>,
            method: HttpMethod,
            flags: HttpClientFlags,
            answer_data: ComPtr<dyn IAIMPStream>,
            post_data: Option<ComRc<dyn IAIMPStream>>,
            event_handler: ComRc<dyn IAIMPHTTPClientEvents>,
            params: Option<ComRc<dyn IAIMPConfig>>,
            task_id: *mut *const c_void,
        ) -> HRESULT;

        unsafe fn cancel(&self, task_id: *const c_void, flags: HttpClientFlags,) -> HRESULT;
    }
}

#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Head,
}

com_trait! {
    pub trait IAIMPHTTPClientEvents2: IUnknown {
        const IID = {0x41494D50, 0x4874, 0x7470, 0x43, 0x6C, 0x74, 0x45, 0x76, 0x74, 0x73, 0x32};

        unsafe fn on_accept_headers(&self, header: ComRc<dyn IAIMPString>, allow: *mut BOOL,) -> ();
    }
}

// File Manager

com_trait! {
    pub trait IAIMPFileInfo: IAIMPPropertyList {
        const IID = {0x41494D50, 0x4669, 0x6C65, 0x49, 0x6E, 0x66, 0x6F, 0x00, 0x00, 0x00, 0x00};

        unsafe fn assign(&self, source: ComPtr<dyn IAIMPFileInfo>,) -> HRESULT;

        unsafe fn clone(&self, info: *mut ComRc<dyn IAIMPFileInfo>,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum FileInfoProp {
    Custom,
    Album,
    AlbumArt,
    AlbumArtist,
    AlbumGain,
    AlbumPeak,
    Artist,
    BitRate,
    Bpm,
    Channels,
    Comment,
    Composer,
    Copyright,
    CueSheet,
    Date,
    DiskNumber,
    DiskTotal,
    Duration,
    Filename,
    FileSize,
    Genre,
    Lyrics,
    Publisher = 23,
    SampleRate,
    Title,
    TrackGain,
    TrackNumber,
    TrackPeak,
    TrackTotal,
    Url,
    BitDepth,
    Codec,
    Conductor,
    Mood,
    Catalog,
    Isrc,
    Lyricist,
    EncodeBy,
    Rating,
    StatAddingDate,
    StatLastPlayDate,
    StatMark,
    StatPlayCount,
    StatRating,
    StatDisplayingMark = 22,
}

com_trait! {
    pub trait IAIMPVirtualFile: IAIMPPropertyList {
        const IID = {0x41494D50, 0x5669, 0x7274, 0x75, 0x61, 0x6C, 0x46, 0x69, 0x6C, 0x65, 0x00};

        unsafe fn create_stream(&self, stream: *mut ComRc<dyn IAIMPStream>,) -> HRESULT;

        unsafe fn get_file_info(&self, info: ComPtr<dyn IAIMPFileInfo>,) -> HRESULT;

        unsafe fn is_exists(&self,) -> BOOL;

        unsafe fn is_in_same_stream(&self, virtual_file: ComPtr<dyn IAIMPVirtualFile>,) -> HRESULT;

        unsafe fn synchronize(&self,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum VirtualFileProp {
    FileUri,
    AudioSourceFile,
    ClipStart,
    ClipFinish,
    IndexInSet,
    FileFormat,
}

com_trait! {
    pub trait IAIMPServiceFileFormats: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x46, 0x6D, 0x74, 0x73, 0x00};

        unsafe fn get_formats(&self, flags: FileFormatsCategory, s: *mut ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn is_supported(&self, file_name: ComRc<dyn IAIMPString>, flags: FileFormatsCategory,) -> HRESULT;
    }
}

bitflags! {
    pub struct FileFormatsCategory: DWORD {
        const AUDIO = 1;
        const PLAYLISTS = 2;
    }
}

com_trait! {
    pub trait IAIMPServiceFileInfo: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x49, 0x6E, 0x66, 0x6F, 0x00};

        unsafe fn get_file_info_from_file_uri(
            &self,
            file_uri: ComRc<dyn IAIMPString>,
            flags: FileInfoFlags,
            info: ComPtr<dyn IAIMPFileInfo>,
        ) -> HRESULT;

        unsafe fn get_file_info_from_stream(
            &self,
            stream: ComRc<dyn IAIMPStream>,
            flags: FileInfoFlags,
            info: ComPtr<dyn IAIMPFileInfo>,
        ) -> HRESULT;

        unsafe fn get_virtual_file(&self, file_uri: ComRc<dyn IAIMPString>, flags: DWORD, info: *mut ComRc<dyn IAIMPVirtualFile>,) -> HRESULT;
    }
}

bitflags! {
    pub struct FileInfoFlags: DWORD {
        const NONE = 0;
        const DONT_USE_AUDIO_DECODERS = 1;
    }
}

com_trait! {
    pub trait IAIMPServiceFileInfoFormatter: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x6C, 0x49, 0x6E, 0x66, 0x46, 0x6D, 0x74, 0x00};

        unsafe fn format(
            &self,
            template: ComRc<dyn IAIMPString>,
            file_info: Option<ComRc<dyn IAIMPFileInfo>>,
            reserved: c_int,
            additional_info: Option<ComRc<dyn IUnknown>>,
            formatted_result: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPServiceFileInfoFormatterUtils: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x6C, 0x49, 0x6E, 0x66, 0x46, 0x6D, 0x74, 0x55};

        unsafe fn show_macros_legend(&self, screen_target: RECT, reserved: c_int, events_handler: ComRc<dyn IUnknown>,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPServiceFileManager: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x4D, 0x61, 0x6E, 0x00, 0x00};
    }
}

com_trait! {
    pub trait IAIMPServiceFileStreaming: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x53, 0x74, 0x72, 0x6D, 0x00};

        unsafe fn create_stream_for_file(
            &self,
            file_name: ComRc<dyn IAIMPString>,
            flags: FileStreamingFlags,
            offset: i64,
            size: i64,
            stream: *mut ComRc<dyn IAIMPFileStream>,
        ) -> HRESULT;

        unsafe fn create_stream_for_file_uri(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            virtual_file: *mut Option<ComRc<dyn IAIMPVirtualFile>>,
            stream: *mut ComRc<dyn IAIMPFileStream>,
        ) -> HRESULT;
    }
}

bitflags! {
    pub struct FileStreamingFlags: DWORD {
        const READ = 0;
        const CREATE_NEW = 1;
        const READ_WRITE = 2;
        const MAP_TO_MEMORY = 4;
    }
}

com_trait! {
    pub trait IAIMPServiceFileSystems: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x53, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get(&self, file_uri: ComPtr<dyn IAIMPString>, iid: REFIID, obj: *mut *mut c_void,) -> HRESULT;

        unsafe fn get_default(&self, iid: REFIID, obj: *mut *mut c_void,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPFileSystemCustomFileCommand: IUnknown {
        const IID = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};

        unsafe fn can_process(&self, file_name: ComRc<dyn IAIMPString>,) -> WinHRESULT;

        unsafe fn process(&self, file_name: ComRc<dyn IAIMPString>,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPFileSystemCommandCopyToClipboard: IUnknown {
        const IID = {0x41465343, 0x6D64, 0x436F, 0x70, 0x79, 0x32, 0x43, 0x6C, 0x70, 0x62, 0x64};

        unsafe fn copy_to_clipboard(&self, files: ComRc<dyn IAIMPObjectList>,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPFileSystemCommandDelete: IAIMPFileSystemCustomFileCommand {
        const IID = {0x41465343, 0x6D64, 0x4465, 0x6C, 0x65, 0x74, 0x65, 0x00, 0x00, 0x00, 0x00};
    }
}

com_trait! {
    pub trait IAIMPFileSystemCommandDropSource: IUnknown {
        const IID = {0x41465343, 0x6D64, 0x4472, 0x6F, 0x70, 0x53, 0x72, 0x63, 0x00, 0x00, 0x00};

        unsafe fn create_stream(
            &self,
            file_name: ComRc<dyn IAIMPString>,
            stream: *mut ComRc<dyn IAIMPStream>,
        ) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPFileSystemCommandFileInfo: IUnknown {
        const IID = { 0x41494D50, 0x4578, 0x7446, 0x69, 0x6C, 0x65, 0x49, 0x6E, 0x66, 0x6F, 0x00};

        unsafe fn get_file_attrs(&self, file_name: ComRc<dyn IAIMPString>, attrs: *mut TAIMPFileAttributes,) -> WinHRESULT;

        unsafe fn get_file_size(&self, file_name: ComRc<dyn IAIMPString>, size: *mut i64,) -> WinHRESULT;

        unsafe fn is_file_exists(&self, file_name: ComRc<dyn IAIMPString>,) -> WinHRESULT;
    }
}

#[repr(packed(1))]
pub struct TAIMPFileAttributes {
    pub attributes: DWORD,
    pub time_creation: TDateTime,
    pub time_last_access: TDateTime,
    pub time_last_write: TDateTime,
    pub reserved0: i64,
    pub reserved1: i64,
    pub reserved2: i64,
}

com_trait! {
    pub trait IAIMPFileSystemCommandOpenFileFolder: IAIMPFileSystemCustomFileCommand {
        const IID = {0x41465343, 0x6D64, 0x4669, 0x6C, 0x65, 0x46, 0x6C, 0x64, 0x72, 0x00, 0x00};
    }
}

com_trait! {
    pub trait IAIMPFileSystemCommandStreaming: IUnknown {
        const IID = {0x41465343, 0x6D64, 0x5374, 0x72, 0x65, 0x61, 0x6D, 0x69, 0x6E, 0x67, 0x00};

        unsafe fn create_stream(
            &self,
            file_name: ComRc<dyn IAIMPString>,
            flags: FileStreamingFlags,
            offset: i64,
            size: i64,
            stream: *mut ComRc<dyn IAIMPStream>,
        ) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPServiceFileURI: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x55, 0x52, 0x49, 0x00, 0x00};

        unsafe fn build(
            &self,
            container_file_name: ComRc<dyn IAIMPString>,
            part_name: ComRc<dyn IAIMPString>,
            file_name: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn parse(
            &self,
            file_uri: ComRc<dyn IAIMPString>,
            container_file_name: *mut ComRc<dyn IAIMPString>,
            part_name: *mut Option<ComRc<dyn IAIMPString>>,
        ) -> HRESULT;

        unsafe fn change_file_ext(
            &self,
            file_uri: *mut ComPtr<dyn IAIMPString>,
            new_ext: ComRc<dyn IAIMPString>,
            flags: FileUriFlags,
        ) -> HRESULT;

        unsafe fn extract_file_ext(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            s: *mut ComRc<dyn IAIMPString>,
            flags: FileUriFlags,
        ) -> HRESULT;

        unsafe fn extract_file_name(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            s: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn extract_file_parent_dir_name(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            s: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn extract_file_parent_name(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            s: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn extract_file_path(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            s: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn is_url(&self, file_uri: ComPtr<dyn IAIMPString>,) -> HRESULT;
    }
}

bitflags! {
    pub struct FileUriFlags: DWORD {
        const NONE = 0;
        const DOUBLE_EXTS = 1;
        const PART_EXT = 2;
    }
}

com_trait! {
    pub trait IAIMPServiceFileURI2: IAIMPServiceFileURI {
        const IID = {0x41494D50, 0x5372, 0x7646, 0x69, 0x6C, 0x65, 0x55, 0x52, 0x49, 0x32, 0x00};

        unsafe fn get_scheme(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            scheme: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionFileExpander: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7446, 0x69, 0x6C, 0x65, 0x45, 0x78, 0x70, 0x64, 0x72};

        unsafe fn expand(
            &self,
            file_name: ComRc<dyn IAIMPString>,
            list: *mut ComRc<dyn IAIMPObjectList>,
            progress_callback: Option<ComPtr<dyn IAIMPProgressCallback>>,
        ) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionFileFormat: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7446, 0x69, 0x6C, 0x65, 0x46, 0x6D, 0x74, 0x00, 0x00};

        unsafe fn get_description(&self, s: *mut ComRc<dyn IAIMPString>,) -> WinHRESULT;

        unsafe fn get_ext_list(&self, s: *mut ComRc<dyn IAIMPString>,) -> WinHRESULT;

        unsafe fn get_flags(&self, s: *mut FileFormatsCategory,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionFileInfoProvider: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7446, 0x69, 0x6C, 0x65, 0x49, 0x6E, 0x66, 0x6F, 0x00};

        unsafe fn get_file_info(&self, file_uri: ComRc<dyn IAIMPString>, info: ComRc<dyn IAIMPFileInfo>,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionFileInfoProviderEx: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7446, 0x69, 0x6C, 0x65, 0x49, 0x6E, 0x66, 0x6F, 0x45};

        unsafe fn get_file_info(&self, stream: ComRc<dyn IAIMPStream>, info: ComRc<dyn IAIMPFileInfo>,) -> WinHRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionFileSystem: IAIMPPropertyList {
        const IID = {0x41494D50, 0x4578, 0x7446, 0x53, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum FileSystemProp {
    Scheme = 1,
    ReadOnly = 2,
}

// Actions

com_trait! {
    pub trait IAIMPServiceActionManager: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7641, 0x63, 0x74, 0x69, 0x6F, 0x6E, 0x4D, 0x61, 0x6E};

        unsafe fn get_by_id(&self, id: ComRc<dyn IAIMPString>, action: *mut ComRc<dyn IAIMPAction>,) -> HRESULT;

        unsafe fn make_hotkey(&self, modifiers: HotkeyModifier, key: Key,) -> c_int;
    }
}

bitflags! {
    pub struct HotkeyModifier: WORD {
        const CTRL = 1;
        const ALT = 2;
        const SHIFT = 4;
        const WIN = 8;
    }
}

// from https://godoc.org/github.com/lxn/walk#Key
#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Key {
    LButton = VK_LBUTTON as u16,
    RButton = VK_RBUTTON as u16,
    Cancel = VK_CANCEL as u16,
    MButton = VK_MBUTTON as u16,
    XButton1 = VK_XBUTTON1 as u16,
    XButton2 = VK_XBUTTON2 as u16,
    Back = VK_BACK as u16,
    Tab = VK_TAB as u16,
    Clear = VK_CLEAR as u16,
    Return = VK_RETURN as u16,
    Shift = VK_SHIFT as u16,
    Control = VK_CONTROL as u16,
    Menu = VK_MENU as u16,
    Pause = VK_PAUSE as u16,
    Capital = VK_CAPITAL as u16,
    KanaOrHangul = VK_KANA as u16,
    Junja = VK_JUNJA as u16,
    Final = VK_FINAL as u16,
    HanjaOrKanji = VK_HANJA as u16,
    Escape = VK_ESCAPE as u16,
    Convert = VK_CONVERT as u16,
    NonConvert = VK_NONCONVERT as u16,
    Accept = VK_ACCEPT as u16,
    ModeChange = VK_MODECHANGE as u16,
    Space = VK_SPACE as u16,
    Prior = VK_PRIOR as u16,
    Next = VK_NEXT as u16,
    End = VK_END as u16,
    Home = VK_HOME as u16,
    Left = VK_LEFT as u16,
    Up = VK_UP as u16,
    Right = VK_RIGHT as u16,
    Down = VK_DOWN as u16,
    Select = VK_SELECT as u16,
    Print = VK_PRINT as u16,
    Execute = VK_EXECUTE as u16,
    Snapshot = VK_SNAPSHOT as u16,
    Insert = VK_INSERT as u16,
    Delete = VK_DELETE as u16,
    Help = VK_HELP as u16,
    Zero = 0x30,
    One = 0x31,
    Two = 0x32,
    Three = 0x33,
    Four = 0x34,
    Five = 0x35,
    Six = 0x36,
    Seven = 0x37,
    Eight = 0x38,
    Nine = 0x39,
    A = 0x41,
    B = 0x42,
    C = 0x43,
    D = 0x44,
    E = 0x45,
    F = 0x46,
    G = 0x47,
    H = 0x48,
    I = 0x49,
    J = 0x4A,
    K = 0x4B,
    L = 0x4C,
    M = 0x4D,
    N = 0x4E,
    O = 0x4F,
    P = 0x50,
    Q = 0x51,
    R = 0x52,
    S = 0x53,
    T = 0x54,
    U = 0x55,
    V = 0x56,
    W = 0x57,
    X = 0x58,
    Y = 0x59,
    Z = 0x5A,
    LWin = VK_LWIN as u16,
    RWin = VK_RWIN as u16,
    Apps = VK_APPS as u16,
    Sleep = VK_SLEEP as u16,
    Numpad0 = VK_NUMPAD0 as u16,
    Numpad1 = VK_NUMPAD1 as u16,
    Numpad2 = VK_NUMPAD2 as u16,
    Numpad3 = VK_NUMPAD3 as u16,
    Numpad4 = VK_NUMPAD4 as u16,
    Numpad5 = VK_NUMPAD5 as u16,
    Numpad6 = VK_NUMPAD6 as u16,
    Numpad7 = VK_NUMPAD7 as u16,
    Numpad8 = VK_NUMPAD8 as u16,
    Numpad9 = VK_NUMPAD9 as u16,
    Multiply = VK_MULTIPLY as u16,
    Add = VK_ADD as u16,
    Separator = VK_SEPARATOR as u16,
    Subtract = VK_SUBTRACT as u16,
    Decimal = VK_DECIMAL as u16,
    Divide = VK_DIVIDE as u16,
    F1 = VK_F1 as u16,
    F2 = VK_F2 as u16,
    F3 = VK_F3 as u16,
    F4 = VK_F4 as u16,
    F5 = VK_F5 as u16,
    F6 = VK_F6 as u16,
    F7 = VK_F7 as u16,
    F8 = VK_F8 as u16,
    F9 = VK_F9 as u16,
    F10 = VK_F10 as u16,
    F11 = VK_F11 as u16,
    F12 = VK_F12 as u16,
    F13 = VK_F13 as u16,
    F14 = VK_F14 as u16,
    F15 = VK_F15 as u16,
    F16 = VK_F16 as u16,
    F17 = VK_F17 as u16,
    F18 = VK_F18 as u16,
    F19 = VK_F19 as u16,
    F20 = VK_F20 as u16,
    F21 = VK_F21 as u16,
    F22 = VK_F22 as u16,
    F23 = VK_F23 as u16,
    F24 = VK_F24 as u16,
    Numlock = VK_NUMLOCK as u16,
    Scroll = VK_SCROLL as u16,
    LShift = VK_LSHIFT as u16,
    RShift = VK_RSHIFT as u16,
    LControl = VK_LCONTROL as u16,
    RControl = VK_RCONTROL as u16,
    LMenu = VK_LMENU as u16,
    RMenu = VK_RMENU as u16,
    BrowserBack = VK_BROWSER_BACK as u16,
    BrowserForward = VK_BROWSER_FORWARD as u16,
    BrowserRefresh = VK_BROWSER_REFRESH as u16,
    BrowserStop = VK_BROWSER_STOP as u16,
    BrowserSearch = VK_BROWSER_SEARCH as u16,
    BrowserFavorites = VK_BROWSER_FAVORITES as u16,
    BrowserHome = VK_BROWSER_HOME as u16,
    VolumeMute = VK_VOLUME_MUTE as u16,
    VolumeDown = VK_VOLUME_DOWN as u16,
    VolumeUp = VK_VOLUME_UP as u16,
    MediaNextTrack = VK_MEDIA_NEXT_TRACK as u16,
    MediaPrevTrack = VK_MEDIA_PREV_TRACK as u16,
    MediaStop = VK_MEDIA_STOP as u16,
    MediaPlayPause = VK_MEDIA_PLAY_PAUSE as u16,
    LaunchMail = VK_LAUNCH_MAIL as u16,
    LaunchMediaSelect = VK_LAUNCH_MEDIA_SELECT as u16,
    LaunchApp1 = VK_LAUNCH_APP1 as u16,
    LaunchApp2 = VK_LAUNCH_APP2 as u16,
    Oem1 = VK_OEM_1 as u16,
    OemPlus = VK_OEM_PLUS as u16,
    OemComma = VK_OEM_COMMA as u16,
    OemMinus = VK_OEM_MINUS as u16,
    OemPeriod = VK_OEM_PERIOD as u16,
    Oem2 = VK_OEM_2 as u16,
    Oem3 = VK_OEM_3 as u16,
    Oem4 = VK_OEM_4 as u16,
    Oem5 = VK_OEM_5 as u16,
    Oem6 = VK_OEM_6 as u16,
    Oem7 = VK_OEM_7 as u16,
    Oem8 = VK_OEM_8 as u16,
    Oem102 = VK_OEM_102 as u16,
    ProcessKey = VK_PROCESSKEY as u16,
    Packet = VK_PACKET as u16,
    Attn = VK_ATTN as u16,
    CRSel = VK_CRSEL as u16,
    EXSel = VK_EXSEL as u16,
    ErEOF = VK_EREOF as u16,
    Play = VK_PLAY as u16,
    Zoom = VK_ZOOM as u16,
    NoName = VK_NONAME as u16,
    PA1 = VK_PA1 as u16,
    OemClear = VK_OEM_CLEAR as u16,
}

com_trait! {
    pub trait IAIMPAction: IAIMPPropertyList {
        const IID = {0x41494D50, 0x4163, 0x7469, 0x6F, 0x6E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ActionProp {
    Custom,
    Id,
    Name,
    GroupName,
    Enabled,
    DefaultLocalHotkey,
    DefaultGlobalHotkey,
    DefaultGlobalHotkey2,
    Event,
}

com_trait! {
    pub trait IAIMPActionEvent: IUnknown {
        const IID = {0x41494D50, 0x4163, 0x7469, 0x6F, 0x6E, 0x45, 0x76, 0x65, 0x6E, 0x74, 0x00};

        unsafe fn on_execute(&self, data: Option<ComPtr<dyn IUnknown>>,) -> ();
    }
}

// Decoders

com_trait! {
    pub trait IAIMPServiceAudioDecoders: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7641, 0x75, 0x64, 0x69, 0x6F, 0x44, 0x65, 0x63, 0x00};

        unsafe fn create_decoder_for_stream(
            &self,
            stream: ComRc<dyn IAIMPStream>,
            flags: DWORD,
            error_info: Option<ComPtr<dyn IAIMPErrorInfo>>,
            decoder: *mut ComRc<dyn IAIMPAudioDecoder>,
        ) -> HRESULT;

        unsafe fn create_decoder_for_file_uri(
            &self,
            file_uri: ComPtr<dyn IAIMPString>,
            flags: DWORD,
            error_info: Option<ComPtr<dyn IAIMPErrorInfo>>,
            decoder: *mut ComRc<dyn IAIMPAudioDecoder>,
        ) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPExtensionAudioDecoder: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7441, 0x75, 0x64, 0x69, 0x6F, 0x44, 0x65, 0x63, 0x00};

        unsafe fn create_decoder(
            &self,
            stream: ComRc<dyn IAIMPStream>,
            flags: DecoderFlags,
            error_info: ComPtr<dyn IAIMPErrorInfo>,
            decoder: *mut ComRc<dyn IAIMPAudioDecoder>,
        ) -> WinHRESULT;
    }
}

bitflags! {
    pub struct DecoderFlags: DWORD {
        const NONE = 0;
        const FORCE_CREATE_INSTANCE = 0x1000;
    }
}

com_trait! {
    pub trait IAIMPExtensionAudioDecoderPriority: IUnknown {
        const IID = {0x41494D50, 0x4578, 0x7444, 0x65, 0x63, 0x50, 0x72, 0x69, 0x6F, 0x72, 0x00};

        unsafe fn get_priority(&self,) -> c_int;
    }
}

com_trait! {
    pub trait IAIMPAudioDecoder: IUnknown {
        const IID = {0x41494D50, 0x4175, 0x6469, 0x6F, 0x44, 0x65, 0x63, 0x00, 0x00, 0x00, 0x00};

        unsafe fn get_file_info(&self, file_info: ComPtr<dyn IAIMPFileInfo>,) -> BOOL;

        unsafe fn get_stream_info(
            &self,
            sample_rate: *mut c_int,
            channels: *mut c_int,
            sample_format: *mut SampleFormat,
        ) -> BOOL;

        unsafe fn is_seekable(&self,) -> BOOL;

        unsafe fn is_realtime_stream(&self,) -> BOOL;

        unsafe fn get_available_data(&self,) -> i64;

        unsafe fn get_size(&self,) -> i64;

        unsafe fn get_position(&self,) -> i64;

        unsafe fn set_position(&self, value: i64,) -> BOOL;

        unsafe fn read(&self, buffer: *mut c_void, count: c_int,) -> c_int;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum SampleFormat {
    EightBit = 1,
    SixteenBit,
    TwentyFourBit,
    ThirtyTwoBit,
    ThirtyTwoBitFloat,
}

com_trait! {
    pub trait IAIMPAudioDecoderBufferingProgress: IUnknown {
        const IID = {0x41494D50, 0x4175, 0x6469, 0x6F, 0x44, 0x65, 0x63, 0x42, 0x75, 0x66, 0x66};

        unsafe fn get(&self, value: *mut BufferingProgress,) -> BOOL;
    }
}

#[repr(transparent)]
pub struct BufferingProgress(c_double);

impl BufferingProgress {
    pub fn new(value: c_double) -> Option<Self> {
        if value >= 0.0 && value <= 1.0 {
            Some(Self(value))
        } else {
            None
        }
    }
}

com_trait! {
    pub trait IAIMPAudioDecoderNotifications: IUnknown {
        const IID = {0x41494D50, 0x4175, 0x6469, 0x6F, 0x44, 0x65, 0x63, 0x4E, 0x74, 0x66, 0x79};

        unsafe fn listener_add(&self, listener: ComRc<dyn IAIMPAudioDecoderListener>,) -> ();

        unsafe fn listener_remove(&self, listener: ComRc<dyn IAIMPAudioDecoderListener>,) -> ();
    }
}

com_trait! {
    pub trait IAIMPAudioDecoderListener: IUnknown {
        const IID = {0x41494D50, 0x4175, 0x6469, 0x6F, 0x44, 0x65, 0x63, 0x4C, 0x73, 0x74, 0x00};

        unsafe fn changed(&self, changes: DecoderChange,) -> ();
    }
}

bitflags! {
    pub struct DecoderChange: c_int {
        const NONE = 0;
        const INPUT_FORMAT = 1;
    }
}

#[cfg(test)]
mod tests {
    use std::mem::MaybeUninit;

    use super::*;

    #[test]
    fn com_ptr_size() {
        assert_eq!(
            mem::size_of::<Option<ComPtr<dyn IUnknown>>>(),
            mem::size_of::<*mut *mut <dyn IUnknown as ComInterface>::VTable>()
        );
    }

    #[test]
    fn com_rc_size() {
        assert_eq!(
            mem::size_of::<Option<ComRc<dyn IUnknown>>>(),
            mem::size_of::<*mut *mut <dyn IUnknown as ComInterface>::VTable>()
        );
    }

    com_trait! {
        pub trait A: IUnknown {
            const IID = {0x11111111, 0x4874, 0x7470, 0x43, 0x6C, 0x74, 0x45, 0x76, 0x74, 0x73, 0x00};

            unsafe fn a(&self,) -> ();
        }
    }

    com_trait! {
        pub trait B: IUnknown {
            const IID = {0x44444444, 0x4874, 0x7470, 0x43, 0x6C, 0x74, 0x45, 0x76, 0x74, 0x73, 0x00};

            unsafe fn b(&self,) -> ();
        }
    }

    struct Wrapper;

    impl A for Wrapper {
        unsafe fn a(&self) {}
    }

    impl B for Wrapper {
        unsafe fn b(&self) {}
    }

    impl ComInterfaceQuerier for Wrapper {}

    #[test]
    fn check_inheritance_chain() {
        let wrapper = com_wrapper!(Wrapper => dyn A, dyn B);
        assert_eq!(
            wrapper
                .pointers
                .query_interface(&<dyn IUnknown as ComInterface>::IID),
            Some(&wrapper.pointers.0 as *const _ as *mut _)
        );
        assert_eq!(
            wrapper
                .pointers
                .query_interface(&<dyn A as ComInterface>::IID),
            Some(&wrapper.pointers.0 as *const _ as *mut _)
        );
        assert_eq!(
            wrapper
                .pointers
                .query_interface(&<dyn B as ComInterface>::IID),
            Some(&wrapper.pointers.1 as *const _ as *mut _)
        );
    }

    #[test]
    fn check_iunknown_inheritance() {
        macro_rules! query_interface {
            ($from:ident, $trait:ty) => {{
                let mut tmp = MaybeUninit::<ComPtr<$trait>>::uninit();
                assert_eq!(
                    $from.query_interface(
                        &<$trait as ComInterface>::IID,
                        tmp.as_mut_ptr() as *mut _
                    ),
                    NOERROR
                );
                tmp.assume_init()
            }};
        }

        unsafe {
            let wrapper: ComRc<dyn IUnknown> = com_wrapper!(Wrapper => dyn A, dyn B).into_com_rc();

            let iunknown = query_interface!(wrapper, dyn IUnknown);
            let a = query_interface!(wrapper, dyn A);
            let b = query_interface!(wrapper, dyn B);
            assert_eq!(iunknown, a.clone().cast());

            let a_iunknown = query_interface!(a, dyn IUnknown);
            assert_eq!(iunknown, a_iunknown);

            let b_iunknown = query_interface!(b, dyn IUnknown);
            assert_eq!(iunknown, b_iunknown);
        }
    }

    #[test]
    fn tdatetime_conversions() {
        let delphi_time = TDateTime::unix_start();
        let time: SystemTime = delphi_time.into();
        let other_delphi_time: TDateTime = time.into();
        assert_eq!(delphi_time, other_delphi_time);
    }
}
