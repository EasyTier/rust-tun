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

use bytes::BytesMut;
use packet::{ip::Packet, Error};
use tokio::sync::mpsc::Receiver;
use tokio_util::codec::Decoder;
use tun_easytier::{AbstractDevice, AsyncReadExt, BoxError};

pub struct IPPacketCodec;

impl Decoder for IPPacketCodec {
    type Item = Packet<BytesMut>;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if buf.is_empty() {
            return Ok(None);
        }

        let buf = buf.split_to(buf.len());
        Ok(match Packet::no_payload(buf) {
            Ok(pkt) => Some(pkt),
            Err(err) => {
                println!("error {:?}", err);
                None
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    let (tx, rx) = tokio::sync::mpsc::channel::<()>(1);

    ctrlc2::set_async_handler(async move {
        tx.send(()).await.expect("Signal error");
    })
    .await;

    main_entry(rx).await?;
    Ok(())
}

async fn main_entry(mut quit: Receiver<()>) -> Result<(), BoxError> {
    let mut config = tun_easytier::Configuration::default();

    config
        .address((10, 0, 0, 9))
        .netmask((255, 255, 255, 0))
        .destination((10, 0, 0, 1))
        .up();

    #[cfg(target_os = "linux")]
    config.platform_config(|config| {
        #[allow(deprecated)]
        config.packet_information(true);
        config.ensure_root_privileges(true);
    });

    #[cfg(target_os = "windows")]
    config.platform_config(|config| {
        config.device_guid(9099482345783245345345_u128);
    });

    let dev = tun_easytier::create_as_async(&config)?;
    let size = dev.mtu().unwrap_or(65535) as usize + tun_easytier::PACKET_INFORMATION_LENGTH;
    let (mut reader, _) = dev.split();
    let mut codec = IPPacketCodec;
    let mut buf = vec![0; size];

    loop {
        tokio::select! {
            _ = quit.recv() => {
                println!("Quit...");
                break;
            }
            len = reader.read(&mut buf) => {
                let mut bytes = BytesMut::from(&buf[..len?]);
                match codec.decode(&mut bytes) {
                    Ok(Some(pkt)) => println!("pkt: {:#?}", pkt),
                    Ok(None) => {},
                    Err(err) => panic!("Error: {:?}", err),
                }
            }
        };
    }
    Ok(())
}
