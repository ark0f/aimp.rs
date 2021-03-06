pub use iaimp::{ConnectionType, HttpClientPriorityFlags};

use crate::{
    error::HresultExt,
    file::FileStream,
    prop_list,
    prop_list::{PropertyList, PropertyListAccessor},
    stream::MemoryStream,
    util::Service,
    AimpString, ErrorInfo,
};
use http::{
    header::{ToStrError, CONTENT_LENGTH, CONTENT_TYPE},
    uri::InvalidUri,
    Request, Uri,
};
use iaimp::{
    com_wrapper, ComInterfaceQuerier, ComPtr, ComRc, ConnectionSettingsProp, ConnectionTypeWrapper,
    HttpClientFlags, HttpClientRestFlags, HttpMethod, IAIMPErrorInfo, IAIMPHTTPClientEvents,
    IAIMPHTTPClientEvents2, IAIMPPropertyList, IAIMPServiceConnectionSettings,
    IAIMPServiceHTTPClient2, IAIMPStream, IAIMPString,
};
use std::{
    convert::TryFrom,
    io, mem,
    mem::MaybeUninit,
    os::raw::c_void,
    sync::{
        mpsc,
        mpsc::{Receiver, Sender, SyncSender},
    },
};
use winapi::shared::minwindef::{BOOL, TRUE};

pub static CONNECTION_SETTINGS: Service<ConnectionSettings> = Service::new();
pub static HTTP_CLIENT: Service<HttpClient> = Service::new();

prop_list! {
    list: ConnectionSettings(ComPtr<dyn IAIMPServiceConnectionSettings>),
    prop: ConnectionSettingsProp,
    guard: ConnectionSettingsGuard,
    methods:
    proxy_server(ProxyServer) -> AimpString,
    proxy_port(ProxyPort) -> AimpString,
    proxy_username(ProxyUsername) -> AimpString,
    proxy_user_pass(ProxyUserPass) -> AimpString,
    timeout(Timeout) -> i32,
    user_agent(UserAgent) -> AimpString,
    connection_type(ConnectionType) -> Option<ConnectionType>,
}

impl<T: IAIMPPropertyList> PropertyListAccessor<T> for Option<ConnectionType> {
    fn get(id: i32, list: &PropertyList<T>) -> Self {
        let ty = i32::get(id, list);
        let wrapper: ConnectionTypeWrapper = unsafe { mem::transmute(ty) };
        wrapper.into_inner()
    }

    fn set(self, id: i32, list: &mut PropertyList<T>) {
        self.map(|ty| ty as i32).set(id, list)
    }
}

impl From<ComPtr<dyn IAIMPServiceConnectionSettings>> for ConnectionSettings {
    fn from(ptr: ComPtr<dyn IAIMPServiceConnectionSettings>) -> Self {
        Self {
            prop_list: PropertyList::from(ptr),
        }
    }
}

type Result<T> = std::result::Result<T, HttpError>;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{0}")]
    ToStr(
        #[from]
        #[source]
        ToStrError,
    ),
    #[error("{0}")]
    Io(
        #[from]
        #[source]
        io::Error,
    ),
    #[error("{0}")]
    InvalidUri(
        #[from]
        #[source]
        InvalidUri,
    ),
    #[error("{0}")]
    Http(
        #[from]
        #[source]
        http::Error,
    ),
    #[error("Task was canceled by user")]
    Canceled,
    #[error("{0}")]
    Failed(ErrorInfo),
    #[error("Method is not supported")]
    UnsupportedMethod,
}

pub struct HttpClient(ComPtr<dyn IAIMPServiceHTTPClient2>);

impl HttpClient {
    pub fn request<T: Body>(req: Request<T>) -> RequestBuilder<T> {
        req.into()
    }

    pub fn get<T>(uri: T) -> Result<RequestBuilder<()>>
    where
        Uri: TryFrom<T, Error = InvalidUri>,
    {
        Ok(Request::get(uri).body(())?.into())
    }
}

impl From<ComPtr<dyn IAIMPServiceHTTPClient2>> for HttpClient {
    fn from(ptr: ComPtr<dyn IAIMPServiceHTTPClient2>) -> Self {
        Self(ptr)
    }
}

pub trait Body {
    fn into_stream(self) -> Option<Result<ComRc<dyn IAIMPStream>>>;
}

impl Body for () {
    fn into_stream(self) -> Option<Result<ComRc<dyn IAIMPStream>>> {
        None
    }
}

impl Body for MemoryStream {
    fn into_stream(self) -> Option<Result<ComRc<dyn IAIMPStream>>> {
        unsafe { Some(Ok((self.0).0.cast())) }
    }
}

impl Body for FileStream {
    fn into_stream(self) -> Option<Result<ComRc<dyn IAIMPStream>>> {
        unsafe { Some(Ok((self.0).0.cast())) }
    }
}

pub struct RequestBuilder<T> {
    request: Request<Option<T>>,
    priority: HttpClientPriorityFlags,
}

impl<T> RequestBuilder<T>
where
    T: Body,
{
    pub fn priority(mut self, priority: HttpClientPriorityFlags) -> Self {
        self.priority = priority;
        self
    }

    fn make_uri_and_headers(&self) -> Result<AimpString> {
        let uri = self.request.uri().to_string();
        let headers = self
            .request
            .headers()
            .iter()
            .map(|(k, v)| Ok(format!("\r\n{}: {}", k, v.to_str()?)))
            .collect::<Result<String>>()?;
        Ok(AimpString::from(uri + &headers))
    }

    fn match_method(&self) -> Result<HttpMethod> {
        match *self.request.method() {
            http::Method::GET => Ok(HttpMethod::Get),
            http::Method::POST => Ok(HttpMethod::Post),
            http::Method::PUT => Ok(HttpMethod::Put),
            http::Method::DELETE => Ok(HttpMethod::Delete),
            http::Method::HEAD => Ok(HttpMethod::Head),
            _ => Err(HttpError::UnsupportedMethod),
        }
    }

    fn inner_send(mut self, flags: HttpClientRestFlags) -> Result<HttpTask> {
        let uri_and_headers = self.make_uri_and_headers()?.0;
        let method = self.match_method()?;
        let flags = HttpClientFlags::new(HttpClientRestFlags::UTF8 | flags, self.priority);
        let answer_data = MemoryStream::default();
        let post_data = self
            .request
            .body_mut()
            .take()
            .unwrap()
            .into_stream()
            .transpose()?;

        let downloaded = mpsc::channel();
        let status = mpsc::sync_channel(1);
        let content_info = mpsc::sync_channel(1);
        let complete = mpsc::sync_channel(1);
        let events_handler = EventsHandler {
            downloaded: downloaded.0,
            status: status.0,
            content_info: content_info.0,
            complete: complete.0,
        };
        let events_handler =
            com_wrapper!(events_handler => dyn IAIMPHTTPClientEvents, dyn IAIMPHTTPClientEvents2);
        let mut task_id = MaybeUninit::uninit();

        unsafe {
            HTTP_CLIENT
                .get()
                .0
                .request(
                    uri_and_headers,
                    method,
                    flags,
                    (*answer_data).0.as_raw().cast(),
                    post_data,
                    events_handler.into_com_rc(),
                    None,
                    task_id.as_mut_ptr(),
                )
                .into_result()
                .unwrap();

            Ok(HttpTask {
                id: task_id.assume_init(),
                answer_data,
                downloaded: downloaded.1,
                status: status.1,
                content_info: content_info.1,
                complete: complete.1,
            })
        }
    }

    pub fn send(self) -> Result<HttpTask> {
        self.inner_send(HttpClientRestFlags::NONE)
    }

    pub fn send_and_wait(self) -> Result<http::Response<MemoryStream>> {
        self.inner_send(HttpClientRestFlags::WAIT_FOR)?.wait()
    }
}

impl<T> From<Request<T>> for RequestBuilder<T> {
    fn from(request: Request<T>) -> Self {
        let (parts, body) = request.into_parts();
        Self {
            request: Request::from_parts(parts, Some(body)),
            priority: Default::default(),
        }
    }
}

pub struct HttpTask {
    id: *const c_void,
    answer_data: MemoryStream,
    pub downloaded: Receiver<u32>,
    status: Receiver<AimpString>,
    content_info: Receiver<(AimpString, u32)>,
    complete: Receiver<(Option<ErrorInfo>, BOOL)>,
}

impl HttpTask {
    fn inner_cancel(self, rest: HttpClientRestFlags) {
        unsafe {
            HTTP_CLIENT
                .get()
                .0
                .cancel(
                    self.id,
                    HttpClientFlags::new(rest, HttpClientPriorityFlags::Normal),
                )
                .into_result()
                .unwrap();
        }
    }

    pub fn cancel(self) {
        self.inner_cancel(HttpClientRestFlags::NONE)
    }

    pub fn cancel_and_wait(self) {
        self.inner_cancel(HttpClientRestFlags::WAIT_FOR)
    }

    pub fn wait(self) -> Result<http::Response<MemoryStream>> {
        let (info, canceled) = self.complete.recv().unwrap();
        match (info, canceled == TRUE) {
            (_, true) => Err(HttpError::Canceled),
            (Some(info), false) => Err(HttpError::Failed(info)),
            (None, false) => {
                let mut builder = http::Response::builder();

                let status_line = self.status.recv().unwrap().to_string();
                let status = status_line.split_ascii_whitespace().nth(1).unwrap();
                builder = builder.status(status);

                let (content_type, content_length) = self.content_info.recv().unwrap();
                builder = builder
                    .header(CONTENT_TYPE, content_type.to_string())
                    .header(CONTENT_LENGTH, content_length);

                Ok(builder.body(self.answer_data)?)
            }
        }
    }
}

struct EventsHandler {
    downloaded: Sender<u32>,
    status: SyncSender<AimpString>,
    content_info: SyncSender<(AimpString, u32)>,
    complete: SyncSender<(Option<ErrorInfo>, BOOL)>,
}

impl IAIMPHTTPClientEvents for EventsHandler {
    unsafe fn on_accept(
        &self,
        content_type: ComRc<dyn IAIMPString>,
        content_size: i64,
        allow: *mut BOOL,
    ) {
        *allow = TRUE;
        self.content_info
            .send((AimpString(content_type), content_size as u32))
            .unwrap();
    }

    unsafe fn on_complete(&self, error_info: Option<ComRc<dyn IAIMPErrorInfo>>, canceled: BOOL) {
        self.complete
            .send((error_info.map(ErrorInfo), canceled))
            .unwrap();
    }

    unsafe fn on_progress(&self, downloaded: i64, _total: i64) {
        self.downloaded.send(downloaded as u32).unwrap();
    }
}

impl IAIMPHTTPClientEvents2 for EventsHandler {
    unsafe fn on_accept_headers(&self, header: ComRc<dyn IAIMPString>, allow: *mut BOOL) {
        *allow = TRUE;
        self.status.send(AimpString(header)).unwrap();
    }
}

impl ComInterfaceQuerier for EventsHandler {}
