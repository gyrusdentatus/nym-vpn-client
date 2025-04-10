// Copyright 2016-2025 Mullvad VPN AB. All Rights Reserved.
// Copyright 2025 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::mem::{self, MaybeUninit};

use windows::{
    core::{s, w},
    Win32::System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        SystemInformation::OSVERSIONINFOEXW,
        SystemServices::VER_NT_WORKSTATION,
    },
};

#[allow(non_camel_case_types)]
type RTL_OSVERSIONINFOEXW = OSVERSIONINFOEXW;

pub fn version() -> String {
    let (major, build) = WindowsVersion::new()
        .map(|version_info| {
            (
                version_info.windows_version_string(),
                version_info.build_number().to_string(),
            )
        })
        .unwrap_or_else(|_| ("N/A".to_owned(), "N/A".to_owned()));

    format!("Windows {} Build {}", major, build)
}

pub fn short_version() -> String {
    let version_string = WindowsVersion::new()
        .map(|version| version.windows_version_string())
        .unwrap_or("N/A".into());
    format!("Windows {}", version_string)
}

pub fn extra_metadata() -> impl Iterator<Item = (String, String)> {
    std::iter::empty()
}

pub struct WindowsVersion {
    inner: RTL_OSVERSIONINFOEXW,
}

impl WindowsVersion {
    pub fn new() -> Result<WindowsVersion, windows::core::Error> {
        let ntdll = unsafe { GetModuleHandleW(w!("ntdll"))? };
        let function_address = unsafe { GetProcAddress(ntdll, s!("RtlGetVersion")) }
            .ok_or_else(windows::core::Error::from_win32)?;

        let rtl_get_version: extern "stdcall" fn(*mut RTL_OSVERSIONINFOEXW) =
            unsafe { *(&function_address as *const _ as *const _) };

        let mut version_info: MaybeUninit<RTL_OSVERSIONINFOEXW> = mem::MaybeUninit::zeroed();
        unsafe {
            (*version_info.as_mut_ptr()).dwOSVersionInfoSize =
                mem::size_of_val(&version_info) as u32;
            rtl_get_version(version_info.as_mut_ptr());

            Ok(WindowsVersion {
                inner: version_info.assume_init(),
            })
        }
    }

    pub fn windows_version_string(&self) -> String {
        // `wProductType != VER_NT_WORKSTATION` implies that OS is Windows Server
        // https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_osversioninfoexw
        // NOTE: This does not deduce which Windows Server version is running.
        if u32::from(self.inner.wProductType) != VER_NT_WORKSTATION {
            return "Server".to_owned();
        }

        match self.release_version() {
            (major, 0) => major.to_string(),
            (major, minor) => format!("{major}.{minor}"),
        }
    }

    /// Release version. E.g. `(10, 0)` for Windows 10, or `(8, 0)` for Windows 8.1.
    pub fn release_version(&self) -> (u32, u32) {
        // Check https://en.wikipedia.org/wiki/List_of_Microsoft_Windows_versions#Personal_computer_versions 'Release version' column
        // for the correct NT versions for specific windows releases.
        match (self.major_version(), self.minor_version()) {
            (6, 1) => (7, 0),
            (6, 2) => (8, 0),
            (6, 3) => (8, 1),
            (10, 0) => {
                if self.build_number() < 22000 {
                    (10, 0)
                } else {
                    (11, 0)
                }
            }
            (major, minor) => (major, minor),
        }
    }

    pub fn major_version(&self) -> u32 {
        self.inner.dwMajorVersion
    }

    pub fn minor_version(&self) -> u32 {
        self.inner.dwMinorVersion
    }

    pub fn build_number(&self) -> u32 {
        self.inner.dwBuildNumber
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_windows_version() {
        WindowsVersion::new().unwrap();
    }
}
