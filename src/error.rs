use crate::ErrorInfo;
use std::{error, fmt, io};
use winapi::{shared::winerror::S_OK, um::winnt::HRESULT};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    desc: Option<String>,
    kind: ErrorKind,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(desc) = &self.desc {
            write!(f, "{}: {}", self.kind, desc)
        } else {
            write!(f, "{}", self.kind)
        }
    }
}

impl error::Error for Error {}

#[derive(Debug)]
pub enum ErrorKind {
    Aimp(io::Error),
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorKind::Aimp(err) => err.fmt(f),
        }
    }
}

pub trait HresultExt {
    fn into_result(self) -> Result<()>;

    fn with_error_info(self, info: ErrorInfo) -> Result<()>;
}

impl HresultExt for HRESULT {
    fn into_result(self) -> Result<()> {
        if self == S_OK {
            Ok(())
        } else {
            let err = io::Error::from_raw_os_error(self);
            Err(Error {
                desc: None,
                kind: ErrorKind::Aimp(err),
            })
        }
    }

    fn with_error_info(self, info: ErrorInfo) -> Result<()> {
        let result = self.into_result();
        match result {
            Ok(()) => Ok(()),
            Err(mut err) => {
                err.desc = Some(info.get_formatted().to_string());
                Err(err)
            }
        }
    }
}
