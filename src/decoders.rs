pub use iaimp::BufferingProgress;
pub use iaimp::DecoderChange;
pub use iaimp::SampleFormat;

use crate::core::Extension;
use crate::error::HresultExt;
use crate::file::{FileInfo, FileUri};
use crate::stream::Stream;
use crate::util::Service;
use crate::AimpString;
use crate::Result;
use crate::{ErrorInfo, ErrorInfoContent};
use iaimp::{
    com_wrapper, ComInterface, ComInterfaceQuerier, ComPtr, ComRc, DecoderFlags, IAIMPAudioDecoder,
    IAIMPAudioDecoderBufferingProgress, IAIMPAudioDecoderListener, IAIMPAudioDecoderNotifications,
    IAIMPErrorInfo, IAIMPExtensionAudioDecoder, IAIMPExtensionAudioDecoderPriority, IAIMPFileInfo,
    IAIMPServiceAudioDecoders, IAIMPStream, IUnknown, IID,
};
use std::cell::Cell;
use std::mem::MaybeUninit;
use std::{io, mem, slice};
use winapi::_core::ffi::c_void;
use winapi::shared::minwindef::{BOOL, FALSE, TRUE};
use winapi::shared::winerror::{E_FAIL, E_PENDING, HRESULT, S_OK};

pub(crate) static AUDIO_DECODERS: Service<AudioDecoders> = Service::new();

pub trait AudioDecoderBuilder {
    const PRIORITY: Option<i32>;
    const ONLY_INSTANCE: bool;

    type Decoder: AudioDecoder;
    type Error: std::error::Error;

    fn create(&self, stream: Stream) -> Result<Self::Decoder, Self::Error>;
}

pub trait AudioDecoder {
    fn file_info(&self) -> Option<FileInfo>;

    fn stream_info(&self) -> Option<StreamInfo>;

    fn is_seekable(&self) -> bool;

    fn is_realtime_stream(&self) -> bool;

    fn available_data(&self) -> i64 {
        self.size() - self.pos()
    }

    fn size(&self) -> i64;

    fn pos(&self) -> i64;

    fn set_pos(&self, pos: i64) -> bool;

    fn read(&self, buf: &mut [u8]) -> i32;

    fn buffering_progress(&self) -> Option<BufferingProgress>;

    fn notifications<'a>(&self) -> Option<&'a AudioDecoderNotificationsWrapper>;
}

impl io::Read for dyn AudioDecoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(AudioDecoder::read(self, buf) as usize)
    }
}

struct AudioDecoderWrapper<T>(T);

impl<T: AudioDecoder> IAIMPAudioDecoder for AudioDecoderWrapper<T> {
    unsafe fn get_file_info(&self, file_info: ComPtr<dyn IAIMPFileInfo>) -> BOOL {
        if let Some(info) = self.0.file_info() {
            let mut file_info = FileInfo::from(ComRc::from(file_info));
            file_info.clone_from(&info);
            mem::forget(file_info);
            TRUE
        } else {
            FALSE
        }
    }

    unsafe fn get_stream_info(
        &self,
        sample_rate: *mut i32,
        channels: *mut i32,
        sample_format: *mut SampleFormat,
    ) -> BOOL {
        if let Some(info) = self.0.stream_info() {
            *sample_rate = info.sample_rate;
            *channels = info.channels;
            *sample_format = info.sample_format;
            TRUE
        } else {
            FALSE
        }
    }

    unsafe fn is_seekable(&self) -> BOOL {
        self.0.is_seekable() as BOOL
    }

    unsafe fn is_realtime_stream(&self) -> BOOL {
        self.0.is_realtime_stream() as BOOL
    }

    unsafe fn get_available_data(&self) -> i64 {
        self.0.available_data()
    }

    unsafe fn get_size(&self) -> i64 {
        self.0.size()
    }

    unsafe fn get_position(&self) -> i64 {
        self.0.pos()
    }

    unsafe fn set_position(&self, position: i64) -> i32 {
        self.0.set_pos(position) as BOOL
    }

    unsafe fn read(&self, buffer: *mut c_void, count: i32) -> i32 {
        self.0
            .read(slice::from_raw_parts_mut(buffer as *mut _, count as usize))
    }
}

impl<T: AudioDecoder> IAIMPAudioDecoderBufferingProgress for AudioDecoderWrapper<T> {
    unsafe fn get(&self, value: *mut BufferingProgress) -> i32 {
        if let Some(progress) = self.0.buffering_progress() {
            *value = progress;
            TRUE
        } else {
            FALSE
        }
    }
}

impl<T: AudioDecoder> IAIMPAudioDecoderNotifications for AudioDecoderWrapper<T> {
    unsafe fn listener_add(&self, listener: ComRc<dyn IAIMPAudioDecoderListener>) {
        if let Some(notifications) = self.0.notifications() {
            notifications.0.add_listener(AudioDecoderListener(listener))
        }
    }

    unsafe fn listener_remove(&self, listener: ComRc<dyn IAIMPAudioDecoderListener>) {
        if let Some(notifications) = self.0.notifications() {
            notifications
                .0
                .remove_listener(AudioDecoderListener(listener))
        }
    }
}

impl<T: AudioDecoder> ComInterfaceQuerier for AudioDecoderWrapper<T> {
    fn query_interface(&self, riid: &IID) -> bool {
        if riid == &<dyn IAIMPAudioDecoderBufferingProgress>::IID {
            self.0.buffering_progress().is_some()
        } else if riid == &<dyn IAIMPAudioDecoderNotifications>::IID {
            self.0.notifications().is_some()
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct StreamInfo {
    pub sample_rate: i32,
    pub channels: i32,
    pub sample_format: SampleFormat,
}

pub struct AudioDecoderNotificationsWrapper(Box<dyn AudioDecoderNotifications>);

impl AudioDecoderNotificationsWrapper {
    pub fn new<T: AudioDecoderNotifications + 'static>(notifications: T) -> Self {
        Self(Box::new(notifications))
    }
}

pub trait AudioDecoderNotifications {
    fn add_listener(&self, listener: AudioDecoderListener);

    fn remove_listener(&self, listener: AudioDecoderListener);
}

pub struct AudioDecoderListener(ComRc<dyn IAIMPAudioDecoderListener>);

impl AudioDecoderListener {
    pub fn changed(&self, changes: DecoderChange) {
        unsafe { self.0.changed(changes) }
    }
}

pub struct AudioDecoderBuilderWrapper<T> {
    inner: T,
    once_inited: Cell<bool>,
}

impl<T> AudioDecoderBuilderWrapper<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            once_inited: Cell::new(false),
        }
    }
}

impl<T: AudioDecoderBuilder> IAIMPExtensionAudioDecoder for AudioDecoderBuilderWrapper<T> {
    unsafe fn create_decoder(
        &self,
        stream: ComRc<dyn IAIMPStream>,
        flags: DecoderFlags,
        error_info: ComPtr<dyn IAIMPErrorInfo>,
        decoder: *mut ComRc<dyn IAIMPAudioDecoder>,
    ) -> HRESULT {
        let once_inited = self.once_inited.get();
        if once_inited && !flags.contains(DecoderFlags::FORCE_CREATE_INSTANCE) && T::ONLY_INSTANCE {
            return E_PENDING;
        }

        stream.add_ref();
        let stream = Stream(stream);
        let res = self.inner.create(stream);
        match res {
            Ok(tdecoder) => {
                self.once_inited.set(true);

                let wrapper = com_wrapper!(
                    AudioDecoderWrapper(tdecoder) =>
                    dyn IAIMPAudioDecoder,
                    dyn IAIMPAudioDecoderBufferingProgress,
                    dyn IAIMPAudioDecoderNotifications
                )
                .into_com_rc();
                decoder.write(wrapper);

                S_OK
            }
            Err(err) => {
                error_info.add_ref();
                let mut error_info = ErrorInfo(ComRc::from(error_info));
                error_info.set(ErrorInfoContent {
                    code: 1,
                    msg: AimpString::from(err.to_string()),
                    details: None,
                });
                E_FAIL
            }
        }
    }
}

impl<T: AudioDecoderBuilder> IAIMPExtensionAudioDecoderPriority for AudioDecoderBuilderWrapper<T> {
    unsafe fn get_priority(&self) -> i32 {
        T::PRIORITY.unwrap_or(0)
    }
}

impl<T> Extension for AudioDecoderBuilderWrapper<T> {
    const SERVICE_IID: IID = <dyn IAIMPServiceAudioDecoders>::IID;
}

impl<T: AudioDecoderBuilder> From<AudioDecoderBuilderWrapper<T>>
    for ComRc<dyn IAIMPExtensionAudioDecoder>
{
    fn from(wrapper: AudioDecoderBuilderWrapper<T>) -> Self {
        let wrapper = com_wrapper!(
            wrapper =>
            dyn IAIMPExtensionAudioDecoder,
            dyn IAIMPExtensionAudioDecoderPriority
        );
        unsafe { wrapper.into_com_rc() }
    }
}

impl<T: AudioDecoderBuilder> ComInterfaceQuerier for AudioDecoderBuilderWrapper<T> {
    fn query_interface(&self, riid: &IID) -> bool {
        if riid == &<dyn IAIMPExtensionAudioDecoderPriority>::IID {
            T::PRIORITY.is_some()
        } else {
            true
        }
    }
}

pub(crate) struct AudioDecoders(ComPtr<dyn IAIMPServiceAudioDecoders>);

impl AudioDecoders {
    fn create_decoder_for_stream(&self, stream: Stream) -> Result<AimpAudioDecoder> {
        unsafe {
            let mut decoder = MaybeUninit::uninit();
            let error_info = ErrorInfo::default();
            self.0
                .create_decoder_for_stream(
                    stream.0,
                    0,
                    Some(error_info.0.as_raw()),
                    decoder.as_mut_ptr(),
                )
                .with_error_info(error_info)?;
            Ok(AimpAudioDecoder(decoder.assume_init()))
        }
    }

    fn create_decoder_for_file_uri(&self, file_uri: &FileUri) -> Result<AimpAudioDecoder> {
        unsafe {
            let mut decoder = MaybeUninit::uninit();
            let error_info = ErrorInfo::default();
            self.0
                .create_decoder_for_file_uri(
                    (file_uri.0).0.as_raw(),
                    0,
                    Some(error_info.0.as_raw()),
                    decoder.as_mut_ptr(),
                )
                .with_error_info(error_info)?;
            Ok(AimpAudioDecoder(decoder.assume_init()))
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceAudioDecoders>> for AudioDecoders {
    fn from(ptr: ComPtr<dyn IAIMPServiceAudioDecoders>) -> Self {
        Self(ptr)
    }
}

pub struct AimpAudioDecoder(ComRc<dyn IAIMPAudioDecoder>);

impl AimpAudioDecoder {
    pub fn from_stream<T: Into<Stream>>(stream: T) -> Result<Self> {
        AUDIO_DECODERS
            .get()
            .create_decoder_for_stream(stream.into())
    }

    pub fn from_file_uri(file_uri: &FileUri) -> Result<Self> {
        AUDIO_DECODERS.get().create_decoder_for_file_uri(file_uri)
    }
}

impl AudioDecoder for AimpAudioDecoder {
    fn file_info(&self) -> Option<FileInfo> {
        let info = FileInfo::default();
        unsafe {
            if self.0.get_file_info(info.prop_list.0.as_raw()) == TRUE {
                Some(info)
            } else {
                None
            }
        }
    }

    fn stream_info(&self) -> Option<StreamInfo> {
        let mut sample_rate = MaybeUninit::uninit();
        let mut channels = MaybeUninit::uninit();
        let mut sample_format = MaybeUninit::uninit();
        unsafe {
            if self.0.get_stream_info(
                sample_rate.as_mut_ptr(),
                channels.as_mut_ptr(),
                sample_format.as_mut_ptr(),
            ) == TRUE
            {
                Some(StreamInfo {
                    sample_rate: sample_rate.assume_init(),
                    channels: channels.assume_init(),
                    sample_format: sample_format.assume_init(),
                })
            } else {
                None
            }
        }
    }

    fn is_seekable(&self) -> bool {
        unsafe { self.0.is_seekable() == TRUE }
    }

    fn is_realtime_stream(&self) -> bool {
        unsafe { self.0.is_realtime_stream() == TRUE }
    }

    fn available_data(&self) -> i64 {
        unsafe { self.0.get_available_data() }
    }

    fn size(&self) -> i64 {
        unsafe { self.0.get_size() }
    }

    fn pos(&self) -> i64 {
        unsafe { self.0.get_position() }
    }

    fn set_pos(&self, pos: i64) -> bool {
        unsafe { self.0.set_position(pos) == TRUE }
    }

    fn read(&self, buf: &mut [u8]) -> i32 {
        unsafe { self.0.read(buf.as_mut_ptr() as *mut _, buf.len() as i32) }
    }

    fn buffering_progress(&self) -> Option<BufferingProgress> {
        None
    }

    fn notifications<'a>(&self) -> Option<&'a AudioDecoderNotificationsWrapper> {
        None
    }
}
