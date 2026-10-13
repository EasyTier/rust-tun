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

use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;
use std::io::Error;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::codec::Framed;

use super::TunPacketCodec;
use crate::device::AbstractDevice;
use crate::platform::Device;

/// An async TUN device wrapper around a TUN device.
pub struct AsyncDevice {
    inner: Device,
    session: WinSession,
}

/// Returns a shared reference to the underlying Device object.
impl core::ops::Deref for AsyncDevice {
    type Target = Device;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Returns a mutable reference to the underlying Device object.
impl core::ops::DerefMut for AsyncDevice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl AsyncDevice {
    /// Create a new `AsyncDevice` wrapping around a `Device`.
    pub fn new(device: Device) -> io::Result<AsyncDevice> {
        let session = WinSession::new(device.tun.get_session())?;
        Ok(AsyncDevice {
            inner: device,
            session,
        })
    }

    /// Consumes this AsyncDevice and return a Framed object (unified Stream and Sink interface)
    pub fn into_framed(self) -> Framed<Self, TunPacketCodec> {
        let mtu = self.mtu().unwrap_or(crate::DEFAULT_MTU);
        let codec = TunPacketCodec::new(mtu as usize);
        // guarantee to avoid the mtu of wintun may far away larger than the default provided capacity of ReadBuf of Framed
        Framed::with_capacity(self, codec, mtu as usize)
    }

    /// Recv a packet from tun device - Not implemented for windows
    pub async fn recv(&self, _buf: &mut [u8]) -> std::io::Result<usize> {
        unimplemented!()
    }

    /// Send a packet to tun device - Not implemented for windows
    pub async fn send(&self, _buf: &[u8]) -> std::io::Result<usize> {
        unimplemented!()
    }
}

impl AsyncRead for AsyncDevice {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.session).poll_read(cx, buf)
    }
}

impl AsyncWrite for AsyncDevice {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        Pin::new(&mut self.session).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.session).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.session).poll_shutdown(cx)
    }
}

struct WinSession {
    session: std::sync::Arc<wintun::Session>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    task: Option<std::thread::JoinHandle<()>>,
}

impl WinSession {
    fn new(session: std::sync::Arc<wintun::Session>) -> Result<WinSession, io::Error> {
        let session_reader = session.clone();
        let (receiver_tx, receiver_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let task = std::thread::spawn(move || loop {
            match session_reader.receive_blocking() {
                Ok(packet) => {
                    if receiver_tx.send(packet.bytes().to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    log::info!("{}", err);
                    break;
                }
            }
        });

        Ok(WinSession {
            session,
            receiver: receiver_rx,
            task: Some(task),
        })
    }
}

impl Drop for WinSession {
    fn drop(&mut self) {
        // Stop forwarding queued packets and wake a reader blocked on an empty ring.
        self.receiver.close();
        if let Err(err) = self.session.shutdown() {
            log::error!("failed to shutdown session: {:?}", err);
        }
        // The reader owns a session reference. Join it before returning so the
        // adapter is released before another device with the same name is opened.
        if let Some(task) = self.task.take() {
            if let Err(err) = task.join() {
                log::error!("failed to join session reader: {:?}", err);
            }
        }
    }
}

impl AsyncRead for WinSession {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match std::task::ready!(self.receiver.poll_recv(cx)) {
            Some(bytes) => {
                buf.put_slice(&bytes);
                std::task::Poll::Ready(Ok(()))
            }
            None => std::task::Poll::Ready(Ok(())),
        }
    }
}

impl AsyncWrite for WinSession {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let mut write_pack = self.session.allocate_send_packet(buf.len() as u16)?;
        write_pack.bytes_mut().copy_from_slice(buf.as_ref());
        self.session.send_packet(write_pack);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::AsyncDevice;
    use crate::{Configuration, Layer};
    use std::{sync::Arc, time::Duration};

    // Run as Administrator with wintun.dll beside the test executable.
    #[test]
    #[ignore = "requires Administrator privileges and wintun.dll"]
    fn recreate_async_device_while_another_device_is_running() {
        let mut config = Configuration::default();
        config.layer(Layer::L3).tun_name("tun-test-recreate");
        config.platform_config(|config| {
            config.skip_config(true);
            config.ring_cap(Some(config.min_ring_cap()));
            config.device_guid(0x8bdb7622_88bc_42fd_b7ee_dc7704c746da);
        });

        let mut other_config = config.clone();
        other_config.tun_name("tun-test-running");
        other_config.platform_config(|config| {
            config.device_guid(0x7a0c7f82_32c9_4c04_9680_cfc44fdc2136);
        });
        let other = AsyncDevice::new(crate::create(&other_config).unwrap()).unwrap();

        for _ in 0..100 {
            let device = AsyncDevice::new(crate::create(&config).unwrap()).unwrap();
            let session = Arc::downgrade(&device.inner.tun.get_session());
            // Give the worker a chance to enter receive_blocking before closing.
            std::thread::sleep(Duration::from_millis(5));
            drop(device);
            assert!(session.upgrade().is_none(), "reader retained the session");
            assert!(!other.session.task.as_ref().unwrap().is_finished());
        }
    }
}
