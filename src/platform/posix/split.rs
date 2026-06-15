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

use crate::platform::posix::split::io::IoSlice;
use crate::platform::posix::split::io::IoSliceMut;
use crate::platform::posix::Fd;
use crate::utils::SliceExt;
use crate::PACKET_INFORMATION_LENGTH as PIL;
use delegate::delegate;
use smallvec::SmallVec;
use std::io::{self, Read, Write};
use std::iter;
use std::os::unix::io::{AsRawFd, IntoRawFd, RawFd};
use std::sync::Arc;

/// Infer the protocol based on the first nibble in the packet buffer.
pub(crate) fn is_ipv6(buf: &[u8]) -> std::io::Result<bool> {
    use std::io::{Error, ErrorKind::InvalidData};
    if buf.is_empty() {
        return Err(Error::new(InvalidData, "Zero-length data"));
    }
    match buf[0] >> 4 {
        4 => Ok(false),
        6 => Ok(true),
        p => Err(Error::new(InvalidData, format!("IP version {}", p))),
    }
}

pub(crate) const fn packet_information(ipv6: bool) -> [u8; PIL] {
    cfg_select! {
        any(target_os = "linux", target_os = "android") => {
            const TUN_PROTO_IP6: [u8; PIL] = (libc::ETH_P_IPV6 as u32).to_be_bytes();
            const TUN_PROTO_IP4: [u8; PIL] = (libc::ETH_P_IP as u32).to_be_bytes();
        }
        any(target_os = "macos", target_os = "ios") => {
            const TUN_PROTO_IP6: [u8; PIL] = (libc::AF_INET6 as u32).to_be_bytes();
            const TUN_PROTO_IP4: [u8; PIL] = (libc::AF_INET as u32).to_be_bytes();
        }
        // FIXME: Currently, the FreeBSD we test (FreeBSD-14.0-RELEASE) seems to have no PI. Here just a dummy.
        target_os = "freebsd" => {
            const TUN_PROTO_IP6: [u8; PIL] = 0x86DD_u32.to_be_bytes();
            const TUN_PROTO_IP4: [u8; PIL] = 0x0800_u32.to_be_bytes();
        }
    }

    if ipv6 {
        TUN_PROTO_IP6
    } else {
        TUN_PROTO_IP4
    }
}

/// Read-only end for a file descriptor.
pub struct Reader {
    pub(crate) fd: Arc<Fd>,
    pub(crate) offset: usize,
    pub(crate) mtu: u16,
}

impl Reader {
    pub(crate) fn set_mtu(&mut self, value: u16) {
        self.mtu = value;
    }

    pub(crate) fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        if self.offset == 0 {
            return self.fd.read(buf);
        }

        let mut header = [0u8; PIL];

        let amount = self.fd.readv(&mut [
            IoSliceMut::new(&mut header[..self.offset]),
            IoSliceMut::new(buf),
        ])?;

        if amount <= self.offset {
            Ok(0)
        } else {
            Ok(amount - self.offset)
        }
    }

    pub(crate) fn recv_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        if self.offset == 0 {
            return self.fd.readv(bufs);
        }

        let mut header = [0u8; PIL];

        let mut bufs = iter::once(IoSliceMut::new(&mut header[..self.offset]))
            .chain(
                bufs.iter_mut()
                    .filter(|b| !b.is_empty())
                    .map(|buf| IoSliceMut::new(buf)),
            )
            .collect::<SmallVec<[IoSliceMut<'_>; 32]>>();

        let amount = self.fd.readv(&mut bufs)?;

        if amount <= self.offset {
            Ok(0)
        } else {
            Ok(amount - self.offset)
        }
    }
}

impl Read for Reader {
    delegate! {
        to self {
            #[call(recv)]
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
            #[call(recv_vectored)]
            fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize>;
        }
    }
}

impl AsRawFd for Reader {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

/// Write-only end for a file descriptor.
pub struct Writer {
    pub(crate) fd: Arc<Fd>,
    pub(crate) offset: usize,
    pub(crate) mtu: u16,
}

impl Writer {
    pub(crate) fn set_mtu(&mut self, value: u16) {
        self.mtu = value;
    }

    pub(crate) fn send(&self, buf: &[u8]) -> io::Result<()> {
        if self.offset == 0 {
            return self.fd.write(buf);
        }

        let header = packet_information(is_ipv6(buf)?);

        self.fd
            .writev(&[IoSlice::new(&header[..self.offset]), IoSlice::new(buf)])
    }

    pub(crate) fn send_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<()> {
        if self.offset == 0 {
            return self.fd.writev(bufs);
        }

        let mut bufs = bufs.iter().filter(|b| !b.is_empty()).copied().peekable();

        let Some(first) = bufs.peek() else {
            return Ok(());
        };
        let header = packet_information(is_ipv6(first)?);
        let header = IoSlice::new(&header[..self.offset]);

        let bufs = iter::once(header)
            .chain(bufs)
            .collect::<SmallVec<[IoSlice<'_>; 32]>>();

        self.fd.writev(&bufs)
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.send(buf).map(|_| buf.len())
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.send_vectored(bufs).map(|_| bufs.size())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsRawFd for Writer {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

pub struct Tun {
    pub(crate) reader: Reader,
    pub(crate) writer: Writer,
    pub(crate) fd: Arc<Fd>,
    pub(crate) mtu: u16,
    pub(crate) packet_information: bool,
}

impl Tun {
    pub(crate) fn new(fd: Fd, mtu: u16, packet_information: bool) -> Self {
        let fd = Arc::new(fd);
        let offset = if packet_information { PIL } else { 0 };
        Self {
            reader: Reader {
                fd: fd.clone(),
                offset,
                mtu,
            },
            writer: Writer {
                fd: fd.clone(),
                offset,
                mtu,
            },
            fd,
            mtu,
            packet_information,
        }
    }

    pub fn set_nonblock(&self) -> io::Result<()> {
        self.reader.fd.set_nonblock()
    }

    pub fn set_mtu(&mut self, value: u16) {
        self.mtu = value;
        self.reader.set_mtu(value);
        self.writer.set_mtu(value);
    }

    pub fn mtu(&self) -> u16 {
        self.mtu
    }

    pub fn packet_information(&self) -> bool {
        self.packet_information
    }

    delegate! {
        to self.reader {
            pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize>;
            pub fn recv_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize>;
        }
        to self.writer {
            pub fn send(&self, buf: &[u8]) -> io::Result<()>;
            pub fn send_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<()>;
        }
    }
}

impl Read for Tun {
    delegate! {
        to self.reader {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
            fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize>;
        }
    }
}

impl Write for Tun {
    delegate! {
        to self.writer {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize>;
            fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize>;
            fn flush(&mut self) -> io::Result<()>;
        }
    }
}

impl AsRawFd for Tun {
    delegate! {
        to self.fd {
            fn as_raw_fd(&self) -> RawFd;
        }
    }
}

impl IntoRawFd for Tun {
    fn into_raw_fd(self) -> RawFd {
        let Self { fd, .. } = self;
        // guarantee fd is the unique owner such that Arc::into_inner can return some
        Arc::into_inner(fd).unwrap().into_raw_fd() // panic if accident
    }
}
