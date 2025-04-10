// Copyright 2016-2024 Mullvad VPN AB. All Rights Reserved.
// Copyright 2024 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

//! DNS monitor that uses `SetInterfaceDnsSettings`. According to
//! <https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-setinterfacednssettings>,
//! it requires at least Windows 10, build 19041. For that reason, use run-time linking and fall
//! back on other methods if it is not available.

use crate::{DnsMonitorT, ResolvedDnsConfig};
use nym_windows::net::{guid_from_luid, luid_from_alias};
use once_cell::sync::OnceCell;
use std::{
    ffi::OsString,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::windows::ffi::OsStrExt,
};
use windows::{
    core::{s, w, GUID, PWSTR},
    Win32::{
        Foundation::{FreeLibrary, ERROR_PROC_NOT_FOUND, WIN32_ERROR},
        NetworkManagement::IpHelper::{
            DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_IPV6,
            DNS_SETTING_NAMESERVER,
        },
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
    },
};

/// Errors that can happen when configuring DNS on Windows.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Failure to obtain an interface LUID given an alias.
    #[error("Failed to obtain LUID for the interface alias")]
    ObtainInterfaceLuid(#[source] io::Error),

    /// Failure to obtain an interface GUID.
    #[error("Failed to obtain GUID for the interface")]
    ObtainInterfaceGuid(#[source] io::Error),

    /// Failed to set DNS settings on interface.
    #[error("Failed to set DNS settings on interface")]
    SetInterfaceDnsSettings(#[source] windows::core::Error),

    /// Failure to flush DNS cache.
    #[error("Failed to flush DNS resolver cache")]
    FlushResolverCache(#[source] super::dnsapi::Error),

    /// Failed to load iphlpapi.dll.
    #[error("Failed to load iphlpapi.dll")]
    LoadDll(#[source] windows::core::Error),

    /// Failed to obtain exported function.
    #[error("Failed to obtain DNS function")]
    GetFunction(#[source] windows::core::Error),
}

type SetInterfaceDnsSettingsFn = unsafe extern "stdcall" fn(
    interface: GUID,
    settings: *const DNS_INTERFACE_SETTINGS,
) -> WIN32_ERROR;

struct IphlpApi {
    set_interface_dns_settings: SetInterfaceDnsSettingsFn,
}

unsafe impl Send for IphlpApi {}
unsafe impl Sync for IphlpApi {}

static IPHLPAPI_HANDLE: OnceCell<IphlpApi> = OnceCell::new();

impl IphlpApi {
    fn new() -> Result<Self, Error> {
        let module =
            unsafe { LoadLibraryExW(w!("iphlpapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| {
                    tracing::error!("Failed to load iphlpapi.dll: {}", e);
                    Error::LoadDll(e)
                })?;

        // This function is loaded at runtime since it may be unavailable. See the module-level
        // docs. TODO: `windows_sys` can be used directly when support for versions older
        // than Windows 10, 2004, is dropped.
        let set_interface_dns_settings =
            unsafe { GetProcAddress(module, s!("SetInterfaceDnsSettings")) };
        let set_interface_dns_settings = set_interface_dns_settings.ok_or_else(|| {
            let error = windows::core::Error::from_win32();

            if error.code() != ERROR_PROC_NOT_FOUND.to_hresult() {
                tracing::error!(
                    "Could not find SetInterfaceDnsSettings due to an unexpected error: {error}"
                );
            }

            if let Err(e) = unsafe { FreeLibrary(module) } {
                tracing::error!("Failed to free library iphlpapi.dll: {}", e);
            }
            Error::GetFunction(error)
        })?;

        // NOTE: Leaking `module` here, since we're lazily initializing it

        Ok(Self {
            set_interface_dns_settings: unsafe {
                *(&set_interface_dns_settings as *const _ as *const _)
            },
        })
    }
}

pub struct DnsMonitor {
    current_guid: Option<GUID>,
}

impl DnsMonitor {
    pub fn is_supported() -> bool {
        IPHLPAPI_HANDLE.get_or_try_init(IphlpApi::new).is_ok()
    }
}

impl DnsMonitorT for DnsMonitor {
    type Error = Error;

    fn new() -> Result<Self, Error> {
        Ok(DnsMonitor { current_guid: None })
    }

    async fn set(&mut self, interface: &str, config: ResolvedDnsConfig) -> Result<(), Error> {
        let servers = config.tunnel_config();
        let guid = guid_from_luid(&luid_from_alias(interface).map_err(Error::ObtainInterfaceLuid)?)
            .map_err(Error::ObtainInterfaceGuid)?;

        let mut v4_servers = vec![];
        let mut v6_servers = vec![];

        for server in servers {
            match server {
                IpAddr::V4(addr) => v4_servers.push(addr),
                IpAddr::V6(addr) => v6_servers.push(addr),
            }
        }

        self.current_guid = Some(guid);

        if !v4_servers.is_empty() {
            set_interface_dns_servers_v4(&guid, &v4_servers)?;
        }
        if !v6_servers.is_empty() {
            set_interface_dns_servers_v6(&guid, &v6_servers)?;
        }

        flush_dns_cache()?;

        Ok(())
    }

    async fn reset(&mut self) -> Result<(), Error> {
        if let Some(guid) = self.current_guid.take() {
            set_interface_dns_servers_v4(&guid, &[])
                .and(set_interface_dns_servers_v6(&guid, &[]))
                .and(flush_dns_cache())?;
        }
        Ok(())
    }

    async fn reset_before_interface_removal(&mut self) -> Result<(), Self::Error> {
        // do nothing since the tunnel interface goes away
        let _ = self.current_guid.take();
        Ok(())
    }
}

fn set_interface_dns_servers_v4(guid: &GUID, servers: &[&Ipv4Addr]) -> Result<(), Error> {
    set_interface_dns_servers(guid, servers, DNS_SETTING_NAMESERVER)
}

fn set_interface_dns_servers_v6(guid: &GUID, servers: &[&Ipv6Addr]) -> Result<(), Error> {
    set_interface_dns_servers(guid, servers, DNS_SETTING_NAMESERVER | DNS_SETTING_IPV6)
}

fn set_interface_dns_servers<T: ToString>(
    guid: &GUID,
    servers: &[T],
    flags: u32,
) -> Result<(), Error> {
    let iphlpapi = IPHLPAPI_HANDLE.get_or_try_init(IphlpApi::new)?;

    // Create comma-separated nameserver list
    let nameservers = servers
        .iter()
        .map(|addr| addr.to_string())
        .collect::<Vec<String>>()
        .join(",");
    let mut nameservers: Vec<u16> = OsString::from(nameservers)
        .encode_wide()
        .chain(std::iter::once(0u16))
        .collect();

    let dns_interface_settings = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: u64::from(flags),
        Domain: PWSTR::null(),
        NameServer: PWSTR::from_raw(nameservers.as_mut_ptr()),
        SearchList: PWSTR::null(),
        RegistrationEnabled: 0,
        RegisterAdapterName: 0,
        EnableLLMNR: 0,
        QueryAdapterName: 0,
        ProfileNameServer: PWSTR::null(),
    };

    unsafe { (iphlpapi.set_interface_dns_settings)(guid.to_owned(), &dns_interface_settings) }
        .ok()
        .map_err(Error::SetInterfaceDnsSettings)
}

fn flush_dns_cache() -> Result<(), Error> {
    super::dnsapi::flush_resolver_cache().map_err(Error::FlushResolverCache)
}
