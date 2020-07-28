use crate::{error::HresultExt, AimpString};
use dashmap::DashMap;
use iaimp::{
    ComInterface, ComRc, IAIMPPropertyList, IAIMPString, IUnknown, TDateTime, HRESULT, IID,
};
use std::mem::MaybeUninit;
use winapi::shared::winerror::{E_FAIL, E_INVALIDARG, E_NOTIMPL, NOERROR, S_OK};

#[derive(Debug, Default, Clone)]
pub struct HashedPropertyList {
    floats: DashMap<i32, f64>,
    i32s: DashMap<i32, i32>,
    i64s: DashMap<i32, i64>,
    objects: DashMap<i32, ComRc<dyn IUnknown>>,
}

impl IAIMPPropertyList for HashedPropertyList {
    unsafe fn begin_update(&self) {}

    unsafe fn end_update(&self) {}

    unsafe fn reset(&self) -> HRESULT {
        HRESULT(S_OK)
    }

    unsafe fn get_value_as_float(&self, property_id: i32, value: *mut f64) -> HRESULT {
        let f = self
            .floats
            .remove(&property_id)
            .map(|(_, v)| v)
            .unwrap_or(0.0);
        *value = f;
        HRESULT(S_OK)
    }

    unsafe fn get_value_as_int32(&self, property_id: i32, value: *mut i32) -> HRESULT {
        let i = self.i32s.remove(&property_id).map(|(_, v)| v).unwrap_or(0);
        *value = i;
        HRESULT(S_OK)
    }

    unsafe fn get_value_as_int64(&self, property_id: i32, value: *mut i64) -> HRESULT {
        let i = self.i64s.remove(&property_id).map(|(_, v)| v).unwrap_or(0);
        *value = i;
        HRESULT(S_OK)
    }

    unsafe fn get_value_as_object(
        &self,
        property_id: i32,
        iid: *const IID,
        value: *mut ComRc<dyn IUnknown>,
    ) -> HRESULT {
        let res = self
            .objects
            .remove(&property_id)
            .map(|(_, v)| v)
            .filter(|obj| {
                let mut ppv = MaybeUninit::uninit();
                obj.query_interface(iid, ppv.as_mut_ptr()) == NOERROR
            })
            .map(|obj| {
                *value = obj;
                S_OK
            })
            .unwrap_or(E_INVALIDARG);
        HRESULT(res)
    }

    unsafe fn set_value_as_float(&self, property_id: i32, value: f64) -> HRESULT {
        self.floats.insert(property_id, value);
        HRESULT(S_OK)
    }

    unsafe fn set_value_as_int32(&self, property_id: i32, value: i32) -> HRESULT {
        self.i32s.insert(property_id, value);
        HRESULT(S_OK)
    }

    unsafe fn set_value_as_int64(&self, property_id: i32, value: i64) -> HRESULT {
        self.i64s.insert(property_id, value);
        HRESULT(S_OK)
    }

    unsafe fn set_value_as_object(&self, property_id: i32, value: ComRc<dyn IUnknown>) -> HRESULT {
        self.objects.insert(property_id, value);
        HRESULT(S_OK)
    }
}

pub struct PropertyList<T: IAIMPPropertyList>(pub(crate) T);

impl<T: IAIMPPropertyList> PropertyList<T> {
    pub fn get<U: PropertyListAccessor<T>>(&self, id: i32) -> U {
        U::get(id, self)
    }

    pub fn update(&mut self) -> PropertyListGuard<'_, T> {
        unsafe {
            self.0.begin_update();
        }
        PropertyListGuard(self)
    }
}

impl<T: IAIMPPropertyList> From<T> for PropertyList<T> {
    fn from(inner: T) -> Self {
        Self(inner)
    }
}

pub struct PropertyListGuard<'a, T: IAIMPPropertyList>(&'a mut PropertyList<T>);

impl<T: IAIMPPropertyList> PropertyListGuard<'_, T> {
    pub fn set<U: PropertyListAccessor<T>>(&mut self, id: i32, prop: U) -> &mut Self {
        prop.set(id, self.0);
        self
    }
}

impl<T: IAIMPPropertyList> Drop for PropertyListGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            (self.0).0.end_update();
        }
    }
}

pub trait PropertyListAccessor<T: IAIMPPropertyList>: Sized {
    fn get(id: i32, list: &PropertyList<T>) -> Self;

    fn set(self, id: i32, list: &mut PropertyList<T>);
}

#[macro_export(local_inner_macros)]
macro_rules! impl_prop_accessor {
    ($ty:ident) => {
        impl<T: iaimp::IAIMPPropertyList> $crate::prop_list::PropertyListAccessor<T>
            for Option<$ty>
        {
            fn get(id: i32, list: &PropertyList<T>) -> Self {
                Option::<_>::get(id, list).map($ty)
            }

            fn set(self, id: i32, list: &mut PropertyList<T>) {
                self.as_deref().copied().set(id, list)
            }
        }

        impl<T: iaimp::IAIMPPropertyList> $crate::prop_list::PropertyListAccessor<T> for $ty {
            fn get(id: i32, list: &PropertyList<T>) -> Self {
                Option::<Self>::get(id, list).unwrap()
            }

            fn set(self, id: i32, list: &mut PropertyList<T>) {
                Some(self).set(id, list)
            }
        }
    };
}

impl_prop_accessor!(TDateTime);

macro_rules! impl_accessor_get_set {
    ($prop:ty, $get:ident, $set:ident) => {
        impl<T: IAIMPPropertyList> PropertyListAccessor<T> for Option<$prop> {
            fn get(id: i32, list: &PropertyList<T>) -> Self {
                unsafe {
                    let mut value = MaybeUninit::uninit();
                    let res = (list.0).$get(id, value.as_mut_ptr());
                    // E_FAIL result is not documented, but it looks like
                    // there is no data for prop
                    if res == E_FAIL {
                        None
                    } else {
                        res.into_result().unwrap();
                        Some(value.assume_init())
                    }
                }
            }

            fn set(self, id: i32, list: &mut PropertyList<T>) {
                unsafe {
                    if let Some(this) = self {
                        (list.0).$set(id, this).into_result().unwrap();
                    }
                }
            }
        }

        impl<T: IAIMPPropertyList> PropertyListAccessor<T> for $prop {
            fn get(id: i32, list: &PropertyList<T>) -> Self {
                Option::<Self>::get(id, list).unwrap()
            }

            fn set(self, id: i32, list: &mut PropertyList<T>) {
                Some(self).set(id, list)
            }
        }
    };
}

impl_accessor_get_set!(f64, get_value_as_float, set_value_as_float);
impl_accessor_get_set!(i32, get_value_as_int32, set_value_as_int32);
impl_accessor_get_set!(i64, get_value_as_int64, set_value_as_int64);

impl<T: IAIMPPropertyList, U: ComInterface + ?Sized> PropertyListAccessor<T> for Option<ComRc<U>> {
    fn get(id: i32, list: &PropertyList<T>) -> Self {
        unsafe {
            let mut value = MaybeUninit::uninit();
            let res = list.0.get_value_as_object(
                id,
                &U::IID as *const IID as *const _,
                value.as_mut_ptr(),
            );
            // E_NOTIMPL can be returned according to docs. Yep, this is crutch
            // E_INVALIDARG is not docs, but returns when field is not set
            if res == E_FAIL || res == E_NOTIMPL || res == E_INVALIDARG {
                None
            } else {
                res.into_result().unwrap();
                Some(value.assume_init().cast())
            }
        }
    }

    fn set(self, id: i32, list: &mut PropertyList<T>) {
        unsafe {
            if let Some(this) = self {
                list.0
                    .set_value_as_object(id, this.cast())
                    .into_result()
                    .unwrap();
            }
        }
    }
}

impl<T: IAIMPPropertyList, U: ComInterface + ?Sized> PropertyListAccessor<T> for ComRc<U> {
    fn get(id: i32, list: &PropertyList<T>) -> Self {
        Option::<Self>::get(id, list).unwrap()
    }

    fn set(self, id: i32, list: &mut PropertyList<T>) {
        Some(self).set(id, list)
    }
}

impl<T: IAIMPPropertyList> PropertyListAccessor<T> for AimpString {
    fn get(id: i32, list: &PropertyList<T>) -> Self {
        Self(ComRc::<dyn IAIMPString>::get(id, list))
    }

    fn set(self, id: i32, list: &mut PropertyList<T>) {
        self.0.set(id, list)
    }
}

impl<T: IAIMPPropertyList> PropertyListAccessor<T> for bool {
    fn get(id: i32, list: &PropertyList<T>) -> Self {
        i32::get(id, list) != 0
    }

    fn set(self, id: i32, list: &mut PropertyList<T>) {
        (self as i32).set(id, list)
    }
}

#[macro_export]
macro_rules! prop_list {
    (
        list: $name:ident($interface:ty),
        prop: $prop:ident,
        guard: $guard:ident,
        methods: $(
            $func:ident($field:ident) -> $ty:ty,
        )+
        $(

            => fields: $(
                $struct_field:ident: $struct_ty:ty,
            )+
        )?
    ) => {
        pub struct $name {
            prop_list: $crate::prop_list::PropertyList<$interface>,
            $(
                $(
                    $struct_field: $struct_ty,
                )*
            )?
        }

        impl $name {
            pub fn update(&mut self) -> $guard {
                $guard(self.prop_list.update())
            }

            $(
                pub fn $func(&self) -> $ty {
                    self.prop_list.get($prop::$field as i32)
                }
            )+
        }

        pub struct $guard<'a>($crate::prop_list::PropertyListGuard<'a, $interface>);

        impl $guard<'_> {
            $(
                pub fn $func(&mut self, value: $ty) -> &mut Self {
                    self.0.set($prop::$field as i32, value);
                    self
                }
            )+
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.debug_struct(&std::stringify!($name))
                    $(
                        .field(&std::stringify!($func), &self.$func())
                    )+
                    .finish()
            }
        }
    };
}
