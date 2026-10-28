use std::{
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn device_instance_id(guid: u128) -> String {
    let guid = windows_sys::core::GUID::from_u128(guid);
    format!(
        "SWD\\Wintun\\{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn locate_device(instance_id: &str) -> crate::error::Result<Option<(u32, u32, u32)>> {
    let instance_id = wide(instance_id);
    let mut dev_inst = 0;
    let result = unsafe {
        CM_Locate_DevNodeW(
            &mut dev_inst,
            instance_id.as_ptr(),
            CM_LOCATE_DEVNODE_PHANTOM,
        )
    };
    match result {
        CR_SUCCESS => {}
        CR_NO_SUCH_DEVNODE => return Ok(None),
        _ => return Err(format!("CM_Locate_DevNodeW failed with CONFIGRET {result}").into()),
    }

    let mut status = 0;
    let mut problem = 0;
    let result = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, dev_inst, 0) };
    match result {
        CR_SUCCESS => {}
        CR_NO_SUCH_DEVNODE => {
            let mut present_dev_inst = 0;
            let present_result =
                unsafe { CM_Locate_DevNodeW(&mut present_dev_inst, instance_id.as_ptr(), 0) };
            match present_result {
                CR_SUCCESS => return Ok(Some((present_dev_inst, 0, 0))),
                CR_NO_SUCH_DEVNODE => {
                    return Ok(Some((dev_inst, DN_HAS_PROBLEM, CM_PROB_PHANTOM)));
                }
                _ => {
                    return Err(format!(
                        "CM_Locate_DevNodeW failed with CONFIGRET {present_result}"
                    )
                    .into());
                }
            }
        }
        _ => return Err(format!("CM_Get_DevNode_Status failed with CONFIGRET {result}").into()),
    }
    Ok(Some((dev_inst, status, problem)))
}

pub(super) fn wait_for_stale_device(guid: u128) -> crate::error::Result<()> {
    let instance_id = device_instance_id(guid);
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    loop {
        if locate_device(&instance_id)?.is_none() {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for Wintun device {instance_id}").into());
        }
        thread::sleep(POLL_INTERVAL.min(remaining));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_requested_identity() {
        assert_eq!(
            device_instance_id(0xCFBAE5795EC3512AB6BBACE8DE243C2E),
            "SWD\\Wintun\\{CFBAE579-5EC3-512A-B6BB-ACE8DE243C2E}"
        );
    }

    #[test]
    fn missing_requested_identity_is_not_an_error() {
        wait_for_stale_device(u128::MAX).unwrap();
    }
}
