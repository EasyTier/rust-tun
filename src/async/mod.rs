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

//! Async specific modules.

use std::future::poll_fn;
use std::future::Future;
use std::io;
use std::io::{IoSlice, IoSliceMut};
use std::task::{Context, Poll, Waker};

use crate::configuration::Configuration;
use crate::error;
use crate::platform::create;

mod codec;
pub use codec::TunPacketCodec;

#[cfg(unix)]
mod unix_device;
#[cfg(unix)]
pub use unix_device::{AsyncDevice, AsyncReader, AsyncWriter};

#[cfg(target_os = "windows")]
mod win_device;
#[cfg(target_os = "windows")]
pub use win_device::{AsyncDevice, AsyncReader, AsyncWriter};

#[inline]
fn try_poll_fn<T>(mut f: impl FnMut(&mut Context<'_>) -> Poll<io::Result<T>>) -> io::Result<T> {
    let mut cx = Context::from_waker(Waker::noop());

    match f(&mut cx) {
        Poll::Ready(result) => result,
        Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
    }
}

pub trait AsyncRead {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>>;
    fn poll_read_vectored(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>>;
}

pub trait AsyncReadExt: AsyncRead {
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>>
    where
        Self: Send,
    {
        poll_fn(|cx| self.poll_read(cx, buf))
    }

    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        try_poll_fn(|cx| self.poll_read(cx, buf))
    }

    fn read_vectored(
        &mut self,
        bufs: &mut [IoSliceMut<'_>],
    ) -> impl Future<Output = io::Result<usize>>
    where
        Self: Send,
    {
        poll_fn(|cx| self.poll_read_vectored(cx, bufs))
    }

    fn try_read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        try_poll_fn(|cx| self.poll_read_vectored(cx, bufs))
    }
}

impl<T: AsyncRead + ?Sized> AsyncReadExt for T {}

pub trait AsyncWrite {
    fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<()>>;
    fn poll_write_vectored(
        &mut self,
        _cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<()>>;
}

pub trait AsyncWriteExt: AsyncWrite {
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = io::Result<()>>
    where
        Self: Send,
    {
        poll_fn(|cx| self.poll_write(cx, buf))
    }

    fn try_write(&mut self, buf: &[u8]) -> io::Result<()> {
        try_poll_fn(|cx| self.poll_write(cx, buf))
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> impl Future<Output = io::Result<()>>
    where
        Self: Send,
    {
        poll_fn(|cx| self.poll_write_vectored(cx, bufs))
    }

    fn try_write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<()> {
        try_poll_fn(|cx| self.poll_write_vectored(cx, bufs))
    }
}

impl<T: AsyncWrite + ?Sized> AsyncWriteExt for T {}

/// Create a TUN device with the given name.
pub fn create_as_async(configuration: &Configuration) -> Result<AsyncDevice, error::Error> {
    let device = create(configuration)?;
    AsyncDevice::new(device).map_err(|err| err.into())
}
