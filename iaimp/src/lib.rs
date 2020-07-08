#![allow(clippy::missing_safety_doc)]

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
}

impl<T: ComInterface + ?Sized> fmt::Debug for ComPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
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

impl<T: ComInterface + ?Sized> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}

pub struct ComRc<T: ComInterface + ?Sized>(ComPtr<T>);

impl<T: ComInterface + ?Sized> ComRc<T> {
    pub fn as_raw(&self) -> &ComPtr<T> {
        &self.0
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

pub trait ComVTables {
    type Pointers: ComPointers;
}

pub trait ComPointers: Sized {
    fn dealloc(&self);
}

macro_rules! com_pointers {
    ($( $fields:tt: $generics:ident ),+) => {
        impl<$( $generics ),+> ComPointers for ($( *mut $generics, )+) {
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
pub struct ComWrapper<T: ComVTables> {
    pointers: T::Pointers,
    counter: Cell<u32>,
    inner: T,
}

impl<T: ComVTables> ComWrapper<T> {
    pub fn new(inner: T, pointers: T::Pointers) -> Self {
        Self {
            pointers,
            counter: Cell::new(0),
            inner,
        }
    }

    pub unsafe fn into_com_ptr<U: ComInterface + ?Sized>(self) -> ComPtr<U> {
        let ptr = Box::into_raw(Box::new(self));
        mem::transmute(ptr)
    }
}

impl<T: ComVTables> IUnknown for ComWrapper<T> {
    unsafe fn query_interface(&self, riid: *const GUID, ppv: *mut *mut c_void) -> WinHRESULT {
        let riid = &*riid;
        if *riid == <dyn IUnknown as ComInterface>::IID
        //|| <Self as ComInterface>::check_inheritance_chain(riid)
        {
            //*ppv = &self.vptr as *const _ as *mut c_void;
        } else {
            *ppv = ptr::null_mut::<c_void>();
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
        }
        value
    }
}

#[macro_export(local_inner_macros)]
macro_rules! com_wrapper {
    ($value:expr => $t:ty: $( $table:ty ),+) => {{
        let pointers = ( $( Box::into_raw(Box::new(<$table>::new::<$t>())), )+ );
        let wrapper = $crate::ComWrapper::new($value, pointers);
        unsafe {
            wrapper.add_ref();
        }
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

        impl<T: ComInterface + $trait_name + ?Sized> $trait_name for ComRc<T> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    $trait_name::$func(&self.0, $( $arg_name, )*)
                }
            )*
        }

        paste::item! {
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
                type VTable = [< $trait_name VTable >];
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
            pub fn new<T: ComVTables>() -> Self {
                Self {
                    $( $func: Self::$func::<T>, )*
                }
            }

            $(
                unsafe extern "stdcall" fn $func<T: ComVTables>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
                    let this = this as *mut ComWrapper<T>;
                    (*this).$func($( $arg_name ),*)
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
        impl<T: ComVTables + $trait_name> $trait_name for ComWrapper<T> {
            $(
                unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                    self.inner.$func($( $arg_name, )*)
                }
            )*
        }

        paste::item! {
            #[repr(C)]
            pub struct [< $trait_name VTable >] {
                _base: [< $base VTable >],
                $( $func: unsafe extern "stdcall" fn(this: *mut *const Self, $( $arg_ty ),*) -> $ret, )*
            }

            impl<T: ComInterface + $trait_name + ?Sized> $trait_name for ComPtr<T> {
                $(
                    unsafe fn $func(&self, $( $arg_name: $arg_ty, )*) -> $ret {
                        let vptr = self.inner.as_ptr() as *mut *const [< $trait_name VTable >];
                        ((**vptr).$func)(vptr, $( $arg_name, )*)
                    }
                )*
            }

            impl [< $trait_name VTable >] {
                pub fn new<T: ComVTables + $trait_name>() -> Self {
                    Self {
                        _base: [< $base VTable >] ::new::<T>(),
                        $( $func: Self::$func::<T>, )*
                    }
                }

                $(
                    unsafe extern "stdcall" fn $func<T: ComVTables + $trait_name>(this: *mut *const Self, $( $arg_name: $arg_ty ),*) -> $ret {
                        let this = this as *mut ComWrapper<T>;
                        (*this).$func($( $arg_name ),*)
                    }
                )*
            }
        }
    };
}

#[macro_export]
macro_rules! com_impl {
    (
        $vis:vis struct $name:ident $( < $( $generics:ident ),* > )? : $impl_vtable:ident {
            $( $field_name:ident : $field_ty:ty, )*
        }
    ) => {
        paste::item! {
            $vis struct $name $( < $( $generics ),* > )? {
                $( $field_name: $field_ty, )*
            }

            #[repr(C)]
            struct [< Raw $name >] $( < $( $generics ),* > )? {
                vptr: *mut [< $impl_vtable VTable >],
                counter: std::cell::Cell<u32>,
                inner: $name $( < $( $generics ),* > )?,
            }

            impl $( < $( $generics ),* > )? IUnknown for $name $( < $( $generics ),* > )? {
                unsafe fn query_interface(&self, riid: *const com::sys::GUID, ppv: *mut *mut std::os::raw::c_void) -> i32 {
                    unreachable!()
                }

                unsafe fn add_ref(&self) -> u32 {
                    unreachable!()
                }

                unsafe fn release(&self) -> u32 {
                    unreachable!()
                }
            }

            impl $( < $( $generics ),* > )? [< Raw $name >] $( < $( $generics ),* > )? {
                fn new(inner: $name $( < $( $generics ),* > )? ) -> Self {
                    Self {
                        vptr: Box::into_raw(Box::new([< $impl_vtable VTable >]::new::<$name>())),
                        counter: std::cell::Cell::new(0),
                        inner,
                    }
                }
            }

            impl $( < $( $generics ),* > )? IUnknown for [< Raw $name >] $( < $( $generics ),* > )? {
                unsafe fn query_interface(&self, riid: *const com::sys::GUID, ppv: *mut *mut std::os::raw::c_void) -> i32 {
                    let riid = &*riid;
                    if riid == &com::interfaces::iunknown::IID_IUNKNOWN
                        || <dyn $impl_vtable as com::ComInterface>::is_iid_in_inheritance_chain(riid)
                    {
                        *ppv = &self.vptr as *const _ as *mut std::ffi::c_void;
                    } else {
                        *ppv = std::ptr::null_mut::<std::ffi::c_void>();
                        return com::sys::E_NOINTERFACE;
                    }
                    self.add_ref();
                    com::sys::NOERROR
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
                        Box::from_raw(self.vptr);
                    }
                    value
                }
            }
        }
    };
}

// we define our HRESULT with `must_use` attribute to not forget to handle it
#[repr(transparent)]
#[must_use]
pub struct HRESULT(WinHRESULT);

impl Deref for HRESULT {
    type Target = WinHRESULT;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// workaround issue #60553
#[repr(C)]
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
            Other($int),
        }

        paste::item! {
            pub type [< $name Wrapper >] = EnumWrapper<$name, $int>;
        }

        impl EnumWrapper<$name, $int> {
            pub fn into_inner(self) -> $name {
                match self.value {
                    $( $discriminant => $name::$variant, )*
                    x => $name::Other(x),
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
            details: *mut ComRc<dyn IAIMPString>,
        ) -> HRESULT;

        unsafe fn get_info_formatted(&self, s: *mut ComRc<dyn IAIMPString>,) -> HRESULT;

        unsafe fn set_info(
            &self,
            error_code: c_int,
            message: ComRc<dyn IAIMPString>,
            details: ComRc<dyn IAIMPString>,
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
        const IID = {0x41494D50, 0x496D, 0x6167, 0x65, 032, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};

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

        unsafe fn get_data(&self,) -> *mut c_void;
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
            value: *mut *mut c_void,
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

        unsafe fn seek(&self, offset: i64, mode: StreamSeekMode,) -> HRESULT;

        unsafe fn read(&self, buffer: *mut c_uchar, count: c_uchar,) -> c_int;

        unsafe fn write(&self, buffer: *mut c_uchar, count: c_uchar, written: *mut c_uchar,) -> HRESULT;
    }
}

#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum StreamSeekMode {
    FromBeginning = 0,
    FromCurrent = 1,
    FromEnd = 2,
}

// Threading

com_trait! {
    pub trait IAIMPServiceThreads: IUnknown {
        const IID = {0x41494D50, 0x5372, 0x7654, 0x68, 0x72, 0x65, 0x61, 0x64, 0x73, 0x00, 0x00};

        unsafe fn execute_in_main_thread(
            &self,
            task: ComPtr<dyn IAIMPTask>,
            flags: ServiceThreadsFlags,
        ) -> HRESULT;

        unsafe fn execute_in_thread(
            &self,
            task: ComPtr<dyn IAIMPTask>,
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
