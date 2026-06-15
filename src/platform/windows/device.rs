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

use derive_more::{Deref, DerefMut};
use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_HANDLE_EOF};
use wintun::Session;

use crate::configuration::Configuration;
use crate::device::AbstractDevice;
use crate::error::{Error, Result};
use crate::platform::windows::verify_dll_file::{
    get_dll_absolute_path, get_signer_name, verify_embedded_signature,
};
use crate::utils::{DataExt, SliceExt};

/// A TUN device using the wintun driver.
#[derive(Deref, DerefMut)]
pub struct Device {
    #[deref]
    #[deref_mut]
    pub(crate) tun: Tun,
    mtu: u16,
    name: String,
}

impl Device {
    /// Create a new `Device` for the given `Configuration`.
    pub fn new(config: &Configuration) -> Result<Self> {
        let wintun_file = &config.platform_config.wintun_file;
        let wintun = unsafe {
            // Ensure the dll file has not been tampered with.
            let abs_path = get_dll_absolute_path(wintun_file)?;
            verify_embedded_signature(&abs_path)?;
            let signer_name = get_signer_name(&abs_path)?;
            let wp = super::WINTUN_PROVIDER;
            if signer_name != wp {
                return Err(format!("Signer \"{}\" not match \"{}\"", signer_name, wp).into());
            }

            let wintun = libloading::Library::new(wintun_file)?;
            wintun::load_from_library(wintun)?
        };
        let tun_name = config.tun_name.as_deref().unwrap_or("wintun");
        let guid = config.platform_config.device_guid;
        let adapter = match wintun::Adapter::open(&wintun, tun_name) {
            Ok(a) => a,
            Err(_) => wintun::Adapter::create(&wintun, tun_name, tun_name, guid)?,
        };

        // on win7 guid will not be correctly assigned, user should skip the config step.
        if !config.platform_config.skip_config && adapter.get_name().is_ok() {
            let address = config
                .address
                .unwrap_or(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 2)));
            let mask = config
                .netmask
                .unwrap_or(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 0)));
            adapter.set_network_addresses_tuple(address, mask, config.destination)?;
            #[cfg(feature = "wintun-dns")]
            if let Some(dns_servers) = &config.platform_config.dns_servers {
                adapter.set_dns_servers(dns_servers)?;
            }
        }

        let mtu = config.mtu.unwrap_or(crate::DEFAULT_MTU);

        let session = adapter.start_session(
            config
                .platform_config
                .ring_cap
                .unwrap_or(wintun::MAX_RING_CAPACITY),
        )?;

        let mut device = Device {
            tun: Tun {
                session: Arc::new(session),
            },
            mtu,
            name: tun_name.to_string(),
        };

        if !config.platform_config.skip_config && adapter.get_name().is_ok() {
            // This is not needed since we use netsh to set the address.
            device.configure(config)?;
        }

        Ok(device)
    }

    pub fn split(self) -> (Reader, Writer) {
        let tun = Arc::new(self.tun);
        (Reader(tun.clone()), Writer(tun.clone()))
    }
}

impl Read for Device {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tun.read(buf)
    }
}

impl Write for Device {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tun.write(buf)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.tun.write_vectored(bufs)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tun.flush()
    }
}

impl AsRef<dyn AbstractDevice + 'static> for Device {
    fn as_ref(&self) -> &(dyn AbstractDevice + 'static) {
        self
    }
}

impl AsMut<dyn AbstractDevice + 'static> for Device {
    fn as_mut(&mut self) -> &mut (dyn AbstractDevice + 'static) {
        self
    }
}

impl AbstractDevice for Device {
    fn tun_name(&self) -> Result<String> {
        match self.tun.session.get_adapter().get_name() {
            Ok(name) => Ok(name),
            Err(_) => Ok(self.name.clone()),
        }
    }

    fn set_tun_name(&mut self, value: &str) -> Result<()> {
        self.tun.session.get_adapter().set_name(value)?;
        self.name = value.to_string();
        Ok(())
    }

    fn enabled(&mut self, _value: bool) -> Result<()> {
        Ok(())
    }

    fn address(&self) -> Result<IpAddr> {
        let addresses = self.tun.session.get_adapter().get_addresses()?;
        addresses
            .iter()
            .find_map(|a| match a {
                std::net::IpAddr::V4(a) => Some(std::net::IpAddr::V4(*a)),
                _ => None,
            })
            .ok_or(Error::InvalidConfig)
    }

    fn set_address(&mut self, value: IpAddr) -> Result<()> {
        let IpAddr::V4(value) = value else {
            unimplemented!("do not support IPv6 yet")
        };
        self.tun.session.get_adapter().set_address(value)?;
        Ok(())
    }

    fn destination(&self) -> Result<IpAddr> {
        // It's just the default gateway in windows.
        self.tun
            .session
            .get_adapter()
            .get_gateways()?
            .iter()
            .find_map(|a| match a {
                std::net::IpAddr::V4(a) => Some(std::net::IpAddr::V4(*a)),
                _ => None,
            })
            .ok_or(Error::InvalidConfig)
    }

    fn set_destination(&mut self, value: IpAddr) -> Result<()> {
        let IpAddr::V4(value) = value else {
            unimplemented!("do not support IPv6 yet")
        };
        // It's just set the default gateway in windows.
        self.tun.session.get_adapter().set_gateway(Some(value))?;
        Ok(())
    }

    fn broadcast(&self) -> Result<IpAddr> {
        Err(Error::NotImplemented)
    }

    fn set_broadcast(&mut self, value: IpAddr) -> Result<()> {
        log::debug!("set_broadcast {} is not need", value);
        Ok(())
    }

    fn netmask(&self) -> Result<IpAddr> {
        let current_addr = self.address()?;
        self.tun
            .session
            .get_adapter()
            .get_netmask_of_address(&current_addr)
            .map_err(Error::WintunError)
    }

    fn set_netmask(&mut self, value: IpAddr) -> Result<()> {
        let IpAddr::V4(value) = value else {
            unimplemented!("do not support IPv6 yet")
        };
        self.tun.session.get_adapter().set_netmask(value)?;
        Ok(())
    }

    /// The return value is always `Ok(65535)` due to wintun
    fn mtu(&self) -> Result<u16> {
        // Note: wintun mtu is always 65535
        Ok(self.mtu)
    }

    /// This setting has no effect since the mtu of wintun is always 65535
    fn set_mtu(&mut self, _: u16) -> Result<()> {
        // Note: no-op due to mtu of wintun is always 65535
        Ok(())
    }

    fn packet_information(&self) -> bool {
        // Note: wintun does not support packet information
        false
    }
}

pub struct Tun {
    session: Arc<Session>,
}

impl Tun {
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.session.receive_blocking() {
            Ok(data) => Ok(data.bytes().put(buf)),
            Err(e) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, e)),
        }
    }

    pub fn recv_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        match self.session.receive_blocking() {
            Ok(data) => Ok(data.bytes().putv(bufs)),
            Err(e) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, e)),
        }
    }

    fn with_packet(&self, size: usize, f: impl FnOnce(wintun::Packet)) -> io::Result<()> {
        match self.session.allocate_send_packet(size as u16) {
            Ok(packet) => {
                f(packet);
                Ok(())
            }
            Err(wintun::Error::Io(e)) => match e.raw_os_error() {
                Some(code) if code == ERROR_BUFFER_OVERFLOW as i32 => Ok(()),
                Some(code) if code == ERROR_HANDLE_EOF as i32 => {
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, e))
                }
                _ => Err(e),
            },
            Err(e) => Err(io::Error::other(e)),
        }
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<()> {
        self.with_packet(buf.len(), |mut packet| {
            packet.bytes_mut().copy_from_slice(buf);
            self.session.send_packet(packet);
        })
    }

    pub fn send_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<()> {
        let size = bufs.size();
        if size == 0 {
            return Ok(());
        }
        self.with_packet(size, |mut packet| {
            let dst = packet.bytes_mut();
            let mut offset = 0;
            for buf in bufs {
                if !buf.is_empty() {
                    let end = offset + buf.len();
                    dst[offset..end].copy_from_slice(buf);
                    offset = end;
                }
            }
            self.session.send_packet(packet);
        })
    }
}

impl Read for Tun {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.recv(buf)
    }
}

impl Write for Tun {
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

impl Drop for Tun {
    fn drop(&mut self) {
        if let Err(err) = self.session.shutdown() {
            log::error!("failed to shutdown session: {:?}", err);
        }
    }
}

#[repr(transparent)]
pub struct Reader(Arc<Tun>);

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.recv(buf)
    }
}

#[repr(transparent)]
pub struct Writer(Arc<Tun>);

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf).map(|_| buf.len())
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        self.0.send_vectored(bufs).map(|_| bufs.size())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
