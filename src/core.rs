use crate::{error::HresultExt, util::Service, AimpString, Result};
use iaimp::{ComInterface, ComPtr, ComRc, CorePath, IAIMPCore, IUnknown, IID};
use std::mem::MaybeUninit;

pub static CORE: Service<Core> = Service::without_lock();

#[derive(Debug, Clone)]
pub struct Core(ComPtr<dyn IAIMPCore>);

impl Core {
    pub(crate) fn create<T: ComInterface + ?Sized>(&self) -> Result<ComRc<T>> {
        unsafe {
            let mut obj = MaybeUninit::uninit();
            self.0
                .create_object(&T::IID as *const _ as *const _, obj.as_mut_ptr() as _)
                .into_result()?;
            Ok(obj.assume_init())
        }
    }

    pub(crate) fn query_object<T: ComInterface + ?Sized>(&self) -> ComPtr<T> {
        unsafe {
            let mut ptr = MaybeUninit::uninit();
            self.0
                .query_interface(&T::IID, ptr.as_mut_ptr())
                .into_result()
                .unwrap();
            let ptr = ptr.assume_init();
            ComPtr::from_ptr(ptr as _)
        }
    }

    pub fn register_extension<T, U>(&self, extension: T)
    where
        T: Extension + Into<ComRc<U>>,
        U: ComInterface + ?Sized,
    {
        unsafe {
            self.0
                .register_extension(
                    &T::SERVICE_IID as *const _ as *const _,
                    extension.into().cast(),
                )
                .into_result()
                .unwrap();
        }
    }

    pub fn path(&self, path: CorePath) -> AimpString {
        unsafe {
            let mut s = MaybeUninit::uninit();
            self.0.get_path(path, s.as_mut_ptr()).into_result().unwrap();
            AimpString(s.assume_init())
        }
    }
}

impl From<ComPtr<dyn IAIMPCore>> for Core {
    fn from(ptr: ComPtr<dyn IAIMPCore>) -> Self {
        Self(ptr)
    }
}

pub trait Extension {
    const SERVICE_IID: IID;
}
