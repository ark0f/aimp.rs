#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

use bitflags::bitflags;
use std::{
    cell::Cell,
    fmt,
    marker::PhantomData,
    mem,
    ops::Deref,
    os::raw::{c_double, c_float, c_int, c_uchar, c_void},
    ptr,
    ptr::NonNull,
};
use winapi::{
    shared::{
        basetsd::DWORD_PTR,
        guiddef::{GUID as WinGUID, REFIID},
        minwindef::{BOOL, DWORD, HMODULE},
        windef::{HBITMAP, HDC, HWND, RECT, SIZE},
        winerror::{E_NOINTERFACE, HRESULT as WinHRESULT, NOERROR},
    },
    um::{
        oaidl::VARIANT,
        wingdi::RGBQUAD,
        winnt::{PWCHAR, WCHAR},
    },
};

// COM code based on https://github.com/microsoft/com-rs

pub struct GUID(WinGUID);
pub type IID = GUID;

impl PartialEq for GUID {
    fn eq(&self, other: &Self) -> bool {
        self.0.Data1 == other.0.Data1
            && self.0.Data2 == other.0.Data2
            && self.0.Data3 == other.0.Data3
            && self.0.Data4 == other.0.Data4
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

pub struct ComRc<T: ComInterface + ?Sized>(ComPtr<T>);

impl<T: ComInterface + ?Sized> fmt::Debug for ComRc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<T: ComInterface + ?Sized> ComRc<T> {
    pub unsafe fn cast<U: ComInterface + ?Sized>(self) -> ComRc<U> {
        mem::transmute(self)
    }

    pub fn as_raw(&self) -> ComPtr<T> {
        self.0.clone()
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

pub struct ZeroOffset;

impl ComOffset for ZeroOffset {
    const VALUE: usize = 0;
}

pub struct OneOffset;

impl ComOffset for OneOffset {
    const VALUE: usize = 1;
}

pub trait ComPointers: fmt::Debug + Sized {
    fn query_interface(&self, riid: &IID) -> Option<*mut c_void>;

    fn dealloc(&self);
}

macro_rules! com_pointers {
    ($( $fields:tt: $generics:ident ),+) => {
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
    };
}

com_pointers!(0: T);
com_pointers!(0: T, 1: U);

#[repr(C)]
pub struct ComWrapper<T, U> {
    pointers: U,
    counter: Cell<u32>,
    inner: T,
}

impl<T, U: ComPointers> ComWrapper<T, U> {
    pub fn new(inner: T, pointers: U) -> Self {
        Self {
            pointers,
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

impl<T, U: ComPointers> IUnknown for ComWrapper<T, U> {
    unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void) -> WinHRESULT {
        let riid = &*riid;
        if let Some(ptr) = self.pointers.query_interface(riid) {
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

#[macro_export(local_inner_macros)]
macro_rules! com_wrapper {
    ($value:expr => $t:ty: $( $traits:ty ),+) => {{
        type Pointers = ( $( *mut <$traits as $crate::ComInterface>::VTable, )+ );

        let pointers = com_wrapper!(@alloc $t => $( $traits ),+);
        let wrapper = $crate::ComWrapper::new($value, pointers);
        wrapper
    }};
    (@box $t:ty, $t1:ty, $offset:ty) => {
        Box::into_raw(Box::new(<$t1 as $crate::ComProdInterface<$t, Pointers, $offset>>::new_vtable()))
    };
    (@alloc $t:ty => $t1:ty) => {
        (com_wrapper!(@box $t, $t1, $crate::ZeroOffset),)
    };
    (@alloc $t:ty => $t1:ty, $t2:ty) => {
        (com_wrapper!(@box $t, $t1, $crate::ZeroOffset), com_wrapper!(@box $t, $t2, $crate::OneOffset))
    }
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

        impl<T: ComInterface + $trait_name + ?Sized> $trait_name for ComRc<T> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    $trait_name::$func(&self.0, $( $arg_name, )*)
                }
            )*
        }

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
                unsafe extern "stdcall" fn $func<T, U: ComPointers, O: ComOffset>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
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

        impl<T, U: ComPointers, O: ComOffset> ComProdInterface<T, U, O> for dyn IUnknown {
            type VTable = IUnknownVTable;

            fn new_vtable() -> Self::VTable {
                Self::VTable {
                    $( $func: Self::VTable::$func::<T, U, O>, )*
                }
            }
        }

        impl<T: ComInterface + ?Sized> IUnknown for ComPtr<T> {
            unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void) -> i32 {
                let vptr = std::dbg!(self.inner.as_ptr()) as *mut *const IUnknownVTable;
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
    };
    (
        @rest $trait_name:ident: $base:ident;
        impl ComPtr {
            $( unsafe fn $func:ident(&self, $( $arg_name:ident: $arg_ty:ty, )*) -> $ret:ty; )*
        }
    ) => {
        impl<T: $trait_name, U> $trait_name for ComWrapper<T, U> {
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

        paste::item! {
            #[repr(C)]
            pub struct [< $trait_name VTable >] {
                _base: [< $base VTable >],
                $( $func: unsafe extern "stdcall" fn(this: *mut *const Self, $( $arg_ty ),*) -> $ret, )*
            }

            impl [< $trait_name VTable >] {
                $(
                    unsafe extern "stdcall" fn $func<T: $trait_name, U: ComPointers, O: ComOffset>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
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

            impl<T: $trait_name, U: ComPointers, O: ComOffset> ComProdInterface<T, U, O> for dyn $trait_name {
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
pub struct HRESULT(WinHRESULT);

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
            data: ComPtr<dyn IUnknown>,
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

#[repr(u32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PluginCategory {
    Addons = 0x1,
    Decoders = 0x2,
    Visuals = 0x4,
    Dsp = 0x8,
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

        unsafe fn get_object(&self, index: c_int, iid: REFIID, obj: *mut *mut c_void,) -> HRESULT;

        unsafe fn set_object(&self, index: c_int, obj: ComRc<dyn IUnknown>,) -> HRESULT;
    }
}

com_trait! {
    pub trait IAIMPProgressCallback: IUnknown {
        const IID = {0x41494D50, 0x5072, 0x6F67, 0x72, 0x65, 0x73, 0x73, 0x43, 0x42, 0x00, 0x00};

        unsafe fn process(&self, progress: c_float, canceled: *mut bool,) -> HRESULT;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::MaybeUninit;

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

    #[test]
    fn check_inheritance_chain() {
        let wrapper = com_wrapper!(Wrapper => Wrapper: dyn A, dyn B);
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
            let wrapper: ComRc<dyn IUnknown> =
                com_wrapper!(Wrapper => Wrapper: dyn A, dyn B).into_com_rc();

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
}
