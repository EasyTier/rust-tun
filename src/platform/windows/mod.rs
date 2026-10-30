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

//! Windows specific functionality.

mod device;
mod device_cleanup;
mod verify_dll_file;

use crate::configuration::Configuration;
use crate::error::Result;
pub use device::{Device, Tun};
use std::ffi::OsString;
use std::net::IpAddr;

pub(crate) const WINTUN_PROVIDER: &str = "WireGuard LLC";

fn supports_requested_guid() -> Result<bool> {
    use windows_sys::{
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
    };

    let mut version: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    version.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    let status = unsafe { RtlGetVersion(&mut version) };
    if status != 0 {
        return Err(format!("RtlGetVersion failed with NTSTATUS {status:#x}").into());
    }
    Ok(version.dwMajorVersion >= 10)
}

/// Windows-only interface configuration.
#[derive(Clone, Debug)]
pub struct PlatformConfig {
    pub(crate) device_guid: Option<u128>,
    pub(crate) wintun_file: OsString,
    #[cfg(feature = "wintun-dns")]
    pub(crate) dns_servers: Option<Vec<IpAddr>>,
    pub(crate) ring_cap: Option<u32>,
    pub(crate) skip_config: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            device_guid: None,
            wintun_file: "wintun.dll".into(),
            #[cfg(feature = "wintun-dns")]
            dns_servers: None,
            ring_cap: None,
            skip_config: false,
        }
    }
}

impl PlatformConfig {
    /// Request a fixed interface GUID on Windows 10 and later.
    /// Wintun chooses the interface GUID on older Windows versions.
    pub fn device_guid(&mut self, device_guid: u128) {
        log::trace!("Windows configuration device GUID");
        self.device_guid = Some(device_guid);
    }

    /// Use a custom path to the wintun.dll instead of looking in the working directory.
    /// Security note: It is up to the caller to ensure that the library can be safely loaded from
    /// the indicated path.
    ///
    /// [`wintun_file`](PlatformConfig::wintun_file) likes "path/to/wintun" or "path/to/wintun.dll".
    pub fn wintun_file<S: Into<OsString>>(&mut self, wintun_file: S) {
        self.wintun_file = wintun_file.into();
    }

    #[cfg(feature = "wintun-dns")]
    pub fn dns_servers(&mut self, dns_servers: &[IpAddr]) {
        self.dns_servers = Some(dns_servers.to_vec());
    }

    pub fn min_ring_cap(&self) -> u32 {
        wintun::MIN_RING_CAPACITY
    }

    pub fn max_ring_cap(&self) -> u32 {
        wintun::MAX_RING_CAPACITY
    }

    pub fn ring_cap(&mut self, ring_cap: Option<u32>) {
        self.ring_cap = ring_cap;
    }

    pub fn skip_config(&mut self, skip_config: bool) {
        self.skip_config = skip_config;
    }
}

/// Create a TUN device with the given name.
pub fn create(configuration: &Configuration) -> Result<Device> {
    Device::new(configuration)
}
