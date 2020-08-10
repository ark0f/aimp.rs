use crate::{core::CORE, error::HresultExt, Error, ErrorKind, Result};
use futures::io::SeekFrom;
use iaimp::{ComInterface, ComRc, IAIMPMemoryStream, IAIMPStream, StreamSeekFrom};
use std::{
    fmt, io,
    io::{Read, Seek, Write},
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    slice,
};
use winapi::shared::winerror::E_FAIL;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Failed to change position at specified offset")]
    Offset,
}

pub struct Stream<T: ComInterface + IAIMPStream + ?Sized = dyn IAIMPStream>(pub(crate) ComRc<T>);

impl<T: ComInterface + IAIMPStream + ?Sized> Stream<T> {
    pub(crate) fn as_inner(&self) -> &ComRc<T> {
        &self.0
    }

    pub fn size(&self) -> i64 {
        unsafe { self.0.get_size() }
    }

    pub fn set_size(&mut self, size: i64) -> Result<()> {
        unsafe { self.0.set_size(size).into_result() }
    }

    pub fn pos(&self) -> i64 {
        unsafe { self.0.get_position() }
    }
}

impl<T: ComInterface + IAIMPStream + ?Sized> Seek for Stream<T> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let (offset, mode) = match pos {
            SeekFrom::Start(offset) => (offset as i64, StreamSeekFrom::Beginning),
            SeekFrom::End(offset) => (offset, StreamSeekFrom::End),
            SeekFrom::Current(offset) => (offset, StreamSeekFrom::Current),
        };

        let res = unsafe { self.0.seek(offset, mode) };
        if res == E_FAIL {
            Err(StreamError::Offset).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        } else {
            res.into_result()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        }

        Ok(self.pos() as u64)
    }
}

impl<T: ComInterface + IAIMPStream + ?Sized> Read for Stream<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let written = unsafe { self.0.read(buf.as_mut_ptr(), buf.len() as _) };
        if written == -1 {
            Err(io::Error::new(
                io::ErrorKind::Other,
                Error::from(ErrorKind::Unexpected),
            ))
        } else {
            Ok(written as usize)
        }
    }
}

impl<T: ComInterface + IAIMPStream + ?Sized> Write for Stream<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            let mut written = MaybeUninit::uninit();
            self.0
                .write(buf.as_ptr(), buf.len() as _, written.as_mut_ptr())
                .into_result()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let written = written.assume_init();
            Ok(written as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T: ComInterface + IAIMPStream + ?Sized> fmt::Debug for Stream<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[derive(Debug)]
pub struct MemoryStream(pub(crate) Stream<dyn IAIMPMemoryStream>);

impl Default for MemoryStream {
    fn default() -> Self {
        Self(Stream(CORE.get().create().unwrap()))
    }
}

impl AsRef<[u8]> for MemoryStream {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_inner().get_data(), self.size() as usize) }
    }
}

impl Deref for MemoryStream {
    type Target = Stream<dyn IAIMPMemoryStream>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MemoryStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<MemoryStream> for Stream {
    fn from(memory_stream: MemoryStream) -> Self {
        unsafe { Stream((memory_stream.0).0.cast()) }
    }
}
