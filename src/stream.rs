use crate::{core::CORE, error::HresultExt, AimpString, Error, ErrorKind, Result};
use futures::io::SeekFrom;
use iaimp::{ComInterface, ComRc, IAIMPFileStream, IAIMPMemoryStream, IAIMPStream, StreamSeekFrom};
use std::{
    io,
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

pub struct Stream<T: ComInterface + IAIMPStream + ?Sized>(pub(crate) ComRc<T>);

impl<T: ComInterface + IAIMPStream + ?Sized> Stream<T> {
    fn as_inner(&self) -> &ComRc<T> {
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

pub struct FileStream(Stream<dyn IAIMPFileStream>);

impl FileStream {
    pub fn clipping(&self) -> Option<FileStreamClipping> {
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
                Some(FileStreamClipping { offset, size })
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

pub struct FileStreamClipping {
    pub offset: i64,
    pub size: i64,
}

pub struct MemoryStream(pub(crate) Stream<dyn IAIMPMemoryStream>);

impl Default for MemoryStream {
    fn default() -> Self {
        Self(Stream(
            CORE.get().create::<dyn IAIMPMemoryStream>().unwrap(),
        ))
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
