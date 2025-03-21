// Copyright 2016-2024 Mullvad VPN AB. All Rights Reserved.
// Copyright 2024 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{env, fmt};

use super::{DnsMonitorT, ResolvedDnsConfig};

mod auto;
mod dnsapi;
mod iphlpapi;
mod netsh;
mod tcpip;

/// Errors that can happen when configuring DNS on Windows.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Failed to set DNS config using the iphlpapi module.
    #[error("Error in iphlpapi module")]
    Iphlpapi(#[from] iphlpapi::Error),

    /// Failed to set DNS config using the netsh module.
    #[error("Error in netsh module")]
    Netsh(#[from] netsh::Error),

    /// Failed to set DNS config using the tcpip module.
    #[error("Error in tcpip module")]
    Tcpip(#[from] tcpip::Error),
}

pub struct DnsMonitor {
    inner: DnsMonitorHolder,
}

impl DnsMonitorT for DnsMonitor {
    type Error = Error;

    fn new() -> Result<Self, Error> {
        let dns_module = env::var_os("NYM_DNS_MODULE");

        let inner = match dns_module.as_ref().and_then(|value| value.to_str()) {
            Some("iphlpapi") => DnsMonitorHolder::Iphlpapi(iphlpapi::DnsMonitor::new()?),
            Some("tcpip") => DnsMonitorHolder::Tcpip(tcpip::DnsMonitor::new()?),
            Some("netsh") => DnsMonitorHolder::Netsh(netsh::DnsMonitor::new()?),
            Some(_) | None => DnsMonitorHolder::Auto(auto::DnsMonitor::new()?),
        };

        tracing::debug!("DNS monitor: {}", inner);

        Ok(DnsMonitor { inner })
    }

    async fn set(&mut self, interface: &str, config: ResolvedDnsConfig) -> Result<(), Error> {
        match self.inner {
            DnsMonitorHolder::Auto(ref mut inner) => inner.set(interface, config).await?,
            DnsMonitorHolder::Iphlpapi(ref mut inner) => inner.set(interface, config).await?,
            DnsMonitorHolder::Netsh(ref mut inner) => inner.set(interface, config).await?,
            DnsMonitorHolder::Tcpip(ref mut inner) => inner.set(interface, config).await?,
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<(), Error> {
        match self.inner {
            DnsMonitorHolder::Auto(ref mut inner) => inner.reset().await?,
            DnsMonitorHolder::Iphlpapi(ref mut inner) => inner.reset().await?,
            DnsMonitorHolder::Netsh(ref mut inner) => inner.reset().await?,
            DnsMonitorHolder::Tcpip(ref mut inner) => inner.reset().await?,
        }
        Ok(())
    }

    async fn reset_before_interface_removal(&mut self) -> Result<(), Error> {
        match self.inner {
            DnsMonitorHolder::Auto(ref mut inner) => inner.reset_before_interface_removal().await?,
            DnsMonitorHolder::Iphlpapi(ref mut inner) => {
                inner.reset_before_interface_removal().await?
            }
            DnsMonitorHolder::Netsh(ref mut inner) => {
                inner.reset_before_interface_removal().await?
            }
            DnsMonitorHolder::Tcpip(ref mut inner) => {
                inner.reset_before_interface_removal().await?
            }
        }
        Ok(())
    }
}

enum DnsMonitorHolder {
    Auto(auto::DnsMonitor),
    Iphlpapi(iphlpapi::DnsMonitor),
    Netsh(netsh::DnsMonitor),
    Tcpip(tcpip::DnsMonitor),
}

impl fmt::Display for DnsMonitorHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsMonitorHolder::Auto(_) => f.write_str("auto (iphlpapi > netsh > tcpip)"),
            DnsMonitorHolder::Iphlpapi(_) => f.write_str("SetInterfaceDnsSettings (iphlpapi)"),
            DnsMonitorHolder::Netsh(_) => f.write_str("netsh"),
            DnsMonitorHolder::Tcpip(_) => f.write_str("TCP/IP registry parameter"),
        }
    }
}
