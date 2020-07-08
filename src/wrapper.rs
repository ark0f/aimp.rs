use crate::{core, msg_box, threading, util::ToWide, Plugin};
use iaimp::IAIMPPluginVTable;
use iaimp::{
    ComPtr, ComVTables, IAIMPCore, IAIMPPlugin, IUnknown, PluginCategory, PluginInfoWrapper,
    SystemNotificationWrapper,
};
use std::{cell::Cell, ptr};
use winapi::{
    shared::winerror::{E_FAIL, HRESULT, S_OK},
    um::winnt::PWCHAR,
};

pub struct Wrapper<T> {
    inner: Cell<Option<T>>,
    info: WrapperInfo,
}

impl<T> ComVTables for Wrapper<T> {
    type Pointers = (*mut IAIMPPluginVTable,);
}

impl<T: Plugin> Wrapper<T> {
    pub fn new() -> Self {
        Self {
            inner: Cell::new(None),
            info: WrapperInfo::new::<T>(),
        }
    }
}

impl<T: Plugin> IAIMPPlugin for Wrapper<T> {
    unsafe fn info_get(&self, index: PluginInfoWrapper) -> PWCHAR {
        let p = match index.into_inner() {
            iaimp::PluginInfo::Name => self.info.name.as_ptr(),
            iaimp::PluginInfo::Author => self.info.author.as_ptr(),
            iaimp::PluginInfo::ShortDescription => self.info.short_description.as_ptr(),
            iaimp::PluginInfo::FullDescription => self
                .info
                .full_description
                .as_ref()
                .map(Vec::as_ptr)
                .unwrap_or(ptr::null()),
            _ => ptr::null(),
        };
        p as *mut _
    }

    unsafe fn info_get_categories(&self) -> PluginCategory {
        self.info.category
    }

    unsafe fn initialize(&self, core: ComPtr<dyn IAIMPCore>) -> HRESULT {
        core::init(core);

        let core = core::get();
        threading::init(core.query_object());

        match T::new(core.clone()) {
            Ok(plugin) => {
                self.inner.set(Some(plugin));
                S_OK
            }
            Err(err) => {
                msg_box!("{}", err);
                E_FAIL
            }
        }
    }

    unsafe fn finalize(&self) -> HRESULT {
        match self.inner.take().unwrap().finish() {
            Ok(()) => S_OK,
            Err(err) => {
                msg_box!("{}", err);
                E_FAIL
            }
        }
    }

    unsafe fn system_notification(
        &self,
        _notify_id: SystemNotificationWrapper,
        _data: ComPtr<dyn IUnknown>,
    ) {
    }
}

#[derive(Debug)]
struct WrapperInfo {
    name: Vec<u16>,
    author: Vec<u16>,
    short_description: Vec<u16>,
    full_description: Option<Vec<u16>>,
    category: PluginCategory,
}

impl WrapperInfo {
    fn new<T: Plugin>() -> Self {
        let info = T::INFO;
        Self {
            name: info.name.to_wide_null(),
            author: info.author.to_wide_null(),
            short_description: info.short_description.to_wide_null(),
            full_description: info.full_description.map(ToWide::to_wide_null),
            category: info.category,
        }
    }
}
