use std::{
    io,
    mem::size_of,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*,
    Foundation::{GetLastError, ERROR_IN_WOW64},
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, PartialEq, Eq)]
enum DeviceAction {
    Leave,
    Remove,
}

fn classify_device(status: u32, problem: u32) -> DeviceAction {
    if status & DN_HAS_PROBLEM != 0 && matches!(problem, CM_PROB_PHANTOM | CM_PROB_FAILED_ADD) {
        DeviceAction::Remove
    } else {
        DeviceAction::Leave
    }
}

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
                CR_SUCCESS => return Ok(None),
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

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

fn remove_device_wow64(instance_id: &str) -> crate::error::Result<()> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let pnputil = Path::new(&root).join("Sysnative").join("pnputil.exe");
    let status = std::process::Command::new(pnputil)
        .arg("/remove-device")
        .arg(instance_id)
        .status()?;
    if !status.success() {
        return Err(format!("pnputil /remove-device failed with {status}").into());
    }
    Ok(())
}

fn remove_device(instance_id: &str) -> crate::error::Result<()> {
    let instance_id_wide = wide(instance_id);
    let dev_info = unsafe {
        SetupDiCreateDeviceInfoListExW(
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if dev_info == -1 {
        return Err(io::Error::last_os_error().into());
    }
    let dev_info = DeviceInfoSet(dev_info);

    let mut data: SP_DEVINFO_DATA = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
    if unsafe {
        SetupDiOpenDeviceInfoW(
            dev_info.0,
            instance_id_wide.as_ptr(),
            std::ptr::null_mut(),
            DIOD_INHERIT_CLASSDRVS,
            &mut data,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }

    let mut params: SP_REMOVEDEVICE_PARAMS = unsafe { std::mem::zeroed() };
    params.ClassInstallHeader.cbSize = size_of::<SP_CLASSINSTALL_HEADER>() as u32;
    params.ClassInstallHeader.InstallFunction = DIF_REMOVE;
    params.Scope = DI_REMOVEDEVICE_GLOBAL;
    if unsafe {
        SetupDiSetClassInstallParamsW(
            dev_info.0,
            &data,
            &params.ClassInstallHeader,
            size_of::<SP_REMOVEDEVICE_PARAMS>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    if unsafe { SetupDiCallClassInstaller(DIF_REMOVE, dev_info.0, &data) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_IN_WOW64 {
            return remove_device_wow64(instance_id);
        }
        return Err(io::Error::from_raw_os_error(error as i32).into());
    }
    Ok(())
}

pub(super) fn cleanup_stale_device(guid: u128) -> crate::error::Result<bool> {
    let instance_id = device_instance_id(guid);
    let Some((_, status, problem)) = locate_device(&instance_id)? else {
        return Ok(false);
    };
    if classify_device(status, problem) != DeviceAction::Remove {
        return Ok(false);
    }

    let started = Instant::now();
    remove_device(&instance_id)?;
    let deadline = started + CLEANUP_TIMEOUT;
    loop {
        match locate_device(&instance_id)? {
            None => break,
            Some((_, status, _)) if status & DN_STARTED != 0 => break,
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out removing stale Wintun device {instance_id}").into());
        }
        thread::sleep(POLL_INTERVAL);
    }
    log::info!(
        "removed stale Wintun device {instance_id} in {:?}",
        started.elapsed()
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CM_PROB_NONE: u32 = 0;

    #[test]
    fn formats_the_requested_identity() {
        assert_eq!(
            device_instance_id(0xCFBAE5795EC3512AB6BBACE8DE243C2E),
            "SWD\\Wintun\\{CFBAE579-5EC3-512A-B6BB-ACE8DE243C2E}"
        );
    }

    #[test]
    fn removes_only_phantom_and_failed_add_nodes() {
        for problem in [CM_PROB_PHANTOM, CM_PROB_FAILED_ADD] {
            assert_eq!(
                classify_device(DN_HAS_PROBLEM, problem),
                DeviceAction::Remove
            );
        }
        for (status, problem) in [
            (0, CM_PROB_NONE),
            (DN_HAS_PROBLEM, CM_PROB_NEED_CLASS_CONFIG),
            (DN_HAS_PROBLEM, CM_PROB_REINSTALL),
        ] {
            assert_eq!(classify_device(status, problem), DeviceAction::Leave);
        }
    }

    #[test]
    fn missing_requested_identity_is_not_an_error() {
        assert_eq!(cleanup_stale_device(u128::MAX).unwrap(), false);
    }
}
