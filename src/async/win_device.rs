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

use crate::platform::Device;
use crate::utils::DataExt;
use crate::{AsyncRead, AsyncWrite};
use derive_more::{Deref, DerefMut};
use std::io::{self, IoSlice, IoSliceMut};
use std::pin::Pin;
use std::ptr;
use std::sync::Arc;
use std::task::{Context, Poll};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE,
};

#[derive(Deref, DerefMut)]
pub struct AsyncDevice {
    #[deref]
    #[deref_mut]
    device: Device,
}

impl AsyncDevice {
    pub fn new(device: Device) -> io::Result<AsyncDevice> {
        Ok(AsyncDevice { device })
    }

    pub fn split(self) -> (AsyncReader, AsyncWriter) {
        (
            AsyncReader::new(&self.device),
            AsyncWriter::new(Arc::new(self.device)),
        )
    }
}

pub struct AsyncReader {
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    handle: Option<std::thread::JoinHandle<()>>,
    cancel: usize,
}

impl AsyncReader {
    fn new(device: &Device) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1 << 10);
        let cancel = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) } as usize;
        if cancel == 0 {
            panic!("failed to create cancel event for Wintun AsyncReader");
        }
        let session = device.session();
        let handle = std::thread::spawn(move || {
            let Ok(read) = session.get_read_wait_event() else {
                return;
            };
            let wait = [read as HANDLE, cancel as _];
            loop {
                if unsafe { WaitForMultipleObjects(2, wait.as_ptr(), 0, INFINITE) } != WAIT_OBJECT_0
                {
                    break;
                }
                loop {
                    match session.try_receive() {
                        Ok(Some(packet)) => {
                            if tx.blocking_send(packet.bytes().to_vec()).is_err() {
                                return;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
            }
        });

        Self {
            rx,
            handle: Some(handle),
            cancel,
        }
    }
}

impl AsyncRead for AsyncReader {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.rx)
            .poll_recv(cx)
            .map(|data| Ok(data.map_or(0, |data| data.put(buf))))
    }

    fn poll_read_vectored(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.rx)
            .poll_recv(cx)
            .map(|data| Ok(data.map_or(0, |data| data.putv(bufs))))
    }
}

impl Drop for AsyncReader {
    fn drop(&mut self) {
        let cancel = self.cancel as HANDLE;
        unsafe {
            SetEvent(cancel);
        }
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
        unsafe {
            CloseHandle(cancel);
        }
    }
}

#[derive(Clone)]
pub struct AsyncWriter {
    device: Arc<Device>,
}

impl AsyncWriter {
    fn new(device: Arc<Device>) -> Self {
        Self { device }
    }
}

impl AsyncWrite for AsyncWriter {
    fn poll_write(&mut self, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<()>> {
        Poll::Ready(self.device.send(buf))
    }

    fn poll_write_vectored(
        &mut self,
        _cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<()>> {
        Poll::Ready(self.device.send_vectored(bufs))
    }
}
