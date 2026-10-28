use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;

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
}
