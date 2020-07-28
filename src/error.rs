use crate::ErrorInfo;
use std::{error, fmt, io};
use winapi::{shared::winerror::S_OK, um::winnt::HRESULT};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct Error {
    desc: Option<String>,
    kind: ErrorKind,
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self { desc: None, kind }
    }
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
    Hresult(io::Error),
    Unexpected,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorKind::Hresult(err) => err.fmt(f),
            ErrorKind::Unexpected => "unexpected".fmt(f),
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
                kind: ErrorKind::Hresult(err),
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
