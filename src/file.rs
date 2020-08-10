pub use iaimp::FileFormatsCategory;
pub use winapi::shared::windef::RECT as Rect;

use crate::{
    actions::{ActionEvent, ActionEventObj},
    core::Extension,
    error::HresultExt,
    impl_prop_accessor, prop_list,
    prop_list::{HashedPropertyList, PropertyList},
    stream::Stream,
    util::Service,
    AimpString, Error, List, ProgressCallback, Result, CORE,
};
use iaimp::{
    com_wrapper, ComInterface, ComInterfaceQuerier, ComPtr, ComRc, FileInfoFlags, FileInfoProp,
    FileStreamingFlags, FileSystemProp, FileUriFlags, IAIMPActionEvent, IAIMPExtensionFileExpander,
    IAIMPExtensionFileFormat, IAIMPExtensionFileInfoProvider, IAIMPExtensionFileInfoProviderEx,
    IAIMPExtensionFileSystem, IAIMPFileInfo, IAIMPFileStream,
    IAIMPFileSystemCommandCopyToClipboard, IAIMPFileSystemCommandDelete,
    IAIMPFileSystemCommandDropSource, IAIMPFileSystemCommandFileInfo,
    IAIMPFileSystemCommandOpenFileFolder, IAIMPFileSystemCommandStreaming,
    IAIMPFileSystemCustomFileCommand, IAIMPImage, IAIMPImageContainer, IAIMPObjectList,
    IAIMPProgressCallback, IAIMPPropertyList, IAIMPServiceFileFormats, IAIMPServiceFileInfo,
    IAIMPServiceFileInfoFormatter, IAIMPServiceFileInfoFormatterUtils, IAIMPServiceFileManager,
    IAIMPServiceFileStreaming, IAIMPServiceFileSystems, IAIMPServiceFileURI, IAIMPServiceFileURI2,
    IAIMPStream, IAIMPString, IAIMPVirtualFile, IUnknown, TAIMPFileAttributes, TDateTime,
    VirtualFileProp, HRESULT, IID,
};
use std::{
    fmt,
    mem::MaybeUninit,
    ops::{Deref, DerefMut, Range},
    time::SystemTime,
};
use winapi::shared::minwindef::BOOL;
use winapi::shared::winerror::E_UNEXPECTED;
use winapi::shared::{
    minwindef::TRUE,
    winerror::{E_FAIL, E_NOTIMPL, HRESULT as WinHRESULT, S_OK},
};

pub static FILE_FORMATS: Service<FileFormats> = Service::new();
pub(crate) static FILE_INFO_SERVICE: Service<FileInfoService> = Service::new();
pub static FILE_INFO_FORMATTER: Service<FileInfoFormatter> = Service::new();
pub(crate) static FILE_INFO_FORMATTER_UTILS: Service<FileInfoFormatterUtils> = Service::new();
pub(crate) static FILE_STREAMING: Service<FileStreamingService> = Service::new();
pub(crate) static FILE_URI_SERVICE: Service<FileUriService> = Service::new();
pub static FILE_SYSTEMS: Service<FileSystems> = Service::new();

prop_list! {
    list: FileInfo(ComRc<dyn IAIMPFileInfo>),
    prop: FileInfoProp,
    guard: FileInfoGuard,
    methods:
    custom(Custom) -> Option<ComRc<dyn IUnknown>>,
    album(Album) -> AimpString,
    album_art_img(AlbumArt) -> Option<ComRc<dyn IAIMPImage>>, // TODO: image wrapper
    album_art_img_container(AlbumArt) -> Option<ComRc<dyn IAIMPImageContainer>>,
    album_gain(AlbumGain) -> f64,
    album_peak(AlbumPeak) -> f64,
    artist(Artist) -> AimpString,
    bit_rate(BitRate) -> i32,
    bit_depth(BitDepth) -> i32,
    bpm(Bpm) -> i32,
    channels(Channels) -> i32,
    codec(Codec) -> AimpString,
    comment(Comment) -> AimpString,
    composer(Composer) -> AimpString,
    copyright(Copyright) -> AimpString,
    cue_sheet(CueSheet) -> AimpString,
    date(Date) -> AimpString,
    disk_number(DiskNumber) -> AimpString,
    disk_total(DiskTotal) -> AimpString,
    duration(Duration) -> f64,
    file_name(Filename) -> AimpString,
    file_size(FileSize) -> i64,
    genre(Genre) -> AimpString,
    lyrics(Lyrics) -> AimpString,
    publisher(Publisher) -> AimpString,
    sample_rate(SampleRate) -> i32,
    title(Title) -> AimpString,
    track_gain(TrackGain) -> f64,
    track_number(TrackNumber) -> AimpString,
    track_peak(TrackPeak) -> f64,
    track_total(TrackTotal) -> AimpString,
    url(Url) -> AimpString,
    conductor(Conductor) -> AimpString,
    mood(Mood) -> AimpString,
    catalog(Catalog) -> AimpString,
    isrc(Isrc) -> AimpString,
    lyricist(Lyricist) -> AimpString,
    encode_by(EncodeBy) -> AimpString,
    rating(Rating) -> FileInfoRating,
    stat_adding_date(StatAddingDate) -> Option<TDateTime>,
    stat_last_play_date(StatLastPlayDate) -> Option<TDateTime>,
    stat_mark(StatMark) -> Option<FileInfoRating>,
    stat_play_count(StatPlayCount) -> Option<i32>,
    stat_rating(StatRating) -> Option<FileInfoMark>,
    stat_displaying_mark(StatDisplayingMark) -> FileInfoMark,
}

impl FileInfo {
    pub fn from_file_uri<T: Into<FileUri>>(file_uri: T) -> Result<Self> {
        FILE_INFO_SERVICE.get().file_info_from_url(file_uri.into())
    }

    pub fn from_stream<T: ComInterface + IAIMPStream + ?Sized>(stream: Stream<T>) -> Result<Self> {
        let this = FileInfo::default();
        FILE_INFO_SERVICE
            .get()
            .file_info_from_stream(stream, &this)?;
        Ok(this)
    }

    pub fn clone_from(&mut self, other: &FileInfo) {
        unsafe {
            (self.prop_list)
                .0
                .assign((other.prop_list).0.as_raw())
                .into_result()
                .unwrap();
        }
    }
}

impl From<ComRc<dyn IAIMPFileInfo>> for FileInfo {
    fn from(rc: ComRc<dyn IAIMPFileInfo>) -> Self {
        Self {
            prop_list: PropertyList::from(rc),
        }
    }
}

impl Clone for FileInfo {
    fn clone(&self) -> Self {
        unsafe {
            let mut info = MaybeUninit::uninit();
            IAIMPFileInfo::clone(&(self.prop_list).0, info.as_mut_ptr())
                .into_result()
                .unwrap();
            Self::from(info.assume_init())
        }
    }
}

impl Default for FileInfo {
    fn default() -> Self {
        Self {
            prop_list: PropertyList::from(CORE.get().create::<dyn IAIMPFileInfo>().unwrap()),
        }
    }
}

impl_prop_accessor!(FileInfoRating);

#[derive(Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub struct FileInfoRating(i32);

impl FileInfoRating {
    pub fn new(rating: i32) -> Option<Self> {
        if rating >= 0 && rating <= 5 {
            Some(Self(rating))
        } else {
            None
        }
    }
}

impl fmt::Debug for FileInfoRating {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl Deref for FileInfoRating {
    type Target = i32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(PartialOrd, PartialEq, Copy, Clone)]
pub struct FileInfoMark(f64);

impl FileInfoMark {
    pub fn new(mark: f64) -> Option<Self> {
        if mark >= 0.0 && mark <= 5.0 {
            Some(Self(mark))
        } else {
            None
        }
    }
}

impl fmt::Debug for FileInfoMark {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl Deref for FileInfoMark {
    type Target = f64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl_prop_accessor!(FileInfoMark);

pub trait CustomVirtualFile {
    type Error;

    fn create_stream(&self) -> Result<Option<Stream>, Self::Error>;

    fn file_info(&self) -> Option<FileInfo>;

    fn is_exists(&self) -> bool;

    fn is_in_same_stream(&self, virtual_file: &VirtualFile) -> Result<(), Self::Error>;

    fn sync(&self) -> Result<(), Self::Error>;
}

struct CustomVirtualFileWrapper<T> {
    inner: T,
    hashed: HashedPropertyList,
}

impl<T> IAIMPPropertyList for CustomVirtualFileWrapper<T> {
    unsafe fn begin_update(&self) {
        self.hashed.begin_update()
    }

    unsafe fn end_update(&self) {
        self.hashed.end_update()
    }

    unsafe fn reset(&self) -> HRESULT {
        self.hashed.reset()
    }

    unsafe fn get_value_as_float(&self, property_id: i32, value: *mut f64) -> HRESULT {
        self.hashed.get_value_as_float(property_id, value)
    }

    unsafe fn get_value_as_int32(&self, property_id: i32, value: *mut i32) -> HRESULT {
        self.hashed.get_value_as_int32(property_id, value)
    }

    unsafe fn get_value_as_int64(&self, property_id: i32, value: *mut i64) -> HRESULT {
        self.hashed.get_value_as_int64(property_id, value)
    }

    unsafe fn get_value_as_object(
        &self,
        property_id: i32,
        iid: *const IID,
        value: *mut ComRc<dyn IUnknown>,
    ) -> HRESULT {
        self.hashed.get_value_as_object(property_id, iid, value)
    }

    unsafe fn set_value_as_float(&self, property_id: i32, value: f64) -> HRESULT {
        self.hashed.set_value_as_float(property_id, value)
    }

    unsafe fn set_value_as_int32(&self, property_id: i32, value: i32) -> HRESULT {
        self.hashed.set_value_as_int32(property_id, value)
    }

    unsafe fn set_value_as_int64(&self, property_id: i32, value: i64) -> HRESULT {
        self.hashed.set_value_as_int64(property_id, value)
    }

    unsafe fn set_value_as_object(&self, property_id: i32, value: ComRc<dyn IUnknown>) -> HRESULT {
        self.hashed.set_value_as_object(property_id, value)
    }
}

impl<T: CustomVirtualFile> IAIMPVirtualFile for CustomVirtualFileWrapper<T> {
    unsafe fn create_stream(&self, stream: *mut ComRc<dyn IAIMPStream>) -> HRESULT {
        let res = match self.inner.create_stream() {
            Ok(Some(tstream)) => {
                stream.write(tstream.0);
                S_OK
            }
            Ok(None) => E_NOTIMPL,
            Err(_) => E_UNEXPECTED,
        };
        HRESULT(res)
    }

    unsafe fn get_file_info(&self, info: ComPtr<dyn IAIMPFileInfo>) -> HRESULT {
        if let Some(i) = self.inner.file_info() {
            let rc = ComRc::from(info);
            rc.add_ref();
            let mut info = FileInfo::from(rc);
            info.clone_from(&i);
            HRESULT(S_OK)
        } else {
            HRESULT(E_FAIL)
        }
    }

    unsafe fn is_exists(&self) -> BOOL {
        self.inner.is_exists() as BOOL
    }

    unsafe fn is_in_same_stream(&self, virtual_file: ComPtr<dyn IAIMPVirtualFile>) -> HRESULT {
        let virtual_file = ComRc::from(virtual_file);
        virtual_file.add_ref();
        let virtual_file = VirtualFile::from_com_rc(virtual_file);
        if let Ok(()) = self.inner.is_in_same_stream(&virtual_file) {
            HRESULT(S_OK)
        } else {
            HRESULT(E_FAIL)
        }
    }

    unsafe fn synchronize(&self) -> HRESULT {
        if let Ok(()) = self.inner.sync() {
            HRESULT(S_OK)
        } else {
            HRESULT(E_FAIL)
        }
    }
}

impl<T> ComInterfaceQuerier for CustomVirtualFileWrapper<T> {}

prop_list! {
    list: VirtualFile(ComRc<dyn IAIMPVirtualFile>),
    prop: VirtualFileProp,
    guard: VirtualFileGuard,
    methods:
    index_in_set(IndexInSet) -> i32,
    clip_start(ClipStart) -> Option<f64>,
    clip_finish(ClipFinish) -> Option<f64>,
    audio_source_file(AudioSourceFile) -> Option<AimpString>,
    file_format(FileFormat) -> Option<AimpString>,
    file_uri(FileUri) -> FileUri,
}

impl VirtualFile {
    pub(crate) fn from_com_rc(rc: ComRc<dyn IAIMPVirtualFile>) -> Self {
        Self {
            prop_list: PropertyList::from(rc),
        }
    }

    pub fn from_custom<T: CustomVirtualFile>(custom: T) -> Self {
        let wrapper = CustomVirtualFileWrapper {
            inner: custom,
            hashed: HashedPropertyList::default(),
        };
        let wrapper = unsafe { com_wrapper!(wrapper => dyn IAIMPVirtualFile).into_com_rc() };
        Self::from_com_rc(wrapper)
    }

    pub fn from_file_uri<T: Into<FileUri>>(file_uri: T) -> Option<Self> {
        FILE_INFO_SERVICE.get().virtual_file(file_uri.into())
    }

    pub fn create_stream(&self) -> Result<Stream<dyn IAIMPStream>> {
        unsafe {
            let mut stream = MaybeUninit::uninit();
            (self.prop_list)
                .0
                .create_stream(stream.as_mut_ptr())
                .into_result()
                .unwrap();
            Ok(Stream(stream.assume_init()))
        }
    }

    pub fn file_info(&self) -> FileInfo {
        unsafe {
            let file_info = FileInfo::default();
            (self.prop_list)
                .0
                .get_file_info((file_info.prop_list).0.as_raw())
                .into_result()
                .unwrap();
            file_info
        }
    }

    pub fn exists(&self) -> bool {
        unsafe { (self.prop_list).0.is_exists() == TRUE }
    }

    pub fn in_same_stream(&self, file: &VirtualFile) -> bool {
        unsafe {
            (self.prop_list)
                .0
                .is_in_same_stream((file.prop_list).0.as_raw())
                == S_OK
        }
    }

    pub fn sync(&self) -> Result<()> {
        unsafe { (self.prop_list).0.synchronize().into_result() }
    }
}

pub struct FileFormats(ComPtr<dyn IAIMPServiceFileFormats>);

impl FileFormats {
    pub fn formats(&self, categories: FileFormatsCategory) -> Vec<String> {
        unsafe {
            let mut s = MaybeUninit::uninit();
            self.0
                .get_formats(categories, s.as_mut_ptr())
                .into_result()
                .unwrap();
            let s = s.assume_init();
            AimpString(s)
                .to_string()
                .split_terminator(';')
                .map(str::to_string)
                .collect()
        }
    }

    pub fn is_supported(&self, file_name: AimpString) -> bool {
        unsafe {
            self.0.is_supported(
                file_name.0,
                FileFormatsCategory::AUDIO | FileFormatsCategory::PLAYLISTS,
            ) == S_OK
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileFormats>> for FileFormats {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileFormats>) -> Self {
        Self(ptr)
    }
}

pub struct FileInfoService(ComPtr<dyn IAIMPServiceFileInfo>);

impl FileInfoService {
    fn file_info_from_url(&self, file_uri: FileUri) -> Result<FileInfo> {
        unsafe {
            let info = FileInfo::default();
            self.0
                .get_file_info_from_file_uri(
                    (file_uri.0).0,
                    FileInfoFlags::NONE,
                    (info.prop_list).0.as_raw(),
                )
                .into_result()?;
            Ok(info)
        }
    }

    fn file_info_from_stream<T: ComInterface + IAIMPStream + ?Sized>(
        &self,
        stream: Stream<T>,
        file_info: &FileInfo,
    ) -> Result<()> {
        unsafe {
            self.0
                .get_file_info_from_stream(
                    stream.0.cast(),
                    FileInfoFlags::NONE,
                    (file_info.prop_list).0.as_raw(),
                )
                .into_result()
        }
    }

    fn virtual_file(&self, file_uri: FileUri) -> Option<VirtualFile> {
        unsafe {
            let mut file = MaybeUninit::uninit();
            if self
                .0
                .get_virtual_file((file_uri.0).0, 0, file.as_mut_ptr())
                == S_OK
            {
                Some(VirtualFile::from_com_rc(file.assume_init()))
            } else {
                None
            }
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileInfo>> for FileInfoService {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileInfo>) -> Self {
        Self(ptr)
    }
}

pub struct FileInfoFormatter(ComPtr<dyn IAIMPServiceFileInfoFormatter>);

impl FileInfoFormatter {
    fn inner_format(&self, template: AimpString, info: Option<FileInfo>) -> AimpString {
        unsafe {
            let mut formatted = MaybeUninit::uninit();
            self.0
                .format(
                    template.0,
                    info.map(|info| (info.prop_list).0),
                    0,
                    None,
                    formatted.as_mut_ptr(),
                )
                .into_result()
                .unwrap();
            AimpString(formatted.assume_init())
        }
    }

    pub fn preview<T: Into<AimpString>>(&self, template: T) -> AimpString {
        self.inner_format(template.into(), None)
    }

    pub fn format<T: Into<AimpString>>(&self, template: T, info: FileInfo) -> AimpString {
        self.inner_format(template.into(), Some(info))
    }

    pub fn show_macros_legend<T>(&self, screen_target: Rect, handler: T)
    where
        T: ActionEvent<Data = AimpString> + 'static,
    {
        FILE_INFO_FORMATTER_UTILS
            .get()
            .show_macros_legend(screen_target, ActionEventObj::new(handler))
    }
}

impl From<ComPtr<dyn IAIMPServiceFileInfoFormatter>> for FileInfoFormatter {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileInfoFormatter>) -> Self {
        Self(ptr)
    }
}

pub(crate) struct FileInfoFormatterUtils(ComPtr<dyn IAIMPServiceFileInfoFormatterUtils>);

impl FileInfoFormatterUtils {
    pub fn show_macros_legend(&self, screen_target: Rect, handler: ActionEventObj) {
        unsafe {
            let wrapper = com_wrapper!(handler => dyn IAIMPActionEvent);
            self.0
                .show_macros_legend(screen_target, 0, wrapper.into_com_rc())
                .into_result()
                .unwrap();
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileInfoFormatterUtils>> for FileInfoFormatterUtils {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileInfoFormatterUtils>) -> Self {
        Self(ptr)
    }
}

pub struct FileStreamingService(ComPtr<dyn IAIMPServiceFileStreaming>);

impl FileStreamingService {
    pub fn create_stream_for_file(
        &self,
        file_name: AimpString,
        clipping: Option<FileClipping>,
        flags: FileStreamingFlags,
    ) -> Result<FileStream> {
        unsafe {
            let mut stream = MaybeUninit::uninit();
            let clipping = clipping.unwrap_or_else(|| FileClipping {
                offset: -1,
                size: -1,
            });
            self.0
                .create_stream_for_file(
                    file_name.0,
                    flags,
                    clipping.offset,
                    clipping.size,
                    stream.as_mut_ptr(),
                )
                .into_result()?;
            Ok(FileStream(Stream(stream.assume_init())))
        }
    }

    pub fn create_stream_for_file_uri(
        &self,
        file_uri: &FileUri,
    ) -> Result<(Option<VirtualFile>, FileStream)> {
        unsafe {
            let mut virtual_file = MaybeUninit::uninit();
            let mut stream = MaybeUninit::uninit();
            self.0
                .create_stream_for_file_uri(
                    (file_uri.0).0.as_raw(),
                    virtual_file.as_mut_ptr(),
                    stream.as_mut_ptr(),
                )
                .into_result()?;
            Ok((
                virtual_file.assume_init().map(VirtualFile::from_com_rc),
                FileStream(Stream(stream.assume_init())),
            ))
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileStreaming>> for FileStreamingService {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileStreaming>) -> Self {
        Self(ptr)
    }
}

#[derive(Debug)]
pub struct FileStream(pub(crate) Stream<dyn IAIMPFileStream>);

impl FileStream {
    pub fn open<T: Into<AimpString>>(file_name: T) -> Result<Self> {
        Self::options().open(file_name)
    }

    pub fn options() -> FileStreamingOptions {
        FileStreamingOptions::default()
    }

    pub fn from_file_uri(file_uri: &FileUri) -> Result<(Option<VirtualFile>, Self)> {
        FILE_STREAMING.get().create_stream_for_file_uri(file_uri)
    }

    pub fn clipping(&self) -> Option<FileClipping> {
        unsafe {
            let mut offset = MaybeUninit::uninit();
            let mut size = MaybeUninit::uninit();
            let res = self
                .as_inner()
                .get_clipping(offset.as_mut_ptr(), size.as_mut_ptr());
            if res == E_FAIL {
                None
            } else {
                res.into_result().unwrap();
                let offset = offset.assume_init();
                let size = size.assume_init();
                Some(FileClipping { offset, size })
            }
        }
    }

    pub fn file_name(&self) -> AimpString {
        unsafe {
            let mut s = MaybeUninit::uninit();
            self.as_inner()
                .get_file_name(s.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString::from(s.assume_init())
        }
    }
}

impl From<FileStream> for Stream {
    fn from(file_stream: FileStream) -> Self {
        unsafe { Stream((file_stream.0).0.cast()) }
    }
}

impl Deref for FileStream {
    type Target = Stream<dyn IAIMPFileStream>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FileStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct FileStreamingOptions {
    clipping: Option<FileClipping>,
    flags: Option<FileStreamingFlags>,
}

impl FileStreamingOptions {
    pub fn with_clipping<T: Into<FileClipping>>(mut self, clipping: T) -> Self {
        self.clipping = Some(clipping.into());
        self
    }

    fn update_flags(&mut self, flags: FileStreamingFlags, cond: bool) {
        if let Some(inner_flags) = self.flags.as_mut() {
            inner_flags.set(flags, cond);
        } else {
            self.flags = Some(flags).filter(|_| cond);
        }
    }

    pub fn create_new(mut self, cond: bool) -> Self {
        self.update_flags(FileStreamingFlags::CREATE_NEW, cond);
        self
    }

    pub fn read(mut self, cond: bool) -> Self {
        self.update_flags(FileStreamingFlags::READ, cond);
        self
    }

    pub fn read_write(mut self, cond: bool) -> Self {
        self.update_flags(FileStreamingFlags::READ_WRITE, cond);
        self
    }

    pub fn map_to_memory(mut self, cond: bool) -> Self {
        self.update_flags(FileStreamingFlags::MAP_TO_MEMORY, cond);
        self
    }

    pub fn open<T: Into<AimpString>>(self, file_name: T) -> Result<FileStream> {
        FILE_STREAMING.get().create_stream_for_file(
            file_name.into(),
            self.clipping,
            self.flags.unwrap_or(FileStreamingFlags::READ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileClipping {
    pub offset: i64,
    pub size: i64,
}

impl From<Range<i64>> for FileClipping {
    fn from(range: Range<i64>) -> Self {
        Self {
            offset: range.start,
            size: range.end - range.start,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FileUri(pub(crate) AimpString);

impl FileUri {
    pub fn new<T: Into<AimpString>>(uri: T) -> Option<Self> {
        let uri = uri.into();
        if FILE_URI_SERVICE.get().is_url(&uri) {
            Some(Self(uri))
        } else {
            None
        }
    }

    pub fn build<T: Into<AimpString>, U: Into<AimpString>>(container: T, part: U) -> Result<Self> {
        FILE_URI_SERVICE
            .get()
            .build(container.into(), part.into())
            .map(Self)
    }

    pub fn parse(self) -> (AimpString, Option<AimpString>) {
        FILE_URI_SERVICE.get().parse(self.0)
    }

    pub fn set_ext<T: Into<AimpString>>(&mut self, ext: T) {
        FILE_URI_SERVICE
            .get()
            .change_file_ext(&mut self.0, ext.into())
    }

    pub fn ext(&self) -> AimpString {
        FILE_URI_SERVICE.get().extract_file_ext(&self.0)
    }

    pub fn name(&self) -> AimpString {
        FILE_URI_SERVICE.get().extract_file_name(&self.0)
    }

    pub fn parent_dir(&self) -> AimpString {
        FILE_URI_SERVICE.get().extract_file_parent_dir_name(&self.0)
    }

    pub fn parent_name(&self) -> AimpString {
        FILE_URI_SERVICE.get().extract_file_parent_name(&self.0)
    }

    pub fn path(&self) -> AimpString {
        FILE_URI_SERVICE.get().extract_file_path(&self.0)
    }

    pub fn scheme(&self) -> AimpString {
        FILE_URI_SERVICE.get().get_scheme(&self.0)
    }

    pub fn into_inner(self) -> AimpString {
        self.0
    }
}

impl fmt::Debug for FileUri {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for FileUri {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl_prop_accessor!(FileUri);

pub(crate) struct FileUriService(ComPtr<dyn IAIMPServiceFileURI2>);

impl FileUriService {
    fn build(&self, container: AimpString, part: AimpString) -> Result<AimpString> {
        unsafe {
            let mut file_uri = MaybeUninit::uninit();
            self.0
                .build(container.0, part.0, file_uri.as_mut_ptr())
                .into_result()?;
            Ok(AimpString(file_uri.assume_init()))
        }
    }

    fn parse(&self, file_uri: AimpString) -> (AimpString, Option<AimpString>) {
        unsafe {
            let mut container = MaybeUninit::uninit();
            let mut part = MaybeUninit::uninit();
            self.0
                .parse(file_uri.0, container.as_mut_ptr(), part.as_mut_ptr())
                .into_result()
                .unwrap();
            (
                AimpString(container.assume_init()),
                part.assume_init().map(AimpString),
            )
        }
    }

    fn change_file_ext(&self, file_uri: &mut AimpString, new_ext: AimpString) {
        unsafe {
            let mut file_uri = MaybeUninit::new(file_uri.0.as_raw());
            self.0
                .change_file_ext(
                    file_uri.as_mut_ptr(),
                    new_ext.0,
                    FileUriFlags::DOUBLE_EXTS | FileUriFlags::PART_EXT,
                )
                .into_result()
                .unwrap();
        }
    }

    fn extract_file_ext(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut ext = MaybeUninit::uninit();
            self.0
                .extract_file_ext(
                    file_uri.0.as_raw(),
                    ext.as_mut_ptr(),
                    FileUriFlags::DOUBLE_EXTS | FileUriFlags::PART_EXT,
                )
                .into_result()
                .unwrap();
            AimpString(ext.assume_init())
        }
    }

    fn extract_file_name(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut name = MaybeUninit::uninit();
            self.0
                .extract_file_name(file_uri.0.as_raw(), name.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(name.assume_init())
        }
    }

    fn extract_file_parent_dir_name(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut name = MaybeUninit::uninit();
            self.0
                .extract_file_parent_dir_name(file_uri.0.as_raw(), name.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(name.assume_init())
        }
    }

    fn extract_file_parent_name(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut name = MaybeUninit::uninit();
            self.0
                .extract_file_parent_name(file_uri.0.as_raw(), name.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(name.assume_init())
        }
    }

    fn extract_file_path(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut path = MaybeUninit::uninit();
            self.0
                .extract_file_path(file_uri.0.as_raw(), path.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(path.assume_init())
        }
    }

    fn is_url(&self, file_uri: &AimpString) -> bool {
        unsafe { self.0.is_url(file_uri.0.as_raw()) == S_OK }
    }

    fn get_scheme(&self, file_uri: &AimpString) -> AimpString {
        unsafe {
            let mut scheme = MaybeUninit::uninit();
            self.0
                .get_scheme(file_uri.0.as_raw(), scheme.as_mut_ptr())
                .into_result()
                .unwrap();
            AimpString(scheme.assume_init())
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileURI2>> for FileUriService {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileURI2>) -> Self {
        Self(ptr)
    }
}

// Commands returned by AIMP

pub trait FileSystemsCommand: Sized {
    type Interface: ComInterface + ?Sized;

    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self;
}

pub struct AimpCustomCommand(ComPtr<dyn IAIMPFileSystemCustomFileCommand>);

impl FileSystemsCommand for AimpCustomCommand {
    type Interface = dyn IAIMPFileSystemCustomFileCommand;

    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self {
        Self(ptr)
    }
}

impl CustomCommand for AimpCustomCommand {
    type Error = Error;

    fn can_process(&self, file_name: AimpString) -> Result<(), Self::Error> {
        unsafe { self.0.can_process(file_name.0).into_result() }
    }

    fn process(&self, file_name: AimpString) -> Result<(), Self::Error> {
        unsafe { self.0.process(file_name.0).into_result() }
    }
}

pub struct AimpCopyToClipboardCommand(ComPtr<dyn IAIMPFileSystemCommandCopyToClipboard>);

impl FileSystemsCommand for AimpCopyToClipboardCommand {
    type Interface = dyn IAIMPFileSystemCommandCopyToClipboard;

    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self {
        Self(ptr)
    }
}

impl CopyToClipboardCommand for AimpCopyToClipboardCommand {
    type Error = Error;

    fn copy_to_clipboard(&self, list: List<AimpString>) -> Result<(), Self::Error> {
        unsafe { self.0.copy_to_clipboard((list.inner).0).into_result() }
    }
}

pub struct AimpDropSourceCommand(ComPtr<dyn IAIMPFileSystemCommandDropSource>);

impl FileSystemsCommand for AimpDropSourceCommand {
    type Interface = dyn IAIMPFileSystemCommandDropSource;

    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self {
        Self(ptr)
    }
}

impl DropSourceCommand for AimpDropSourceCommand {
    type Error = Error;

    fn create_stream(&self, file_name: AimpString) -> Result<Stream, Self::Error> {
        unsafe {
            let mut stream = MaybeUninit::uninit();
            self.0
                .create_stream(file_name.0, stream.as_mut_ptr())
                .into_result()?;
            Ok(Stream(stream.assume_init()))
        }
    }
}

pub struct AimpFileInfoCommand(ComPtr<dyn IAIMPFileSystemCommandFileInfo>);

impl FileSystemsCommand for AimpFileInfoCommand {
    type Interface = dyn IAIMPFileSystemCommandFileInfo;

    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self {
        Self(ptr)
    }
}

impl FileInfoCommand for AimpFileInfoCommand {
    type Error = Error;

    fn file_attrs(&self, file_name: AimpString) -> Result<FileAttributes, Self::Error> {
        unsafe {
            let mut attrs = MaybeUninit::uninit();
            self.0
                .get_file_attrs(file_name.0, attrs.as_mut_ptr())
                .into_result()?;
            let attrs = attrs.assume_init();
            Ok(FileAttributes {
                created: attrs.time_creation.into(),
                last_accessed: attrs.time_last_access.into(),
                last_wrote: attrs.time_last_write.into(),
            })
        }
    }

    fn file_size(&self, file_name: AimpString) -> Result<i64, Self::Error> {
        unsafe {
            let mut size = MaybeUninit::uninit();
            self.0
                .get_file_size(file_name.0, size.as_mut_ptr())
                .into_result()?;
            Ok(size.assume_init())
        }
    }

    fn is_file_exists(&self, file_name: AimpString) -> Result<(), Self::Error> {
        unsafe { self.0.is_file_exists(file_name.0).into_result() }
    }
}

pub struct AimpStreamingCommand(ComPtr<dyn IAIMPFileSystemCommandStreaming>);

impl FileSystemsCommand for AimpStreamingCommand {
    type Interface = dyn IAIMPFileSystemCommandStreaming;
    fn from_com_ptr(ptr: ComPtr<Self::Interface>) -> Self {
        Self(ptr)
    }
}

impl StreamingCommand for AimpStreamingCommand {
    type Error = Error;

    fn create_stream(
        &self,
        file_name: AimpString,
        flags: FileStreamingFlags,
        clipping: FileClipping,
    ) -> Result<Stream, Self::Error> {
        unsafe {
            let mut stream = MaybeUninit::uninit();
            self.0
                .create_stream(
                    file_name.0,
                    flags,
                    clipping.offset,
                    clipping.size,
                    stream.as_mut_ptr(),
                )
                .into_result()?;
            Ok(Stream(stream.assume_init()))
        }
    }
}

pub struct FileSystems(ComPtr<dyn IAIMPServiceFileSystems>);

impl FileSystems {
    pub fn get<T: FileSystemsCommand>(&self, uri: &FileUri) -> Result<T> {
        unsafe {
            let mut command = MaybeUninit::uninit();
            self.0
                .get((uri.0).0.as_raw(), &T::Interface::IID, command.as_mut_ptr())
                .into_result()?;
            Ok(T::from_com_ptr(ComPtr::from_ptr(
                command.assume_init() as *mut _
            )))
        }
    }

    pub fn get_default<T: FileSystemsCommand>(&self) -> Result<T> {
        unsafe {
            let mut command = MaybeUninit::uninit();
            self.0
                .get_default(&T::Interface::IID, command.as_mut_ptr())
                .into_result()?;
            Ok(T::from_com_ptr(ComPtr::from_ptr(
                command.assume_init() as *mut _
            )))
        }
    }
}

impl From<ComPtr<dyn IAIMPServiceFileSystems>> for FileSystems {
    fn from(ptr: ComPtr<dyn IAIMPServiceFileSystems>) -> Self {
        Self(ptr)
    }
}

// User commands

prop_list! {
    list: FileSystem(HashedPropertyList),
    prop: FileSystemProp,
    guard: FileSystemGuard,
    methods:
    scheme(Scheme) -> AimpString,
    read_only(ReadOnly) -> bool,
    => fields:
    custom: Option<BoxedCustomCommand>,
    copy_to_clipboard: Option<BoxedCopyToClipboardCommand>,
    delete: bool,
    drop_source: Option<BoxedDropSourceCommand>,
    file_info: Option<BoxedFileInfoCommand>,
    open_file_folder: bool,
    streaming: Option<BoxedStreamingCommand>,
}

impl FileSystem {
    pub fn with_custom<T>(mut self, command: T) -> Self
    where
        T: CustomCommand + 'static,
    {
        self.custom = Some(Box::new(CommandWrapper(command)));
        self
    }

    pub fn with_copy_to_clipboard<T>(mut self, command: T) -> Self
    where
        T: CopyToClipboardCommand + 'static,
    {
        self.copy_to_clipboard = Some(Box::new(CommandWrapper(command)));
        self
    }

    pub fn delete(mut self, del: bool) -> Self {
        self.delete = del;
        self
    }

    pub fn with_drop_source<T>(mut self, command: T) -> Self
    where
        T: DropSourceCommand + 'static,
    {
        self.drop_source = Some(Box::new(CommandWrapper(command)));
        self
    }

    pub fn with_file_info<T>(mut self, command: T) -> Self
    where
        T: FileInfoCommand + 'static,
    {
        self.file_info = Some(Box::new(CommandWrapper(command)));
        self
    }

    pub fn open_file_folder(mut self, open: bool) -> Self {
        self.open_file_folder = open;
        self
    }

    pub fn with_streaming<T>(mut self, command: T) -> Self
    where
        T: StreamingCommand + 'static,
    {
        self.streaming = Some(Box::new(CommandWrapper(command)));
        self
    }
}

impl Default for FileSystem {
    fn default() -> Self {
        Self {
            prop_list: PropertyList::from(HashedPropertyList::default()),
            custom: None,
            copy_to_clipboard: None,
            delete: false,
            drop_source: None,
            file_info: None,
            open_file_folder: false,
            streaming: None,
        }
    }
}

impl From<FileSystem> for ComRc<dyn IAIMPExtensionFileSystem> {
    fn from(file_system: FileSystem) -> Self {
        let wrapper = com_wrapper!(file_system =>
            dyn IAIMPFileSystemCustomFileCommand,
            dyn IAIMPFileSystemCommandCopyToClipboard,
            dyn IAIMPFileSystemCommandDelete,
            dyn IAIMPFileSystemCommandDropSource,
            dyn IAIMPFileSystemCommandFileInfo,
            dyn IAIMPFileSystemCommandOpenFileFolder,
            dyn IAIMPFileSystemCommandStreaming
        );
        unsafe { wrapper.into_com_rc() }
    }
}

impl ComInterfaceQuerier for FileSystem {
    fn query_interface(&self, riid: &IID) -> bool {
        #[macro_export(local_inner_macros)]
        macro_rules! match_iid {
            (
                match riid {
                    $(
                        $interface:ident => $ty:ident $this:ident.$field:ident,
                    )+
                }
            ) => {
                $(
                    if riid == &<dyn $interface as ComInterface>::IID {
                        match_iid!($ty $this.$field)
                    } else
                )+
                {
                    true
                }
            };
            (opt $this:ident.$field:ident) => {
                $this.$field.as_ref().map_or(false, |_| true)
            };
            (bool $this:ident.$field:ident) => {
                $this.$field
            }
        }

        match_iid! {
            match riid {
                IAIMPFileSystemCustomFileCommand => opt self.custom,
                IAIMPFileSystemCommandCopyToClipboard => opt self.copy_to_clipboard,
                IAIMPFileSystemCommandDelete => bool self.delete,
                IAIMPFileSystemCommandDropSource => opt self.drop_source,
                IAIMPFileSystemCommandFileInfo => opt self.file_info,
                IAIMPFileSystemCommandOpenFileFolder => bool self.open_file_folder,
                IAIMPFileSystemCommandStreaming => opt self.streaming,
            }
        }
    }
}

macro_rules! delegate_call {
    ($this:ident.$field:ident.$( $token:tt )+) => {
        $this.$field
            .as_ref()
            .map(|command| {
                command.$( $token )+
            })
            .map_or(E_NOTIMPL, |res: Result<(), _>| res.map_or(E_FAIL, |()| S_OK))
    };
}

impl IAIMPFileSystemCustomFileCommand for FileSystem {
    unsafe fn can_process(&self, file_name: ComRc<dyn IAIMPString>) -> WinHRESULT {
        delegate_call!(self.custom.can_process(AimpString(file_name)))
    }

    unsafe fn process(&self, file_name: ComRc<dyn IAIMPString>) -> WinHRESULT {
        delegate_call!(self.custom.process(AimpString(file_name)))
    }
}

impl IAIMPFileSystemCommandCopyToClipboard for FileSystem {
    unsafe fn copy_to_clipboard(&self, files: ComRc<dyn IAIMPObjectList>) -> WinHRESULT {
        delegate_call!(self
            .copy_to_clipboard
            .copy_to_clipboard(List::from_com_rc(files)))
    }
}

impl IAIMPFileSystemCommandDelete for FileSystem {}

impl IAIMPFileSystemCommandDropSource for FileSystem {
    unsafe fn create_stream(
        &self,
        file_name: ComRc<dyn IAIMPString>,
        stream: *mut ComRc<dyn IAIMPStream>,
    ) -> WinHRESULT {
        delegate_call!(self
            .drop_source
            .create_stream(AimpString(file_name))
            .map(|s| stream.write(s.0)))
    }
}

impl IAIMPFileSystemCommandFileInfo for FileSystem {
    unsafe fn get_file_attrs(
        &self,
        file_name: ComRc<dyn IAIMPString>,
        attrs: *mut TAIMPFileAttributes,
    ) -> WinHRESULT {
        delegate_call!(self
            .file_info
            .file_attrs(AimpString(file_name))
            .map(|a| attrs.write(TAIMPFileAttributes {
                attributes: 0,
                time_creation: a.created.into(),
                time_last_access: a.last_accessed.into(),
                time_last_write: a.last_wrote.into(),
                reserved0: 0,
                reserved1: 0,
                reserved2: 0,
            })))
    }

    unsafe fn get_file_size(
        &self,
        file_name: ComRc<dyn IAIMPString>,
        size: *mut i64,
    ) -> WinHRESULT {
        delegate_call!(self
            .file_info
            .file_size(AimpString(file_name))
            .map(|s| size.write(s)))
    }

    unsafe fn is_file_exists(&self, file_name: ComRc<dyn IAIMPString>) -> WinHRESULT {
        delegate_call!(self.file_info.is_file_exists(AimpString(file_name)))
    }
}

impl IAIMPFileSystemCommandOpenFileFolder for FileSystem {}

impl IAIMPFileSystemCommandStreaming for FileSystem {
    unsafe fn create_stream(
        &self,
        file_name: ComRc<dyn IAIMPString>,
        flags: FileStreamingFlags,
        offset: i64,
        size: i64,
        stream: *mut ComRc<dyn IAIMPStream>,
    ) -> i32 {
        delegate_call!(self
            .streaming
            .create_stream(AimpString(file_name), flags, FileClipping { offset, size })
            .map(|s| stream.write(s.0)))
    }
}

impl Extension for FileSystem {
    const SERVICE_IID: IID = <dyn IAIMPServiceFileSystems as ComInterface>::IID;
}

pub struct BoxedError(Box<dyn std::error::Error>);

impl BoxedError {
    fn new<T: std::error::Error + 'static>(err: T) -> Self {
        Self(Box::new(err))
    }
}

impl std::error::Error for BoxedError {}

impl fmt::Debug for BoxedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for BoxedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

pub struct CommandWrapper<T>(T);

pub trait CustomCommand {
    type Error: std::error::Error;

    fn can_process(&self, file_name: AimpString) -> Result<(), Self::Error>;

    fn process(&self, file_name: AimpString) -> Result<(), Self::Error>;
}

type BoxedCustomCommand = Box<dyn CustomCommand<Error = BoxedError>>;

impl<T> CustomCommand for CommandWrapper<T>
where
    T: CustomCommand,
    T::Error: 'static,
{
    type Error = BoxedError;

    fn can_process(&self, file_name: AimpString) -> Result<(), Self::Error> {
        self.0.can_process(file_name).map_err(BoxedError::new)
    }

    fn process(&self, file_name: AimpString) -> Result<(), Self::Error> {
        self.0.process(file_name).map_err(BoxedError::new)
    }
}

pub trait CopyToClipboardCommand {
    type Error: std::error::Error;

    fn copy_to_clipboard(&self, list: List<AimpString>) -> Result<(), Self::Error>;
}

type BoxedCopyToClipboardCommand = Box<dyn CopyToClipboardCommand<Error = BoxedError>>;

impl<T> CopyToClipboardCommand for CommandWrapper<T>
where
    T: CopyToClipboardCommand,
    T::Error: 'static,
{
    type Error = BoxedError;

    fn copy_to_clipboard(&self, list: List<AimpString>) -> Result<(), Self::Error> {
        self.0.copy_to_clipboard(list).map_err(BoxedError::new)
    }
}

pub trait DropSourceCommand {
    type Error: std::error::Error;

    fn create_stream(&self, file_name: AimpString) -> Result<Stream, Self::Error>;
}

type BoxedDropSourceCommand = Box<dyn DropSourceCommand<Error = BoxedError>>;

impl<T> DropSourceCommand for CommandWrapper<T>
where
    T: DropSourceCommand,
    T::Error: 'static,
{
    type Error = BoxedError;

    fn create_stream(&self, file_name: AimpString) -> Result<Stream, Self::Error> {
        self.0.create_stream(file_name).map_err(BoxedError::new)
    }
}

pub struct FileAttributes {
    // pub attributes: // TODO: match Windows attributes
    pub created: SystemTime,
    pub last_accessed: SystemTime,
    pub last_wrote: SystemTime,
}

pub trait FileInfoCommand {
    type Error: std::error::Error;

    fn file_attrs(&self, file_name: AimpString) -> Result<FileAttributes, Self::Error>;

    fn file_size(&self, file_name: AimpString) -> Result<i64, Self::Error>;

    fn is_file_exists(&self, file_name: AimpString) -> Result<(), Self::Error>;
}

type BoxedFileInfoCommand = Box<dyn FileInfoCommand<Error = BoxedError>>;

impl<T> FileInfoCommand for CommandWrapper<T>
where
    T: FileInfoCommand,
    T::Error: 'static,
{
    type Error = BoxedError;

    fn file_attrs(&self, file_name: AimpString) -> Result<FileAttributes, Self::Error> {
        self.0.file_attrs(file_name).map_err(BoxedError::new)
    }

    fn file_size(&self, file_name: AimpString) -> Result<i64, Self::Error> {
        self.0.file_size(file_name).map_err(BoxedError::new)
    }

    fn is_file_exists(&self, file_name: AimpString) -> Result<(), Self::Error> {
        self.0.is_file_exists(file_name).map_err(BoxedError::new)
    }
}

pub trait StreamingCommand {
    type Error: std::error::Error;

    fn create_stream(
        &self,
        file_name: AimpString,
        flags: FileStreamingFlags,
        clipping: FileClipping,
    ) -> Result<Stream, Self::Error>;
}

type BoxedStreamingCommand = Box<dyn StreamingCommand<Error = BoxedError>>;

impl<T> StreamingCommand for CommandWrapper<T>
where
    T: StreamingCommand,
    T::Error: 'static,
{
    type Error = BoxedError;

    fn create_stream(
        &self,
        file_name: AimpString,
        flags: FileStreamingFlags,
        clipping: FileClipping,
    ) -> Result<Stream<dyn IAIMPStream>, Self::Error> {
        self.0
            .create_stream(file_name, flags, clipping)
            .map_err(BoxedError::new)
    }
}

pub trait FileExpander {
    type Error: std::error::Error;

    fn expand(
        &self,
        file_name: AimpString,
        callback: Option<ProgressCallback>,
    ) -> Result<List<VirtualFile>, Self::Error>;
}

pub struct FileExpanderWrapper<T>(pub T);

impl<T> IAIMPExtensionFileExpander for FileExpanderWrapper<T>
where
    T: FileExpander,
{
    unsafe fn expand(
        &self,
        file_name: ComRc<dyn IAIMPString>,
        list: *mut ComRc<dyn IAIMPObjectList>,
        callback: Option<ComPtr<dyn IAIMPProgressCallback>>,
    ) -> WinHRESULT {
        let res = self
            .0
            .expand(AimpString(file_name), callback.map(ProgressCallback));
        if let Ok(l) = res {
            list.write(l.inner.0);
            S_OK
        } else {
            E_FAIL
        }
    }
}

impl<T: FileExpander> From<FileExpanderWrapper<T>> for ComRc<dyn IAIMPExtensionFileExpander> {
    fn from(wrapper: FileExpanderWrapper<T>) -> Self {
        let wrapper = com_wrapper!(wrapper => dyn IAIMPExtensionFileExpander);
        unsafe { wrapper.into_com_rc() }
    }
}

impl<T> Extension for FileExpanderWrapper<T> {
    const SERVICE_IID: IID = <dyn IAIMPServiceFileManager>::IID;
}

impl<T> ComInterfaceQuerier for FileExpanderWrapper<T> {}

pub trait FileFormat {
    const DESCRIPTION: &'static str;
    const EXTS: &'static [&'static str];
    const FLAGS: FileFormatsCategory;
}

pub struct FileFormatWrapper<T>(pub T);

impl<T: FileFormat> IAIMPExtensionFileFormat for FileFormatWrapper<T> {
    unsafe fn get_description(&self, s: *mut ComRc<dyn IAIMPString>) -> WinHRESULT {
        s.write(AimpString::from(T::DESCRIPTION).0);
        S_OK
    }

    unsafe fn get_ext_list(&self, s: *mut ComRc<dyn IAIMPString>) -> WinHRESULT {
        s.write(AimpString::from(T::EXTS.join(";") + ";").0);
        S_OK
    }

    unsafe fn get_flags(&self, s: *mut FileFormatsCategory) -> WinHRESULT {
        *s = T::FLAGS;
        S_OK
    }
}

impl<T: FileFormat> From<FileFormatWrapper<T>> for ComRc<dyn IAIMPExtensionFileFormat> {
    fn from(wrapper: FileFormatWrapper<T>) -> Self {
        let wrapper = com_wrapper!(wrapper => dyn IAIMPExtensionFileFormat);
        unsafe { wrapper.into_com_rc() }
    }
}

impl<T> Extension for FileFormatWrapper<T> {
    const SERVICE_IID: IID = <dyn IAIMPServiceFileFormats>::IID;
}

impl<T> ComInterfaceQuerier for FileFormatWrapper<T> {}

pub enum FileInfoProviderWrapper<T, U> {
    Uri(T),
    Stream(U),
    UriAndStream(T, U),
}

impl<T> FileInfoProviderWrapper<T, ()> {
    pub fn uri(provider: T) -> Self {
        Self::Uri(provider)
    }
}

impl<U> FileInfoProviderWrapper<(), U> {
    pub fn stream(provider: U) -> Self {
        Self::Stream(provider)
    }
}

impl<T, U> FileInfoProviderWrapper<T, U> {
    pub fn uri_and_stream(uprovider: T, sprovider: U) -> Self {
        Self::UriAndStream(uprovider, sprovider)
    }
}

impl<T, U> IAIMPExtensionFileInfoProvider for FileInfoProviderWrapper<T, U>
where
    T: FileInfoProvider,
{
    unsafe fn get_file_info(
        &self,
        file_uri: ComRc<dyn IAIMPString>,
        info: ComRc<dyn IAIMPFileInfo>,
    ) -> WinHRESULT {
        match self {
            FileInfoProviderWrapper::Uri(provider)
            | FileInfoProviderWrapper::UriAndStream(provider, _) => {
                let uri = FileUri(AimpString(file_uri));
                info.add_ref();
                let mut info = FileInfo::from(info);
                provider.get(uri, info.update()).map_or(E_FAIL, |()| S_OK)
            }
            FileInfoProviderWrapper::Stream(_) => S_OK,
        }
    }
}

impl<T, U> IAIMPExtensionFileInfoProviderEx for FileInfoProviderWrapper<T, U>
where
    U: FileInfoProviderExt,
{
    unsafe fn get_file_info(
        &self,
        stream: ComRc<dyn IAIMPStream>,
        info: ComRc<dyn IAIMPFileInfo>,
    ) -> WinHRESULT {
        match self {
            FileInfoProviderWrapper::Uri(_) => S_OK,
            FileInfoProviderWrapper::Stream(provider)
            | FileInfoProviderWrapper::UriAndStream(_, provider) => {
                let stream = Stream(stream);
                info.add_ref();
                let mut info = FileInfo::from(info);
                provider
                    .get(stream, info.update())
                    .map_or(E_FAIL, |()| S_OK)
            }
        }
    }
}

impl<T, U> From<FileInfoProviderWrapper<T, U>> for ComRc<dyn IAIMPExtensionFileInfoProvider>
where
    T: FileInfoProvider,
    U: FileInfoProviderExt,
{
    fn from(wrapper: FileInfoProviderWrapper<T, U>) -> Self {
        let wrapper = com_wrapper!(
            wrapper =>
            dyn IAIMPExtensionFileInfoProvider,
            dyn IAIMPExtensionFileInfoProviderEx
        );
        unsafe { wrapper.into_com_rc() }
    }
}

impl<T, U> Extension for FileInfoProviderWrapper<T, U> {
    const SERVICE_IID: IID = <dyn IAIMPServiceFileInfo>::IID;
}

impl<T, U> ComInterfaceQuerier for FileInfoProviderWrapper<T, U> {
    fn query_interface(&self, riid: &IID) -> bool {
        let (uri, stream) = if riid == &<dyn IAIMPExtensionFileInfoProvider>::IID {
            (true, false)
        } else if riid == &<dyn IAIMPExtensionFileInfoProviderEx>::IID {
            (false, true)
        } else {
            return true;
        };

        match self {
            FileInfoProviderWrapper::Uri(_) => uri,
            FileInfoProviderWrapper::Stream(_) => stream,
            FileInfoProviderWrapper::UriAndStream(_, _) => uri || stream,
        }
    }
}

pub trait FileInfoProvider {
    type Error: std::error::Error;

    fn get(&self, file_uri: FileUri, guard: FileInfoGuard) -> Result<(), Self::Error>;
}

impl FileInfoProvider for () {
    type Error = Error;

    fn get(&self, _file_uri: FileUri, _guard: FileInfoGuard) -> Result<(), Self::Error> {
        unreachable!()
    }
}

pub trait FileInfoProviderExt {
    type Error: std::error::Error;

    fn get(&self, stream: Stream, guard: FileInfoGuard) -> Result<(), Self::Error>;
}

impl FileInfoProviderExt for () {
    type Error = Error;

    fn get(&self, _stream: Stream, _guard: FileInfoGuard) -> Result<(), Self::Error> {
        unreachable!()
    }
}
