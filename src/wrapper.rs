use crate::{
    actions::ACTION_MANAGER_SERVICE,
    core::CORE,
    file::{
        FILE_FORMATS, FILE_INFO_FORMATTER, FILE_INFO_FORMATTER_UTILS, FILE_INFO_SERVICE,
        FILE_STREAMING, FILE_SYSTEMS, FILE_URI_SERVICE,
    },
    internet::{CONNECTION_SETTINGS, HTTP_CLIENT},
    msg_box,
    threading::THREADS,
    util::ToWide,
    Plugin,
};
use iaimp::{
    ComInterface, ComInterfaceQuerier, ComPtr, IAIMPCore, IAIMPPlugin, IAIMPServiceActionManager,
    IAIMPServiceConnectionSettings, IAIMPServiceFileFormats, IAIMPServiceFileInfo,
    IAIMPServiceFileInfoFormatter, IAIMPServiceFileInfoFormatterUtils, IAIMPServiceFileStreaming,
    IAIMPServiceFileSystems, IAIMPServiceFileURI2, IAIMPServiceHTTPClient2, IAIMPServiceThreads,
    IUnknown, PluginCategory, PluginInfoWrapper, SystemNotification, SystemNotificationWrapper,
};
use std::{cell::Cell, mem::MaybeUninit, ptr};
use winapi::{
    shared::winerror::{E_FAIL, HRESULT, NOERROR, S_OK},
    um::winnt::PWCHAR,
};

pub struct Wrapper<T> {
    inner: Cell<Option<T>>,
    info: WrapperInfo,
}

impl<T: Plugin> Wrapper<T> {
    #[allow(clippy::new_without_default)]
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
            Some(iaimp::PluginInfo::Name) => self.info.name.as_ptr(),
            Some(iaimp::PluginInfo::Author) => self.info.author.as_ptr(),
            Some(iaimp::PluginInfo::ShortDescription) => self.info.short_description.as_ptr(),
            Some(iaimp::PluginInfo::FullDescription) => self
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
        CORE.init(core);
        let core = CORE.get();
        THREADS.init(core.query_object());
        CONNECTION_SETTINGS.init(core.query_object());
        HTTP_CLIENT.init(core.query_object());
        ACTION_MANAGER_SERVICE.init(core.query_object());

        FILE_FORMATS.init(core.query_object());
        FILE_INFO_SERVICE.init(core.query_object());
        FILE_INFO_FORMATTER.init(core.query_object());
        FILE_INFO_FORMATTER_UTILS.init(core.query_object());
        FILE_STREAMING.init(core.query_object());
        FILE_URI_SERVICE.init(core.query_object());
        FILE_SYSTEMS.init(core.query_object());

        match T::new() {
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
        notify_id: SystemNotificationWrapper,
        data: Option<ComPtr<dyn IUnknown>>,
    ) {
        let data = if let Some(data) = data { data } else { return };
        let init = match notify_id.into_inner() {
            Some(SystemNotification::ServiceAdded) => true,
            Some(SystemNotification::ServiceRemoved) => false,
            _ => return,
        };

        macro_rules! match_service {
            ($( $service:ident: $interface:ident, )+) => {{
                let mut ppv = MaybeUninit::uninit();
                $(
                    if data.query_interface(&<dyn $interface>::IID as *const _, ppv.as_mut_ptr()) == NOERROR {
                        if init {
                            $service.init(CORE.get().query_object());
                        } else {
                            $service.deinit();
                        }
                    } else
                )+
                {}
            }};
        }

        match_service!(
            THREADS: IAIMPServiceThreads,
            CONNECTION_SETTINGS: IAIMPServiceConnectionSettings,
            HTTP_CLIENT: IAIMPServiceHTTPClient2,
            ACTION_MANAGER_SERVICE: IAIMPServiceActionManager,
            FILE_FORMATS: IAIMPServiceFileFormats,
            FILE_INFO_SERVICE: IAIMPServiceFileInfo,
            FILE_INFO_FORMATTER: IAIMPServiceFileInfoFormatter,
            FILE_INFO_FORMATTER_UTILS: IAIMPServiceFileInfoFormatterUtils,
            FILE_STREAMING: IAIMPServiceFileStreaming,
            FILE_URI_SERVICE: IAIMPServiceFileURI2,
            FILE_SYSTEMS: IAIMPServiceFileSystems,
        );
    }
}

impl<T> ComInterfaceQuerier for Wrapper<T> {}

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
