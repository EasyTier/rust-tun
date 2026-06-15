//            DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
//                    Version 2, December 2004
//
// Copyleft (ↄ) meh. <meh@schizofreni.co> | http://meh.schizofreni.co
//
// Everyone is permitted to copy and distribute verbatim or modified
// copies of this license document, and changing it is allowed as long
// as the name is changed.
//
//            DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
//   TERMS AND CONDITIONS FOR COPYING, DISTRIBUTION AND MODIFICATION
//
//  0. You just DO WHAT THE FUCK YOU WANT TO.

use crate::AsyncRead;
use crate::AsyncWrite;
use std::io::{self, IoSlice, IoSliceMut};
use std::task::ready;
use std::task::Context;
use std::task::Poll;
use tokio::io::unix::AsyncFd;

use crate::platform::Device;

/// An async TUN device wrapper around a TUN device.
pub struct AsyncDevice {
    device: AsyncFd<Device>,
}

/// Returns a shared reference to the underlying Device object.
impl core::ops::Deref for AsyncDevice {
    type Target = Device;

    fn deref(&self) -> &Self::Target {
        self.device.get_ref()
    }
}

/// Returns a mutable reference to the underlying Device object.
impl core::ops::DerefMut for AsyncDevice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.device.get_mut()
    }
}

impl AsyncDevice {
    /// Create a new `AsyncDevice` wrapping around a `Device`.
    pub fn new(device: Device) -> std::io::Result<AsyncDevice> {
        device.set_nonblock()?;
        Ok(AsyncDevice {
            device: AsyncFd::new(device)?,
        })
    }

    pub fn split(self) -> (AsyncReader, AsyncWriter) {
        let device = std::sync::Arc::new(self.device);
        (
            AsyncReader::new(device.clone()),
            AsyncWriter::new(device.clone()),
        )
    }
}

pub struct AsyncReader {
    device: std::sync::Arc<AsyncFd<Device>>,
}

impl AsyncReader {
    fn new(device: std::sync::Arc<AsyncFd<Device>>) -> Self {
        Self { device }
    }
}

impl AsyncRead for AsyncReader {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.device.poll_read_ready(cx))?;
            match guard.try_io(|device| device.get_ref().recv(buf)) {
                Ok(r) => return Poll::Ready(r),
                Err(_) => continue,
            }
        }
    }

    fn poll_read_vectored(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        loop {
            let mut guard = ready!(self.device.poll_read_ready(cx))?;
            match guard.try_io(|device| device.get_ref().recv_vectored(bufs)) {
                Ok(r) => return Poll::Ready(r),
                Err(_) => continue,
            }
        }
    }
}

#[derive(Clone)]
pub struct AsyncWriter {
    device: std::sync::Arc<AsyncFd<Device>>,
}

impl AsyncWriter {
    fn new(device: std::sync::Arc<AsyncFd<Device>>) -> Self {
        Self { device }
    }
}

impl AsyncWrite for AsyncWriter {
    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<()>> {
        loop {
            let mut guard = ready!(self.device.poll_write_ready(cx))?;
            match guard.try_io(|device| device.get_ref().send(buf)) {
                Ok(r) => return Poll::Ready(r),
                Err(_) => continue,
            }
        }
    }

    fn poll_write_vectored(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<()>> {
        loop {
            let mut guard = ready!(self.device.poll_write_ready(cx))?;
            match guard.try_io(|device| device.get_ref().send_vectored(bufs)) {
                Ok(r) => return Poll::Ready(r),
                Err(_) => continue,
            }
        }
    }
}
