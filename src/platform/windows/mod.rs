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

pub use device::{Device, Queue};

use crate::configuration::Configuration as C;
use crate::error::*;

/// Windows-only interface configuration.
#[derive(Copy, Clone, Default, Debug)]
pub struct Configuration {
    skip_config: bool,
    guid: Option<u128>,
    ring_cap: Option<u32>,
}

impl Configuration {
    pub fn initialize(&mut self) {
        log::trace!("Windows configuration initialize");
    }

    pub fn skip_config(&mut self, skip_config: bool) {
        self.skip_config = skip_config;
    }

    pub fn guid(&mut self, guid: Option<u128>) {
        self.guid = guid;
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
}

/// Create a TUN device with the given name.
pub fn create(configuration: &C) -> Result<Device> {
    Device::new(configuration)
}
