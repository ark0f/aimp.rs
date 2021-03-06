use crate::{
    actions::ACTION_MANAGER_SERVICE,
    core::CORE,
    decoders::AUDIO_DECODERS,
    file::{
        FILE_FORMATS, FILE_INFO_FORMATTER, FILE_INFO_FORMATTER_UTILS, FILE_INFO_SERVICE,
        FILE_STREAMING, FILE_SYSTEMS, FILE_URI_SERVICE,
    },
    internet::{CONNECTION_SETTINGS, HTTP_CLIENT},
    msg_box,
    threading::THREADS,
    util::ToWide,
};
use iaimp::{
    ComInterface, ComInterfaceQuerier, ComPtr, IAIMPCore, IAIMPPlugin, IAIMPServiceActionManager,
    IAIMPServiceAudioDecoders, IAIMPServiceConnectionSettings, IAIMPServiceFileFormats,
    IAIMPServiceFileInfo, IAIMPServiceFileInfoFormatter, IAIMPServiceFileInfoFormatterUtils,
    IAIMPServiceFileStreaming, IAIMPServiceFileSystems, IAIMPServiceFileURI2,
    IAIMPServiceHTTPClient2, IAIMPServiceThreads, IUnknown, PluginCategory, PluginInfoWrapper,
    SystemNotification, SystemNotificationWrapper,
};
use std::{
    cell::Cell, error::Error as StdError, mem::MaybeUninit, ptr, result::Result as StdResult,
};
use winapi::{
    shared::winerror::{E_FAIL, HRESULT, NOERROR, S_OK},
    um::winnt::PWCHAR,
};

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
    pub category: fn() -> PluginCategory,
}

pub struct PluginWrapper<T> {
    inner: Cell<Option<T>>,
    info: PluginWrapperInfo,
}

impl<T: Plugin> PluginWrapper<T> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            inner: Cell::new(None),
            info: PluginWrapperInfo::new::<T>(),
        }
    }
}

impl<T: Plugin> IAIMPPlugin for PluginWrapper<T> {
    unsafe fn info_get(&self, index: PluginInfoWrapper) -> PWCHAR {
        let p = match index.into_inner() {
            Some(iaimp::PluginInfo::Name) => self.info.name.as_ptr(),
            Some(iaimp::PluginInfo::Author) => self.info.author.as_ptr(),
            Some(iaimp::PluginInfo::ShortDescription) => self.info.short_description.as_ptr(),
            Some(iaimp::PluginInfo::FullDescription) => self
                .info
                .full_description
                .as_ref()
                .map_or(ptr::null(), Vec::as_ptr),
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

        AUDIO_DECODERS.init(core.query_object());

        drop(core);

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
            AUDIO_DECODERS: IAIMPServiceAudioDecoders,
        );
    }
}

impl<T> ComInterfaceQuerier for PluginWrapper<T> {}

#[derive(Debug)]
struct PluginWrapperInfo {
    name: Vec<u16>,
    author: Vec<u16>,
    short_description: Vec<u16>,
    full_description: Option<Vec<u16>>,
    category: PluginCategory,
}

impl PluginWrapperInfo {
    fn new<T: Plugin>() -> Self {
        let info = T::INFO;
        Self {
            name: info.name.to_wide_null(),
            author: info.author.to_wide_null(),
            short_description: info.short_description.to_wide_null(),
            full_description: info.full_description.map(ToWide::to_wide_null),
            category: (info.category)(),
        }
    }
}
