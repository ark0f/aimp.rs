pub use iaimp::ConnectionType;

use crate::{
    error::HresultExt, msg_box, stream::MemoryStream, util::Static, AimpString, Error, ErrorInfo,
    PropertyList,
};
use http::{
    header::{IntoHeaderName, ToStrError},
    uri::InvalidUri,
    HeaderMap, HeaderValue, Uri,
};
use iaimp::{
    com_wrapper, ComPtr, ComRc, ConnectionSettingsProp, ConnectionTypeWrapper, HttpClientFlags,
    HttpClientPriorityFlags, HttpClientRestFlags, HttpMethod, IAIMPErrorInfo,
    IAIMPHTTPClientEvents, IAIMPHTTPClientEvents2, IAIMPServiceConnectionSettings,
    IAIMPServiceHTTPClient2, IAIMPStream, IAIMPString,
};
use std::{
    convert::TryFrom,
    io, mem,
    mem::MaybeUninit,
    os::raw::c_void,
    sync::{
        mpsc,
        mpsc::{Receiver, Sender},
    },
};
use winapi::shared::minwindef::{BOOL, FALSE, TRUE};

pub static CONNECTION_SETTINGS: Static<ConnectionSettings> = Static::new();
pub static HTTP_CLIENT: Static<HttpClient> = Static::new();

pub struct ConnectionSettings(PropertyList);

impl ConnectionSettings {
    pub fn connection_type(&self) -> Option<ConnectionType> {
        unsafe {
            mem::transmute::<i32, ConnectionTypeWrapper>(
                self.0.get(ConnectionSettingsProp::ConnectionType as i32),
            )
            .into_inner()
        }
    }

    pub fn set_connection_type(&mut self, kind: ConnectionType) {
        self.0
            .update()
            .set(ConnectionSettingsProp::ConnectionType as i32, kind as i32);
    }

    pub fn proxy_server(&self) -> AimpString {
        self.0.get(ConnectionSettingsProp::ProxyServer as i32)
    }

    pub fn set_proxy_server(&mut self, server: AimpString) {
        self.0
            .update()
            .set(ConnectionSettingsProp::ProxyServer as i32, server);
    }

    pub fn proxy_port(&self) -> AimpString {
        self.0.get(ConnectionSettingsProp::ProxyPort as i32)
    }

    pub fn set_proxy_port(&mut self, port: AimpString) {
        self.0
            .update()
            .set(ConnectionSettingsProp::ProxyPort as i32, port);
    }

    pub fn proxy_username(&self) -> AimpString {
        self.0.get(ConnectionSettingsProp::ProxyUsername as i32)
    }

    pub fn set_proxy_username(&mut self, username: AimpString) {
        self.0
            .update()
            .set(ConnectionSettingsProp::ProxyUsername as i32, username);
    }

    pub fn proxy_user_pass(&self) -> AimpString {
        self.0.get(ConnectionSettingsProp::ProxyUserPass as i32)
    }

    pub fn set_proxy_user_pass(&mut self, user_pass: AimpString) {
        self.0
            .update()
            .set(ConnectionSettingsProp::ProxyUserPass as i32, user_pass);
    }

    pub fn timeout(&self) -> i32 {
        self.0.get(ConnectionSettingsProp::Timeout as i32)
    }

    pub fn set_timeout(&mut self, timeout: i32) {
        self.0
            .update()
            .set(ConnectionSettingsProp::Timeout as i32, timeout);
    }

    pub fn user_agent(&self) -> AimpString {
        self.0.get(ConnectionSettingsProp::UserAgent as i32)
    }

    pub fn set_user_agent(&mut self, user_agent: AimpString) {
        self.0
            .update()
            .set(ConnectionSettingsProp::UserAgent as i32, user_agent);
    }
}

impl From<ComPtr<dyn IAIMPServiceConnectionSettings>> for ConnectionSettings {
    fn from(ptr: ComPtr<dyn IAIMPServiceConnectionSettings>) -> Self {
        Self(PropertyList::from(ptr))
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
    Aimp(
        #[from]
        #[source]
        Error,
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
}

pub struct HttpClient(ComPtr<dyn IAIMPServiceHTTPClient2>);

impl HttpClient {
    pub fn get<T>(uri: T) -> Result<RequestBuilder<()>>
    where
        Uri: TryFrom<T, Error = InvalidUri>,
    {
        Ok(RequestBuilder {
            method: HttpMethod::Get,
            uri: Uri::try_from(uri)?,
            headers: HeaderMap::new(),
            body: None,
            priority: HttpClientPriorityFlags::Normal,
        })
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

pub struct RequestBuilder<T> {
    method: HttpMethod,
    uri: Uri,
    headers: HeaderMap,
    body: Option<T>,
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

    pub fn body(mut self, body: T) -> Self {
        self.body = Some(body);
        self
    }

    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    pub fn header<K, V>(mut self, key: K, value: V) -> Self
    where
        K: IntoHeaderName,
        V: Into<HeaderValue>,
    {
        self.headers.insert(key, value.into());
        self
    }

    pub fn build(self) -> Request<T> {
        Request { builder: self }
    }

    fn make_uri_and_headers(uri: Uri, headers: HeaderMap) -> Result<AimpString> {
        let uri = uri.to_string();
        let (uri_and_headers, _) = headers.into_iter().try_fold(
            (uri, None),
            |(mut uri, mut last_header), (name, value)| {
                let name = name
                    .or(last_header)
                    .expect("Header name at least on the first iteration");
                uri += &format!("\r\n{}: {}", name, value.to_str()?);
                last_header = Some(name);
                Ok::<_, HttpError>((uri, last_header))
            },
        )?;
        Ok(AimpString::from(uri_and_headers))
    }

    pub fn send(self) -> Result<HttpTask> {
        let uri_and_headers = Self::make_uri_and_headers(self.uri, self.headers)?.0;
        let method = self.method;
        let flags = HttpClientFlags::new(HttpClientRestFlags::UTF8, self.priority);
        let answer_data = MemoryStream::default();
        let post_data = self.body.and_then(Body::into_stream).transpose()?;

        let downloaded = mpsc::channel();
        let headers = mpsc::channel();
        let events_handler = EventsHandler {
            downloaded: downloaded.0,
            headers: headers.0,
        };
        let events_handler = com_wrapper!(events_handler => EventsHandler: dyn IAIMPHTTPClientEvents, dyn IAIMPHTTPClientEvents2);
        let mut task_id = MaybeUninit::uninit();

        unsafe {
            HTTP_CLIENT
                .get()
                .0
                .request(
                    uri_and_headers,
                    method,
                    flags,
                    (*answer_data).0.as_raw().clone().cast(),
                    post_data,
                    events_handler.into_com_rc(),
                    None,
                    task_id.as_mut_ptr(),
                )
                .into_result()?;

            Ok(HttpTask {
                id: task_id.assume_init(),
                answer_data,
                downloaded: downloaded.1,
                headers: headers.1,
            })
        }
    }
}

pub struct Request<T> {
    builder: RequestBuilder<T>,
}

pub struct Response {
    body: Vec<u8>,
    headers: HeaderMap,
    error: Option<ErrorInfo>,
}

pub struct HttpTask {
    id: *const c_void,
    answer_data: MemoryStream,
    pub downloaded: Receiver<u32>,
    pub headers: Receiver<AimpString>,
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
}

struct EventsHandler {
    downloaded: Sender<u32>,
    headers: Sender<AimpString>,
}

impl IAIMPHTTPClientEvents for EventsHandler {
    unsafe fn on_accept(
        &self,
        _content_type: ComRc<dyn IAIMPString>,
        _content_size: i64,
        allow: *mut BOOL,
    ) {
        *allow = TRUE;
        msg_box!("on_accept");
    }

    unsafe fn on_complete(&self, error_info: Option<ComRc<dyn IAIMPErrorInfo>>, canceled: BOOL) {
        msg_box!("on_complete");
        assert_eq!(error_info.is_some(), canceled == FALSE);
    }

    unsafe fn on_progress(&self, downloaded: i64, _total: i64) {
        msg_box!("on_progress");
        self.downloaded.send(downloaded as u32).unwrap();
    }
}

impl IAIMPHTTPClientEvents2 for EventsHandler {
    unsafe fn on_accept_headers(&self, header: ComRc<dyn IAIMPString>, allow: *mut BOOL) {
        *allow = TRUE;
        msg_box!("on_accept_headers");
        self.headers.send(AimpString(header)).unwrap();
    }
}
