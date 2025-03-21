// Copyright 2016-2024 Mullvad VPN AB. All Rights Reserved.
// Copyright 2024 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{
    ffi::OsString,
    io,
    net::IpAddr,
    os::windows::prelude::OsStringExt,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};
use windows::Win32::{Foundation::MAX_PATH, System::SystemInformation::GetSystemDirectoryW};

use nym_common::ErrorExt;
use nym_windows::net::{index_from_luid, luid_from_alias};

use crate::{DnsMonitorT, ResolvedDnsConfig};

const NETSH_TIMEOUT: Duration = Duration::from_secs(10);

/// Errors that can happen when configuring DNS on Windows.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Failure to obtain an interface LUID given an alias.
    #[error("Failed to obtain LUID for the interface alias")]
    ObtainInterfaceLuid(#[source] io::Error),

    /// Failure to obtain an interface index.
    #[error("Failed to obtain index of the interface")]
    ObtainInterfaceIndex(#[source] io::Error),

    /// Failure to spawn netsh subprocess.
    #[error("Failed to spawn 'netsh'")]
    SpawnNetsh(#[source] io::Error),

    /// Failure to spawn netsh subprocess.
    #[error("Failed to obtain system directory")]
    GetSystemDir(#[source] windows::core::Error),

    /// Failure to write to stdin.
    #[error("Failed to write to stdin for 'netsh'")]
    NetshInput(#[source] io::Error),

    /// Failure to wait for netsh result.
    #[error("Failed to wait for 'netsh'")]
    WaitNetsh(#[source] io::Error),

    /// netsh returned a non-zero status.
    #[error("'netsh' returned an error: {0:?}")]
    Netsh(Option<i32>),

    /// netsh did not return in a timely manner.
    #[error("'netsh' took too long to complete")]
    NetshTimeout,
}

pub struct DnsMonitor {
    current_index: Option<u32>,
}

impl DnsMonitorT for DnsMonitor {
    type Error = Error;

    fn new() -> Result<Self, Error> {
        Ok(DnsMonitor {
            current_index: None,
        })
    }

    async fn set(&mut self, interface: &str, config: ResolvedDnsConfig) -> Result<(), Error> {
        let servers = config.tunnel_config();
        let interface_luid = luid_from_alias(interface).map_err(Error::ObtainInterfaceLuid)?;
        let interface_index =
            index_from_luid(&interface_luid).map_err(Error::ObtainInterfaceIndex)?;

        self.current_index = Some(interface_index);

        let mut added_ipv4_server = false;
        let mut added_ipv6_server = false;

        let mut netsh_input = String::new();

        for server in servers {
            let is_additional_server;

            if server.is_ipv4() {
                is_additional_server = added_ipv4_server;
                added_ipv4_server = true;
            } else {
                is_additional_server = added_ipv6_server;
                added_ipv6_server = true;
            };

            if is_additional_server {
                netsh_input.push_str(&create_netsh_add_command(interface_index, server));
            } else {
                netsh_input.push_str(&create_netsh_set_command(interface_index, server));
            }
        }

        if !added_ipv4_server {
            netsh_input.push_str(&create_netsh_flush_command(interface_index, IpVersion::V4));
        }
        if !added_ipv6_server {
            netsh_input.push_str(&create_netsh_flush_command(interface_index, IpVersion::V6));
        }

        run_netsh_with_timeout(netsh_input, NETSH_TIMEOUT).await?;

        Ok(())
    }

    async fn reset(&mut self) -> Result<(), Error> {
        if let Some(index) = self.current_index.take() {
            let mut netsh_input = String::new();
            netsh_input.push_str(&create_netsh_flush_command(index, IpVersion::V4));
            netsh_input.push_str(&create_netsh_flush_command(index, IpVersion::V6));

            if let Err(error) = run_netsh_with_timeout(netsh_input, NETSH_TIMEOUT).await {
                tracing::error!("{}", error.display_chain_with_msg("Failed to reset DNS"));
            }
        }
        Ok(())
    }

    async fn reset_before_interface_removal(&mut self) -> Result<(), Self::Error> {
        // do nothing since the tunnel interface goes away
        let _ = self.current_index.take();
        Ok(())
    }
}

async fn run_netsh_with_timeout(netsh_input: String, timeout: Duration) -> Result<(), Error> {
    tracing::debug!("running netsh:\n{}", netsh_input);

    let sysdir = get_system_dir().map_err(Error::GetSystemDir)?;
    let mut netsh = Command::new(sysdir.join(r"netsh.exe"));

    let mut subproc = netsh
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(Error::SpawnNetsh)?;

    let mut stdin = subproc.stdin.take().unwrap();
    stdin
        .write_all(netsh_input.as_bytes())
        .await
        .map_err(Error::NetshInput)?;
    drop(stdin);

    match wait_for_child(&mut subproc, timeout).await {
        Ok(Some(status)) => {
            if !status.success() {
                return Err(Error::Netsh(status.code()));
            }
            Ok(())
        }
        Ok(None) => {
            let _ = subproc.kill().await;
            Err(Error::NetshTimeout)
        }
        Err(error) => Err(Error::WaitNetsh(error)),
    }
}

async fn wait_for_child(subproc: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    match tokio::time::timeout(timeout, subproc.wait()).await {
        Ok(result) => result.map(Some),
        Err(_elapsed) => Ok(None),
    }
}

fn create_netsh_set_command(interface_index: u32, server: &IpAddr) -> String {
    // Set primary DNS server:
    // netsh interface ipv4 set dnsservers name="Mullvad" source=static address=10.64.0.1
    // validate=no

    let interface_type = if server.is_ipv4() { "ipv4" } else { "ipv6" };
    format!("interface {interface_type} set dnsservers name={interface_index} source=static address={server} validate=no\r\n")
}

fn create_netsh_add_command(interface_index: u32, server: &IpAddr) -> String {
    // Add DNS server:
    // netsh interface ipv4 add dnsservers name="Mullvad" address=10.64.0.2 validate=no

    let interface_type = if server.is_ipv4() { "ipv4" } else { "ipv6" };
    format!("interface {interface_type} add dnsservers name={interface_index} address={server} validate=no\r\n")
}

fn create_netsh_flush_command(interface_index: u32, ip_version: IpVersion) -> String {
    // Flush DNS settings:
    // netsh interface ipv4 set dnsservers name="Mullvad" source=static address=none validate=no

    let interface_type = match ip_version {
        IpVersion::V4 => "ipv4",
        IpVersion::V6 => "ipv6",
    };

    format!("interface {interface_type} set dnsservers name={interface_index} source=static address=none validate=no\r\n")
}

fn get_system_dir() -> windows::core::Result<PathBuf> {
    let mut sysdir = [0u16; MAX_PATH as usize];
    let len = unsafe { GetSystemDirectoryW(Some(&mut sysdir)) };
    if len == 0 {
        Err(windows::core::Error::from_win32())
    } else {
        Ok(PathBuf::from(OsString::from_wide(
            &sysdir[0..(len as usize)],
        )))
    }
}

/// IP protocol version.
enum IpVersion {
    V4,
    V6,
}
