// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only
#![warn(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod cli;
mod config;
mod protobuf_conversion;
mod vpnd_client;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::Internal;
use itertools::Itertools;
use nym_gateway_directory::GatewayType;
use nym_vpn_lib_types::TunnelState;
use nym_vpn_proto::{
    ConfirmZkNymDownloadedRequest, ConnectRequest, GetAccountLinksRequest, GetZkNymByIdRequest,
    InfoResponse, ListCountriesRequest, ListGatewaysRequest, ResetDeviceIdentityRequest,
    SetNetworkRequest, StoreAccountRequest, UserAgent,
};
use protobuf_conversion::into_gateway_type;
use sysinfo::System;
use vpnd_client::ClientType;

use crate::{
    cli::Command,
    protobuf_conversion::{into_entry_point, into_exit_point},
};

#[derive(Clone, Debug)]
struct CliOptions {
    client_type: ClientType,
    verbose: bool,
    user_agent: Option<nym_http_api_client::UserAgent>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::CliArgs::parse();
    let client_type = if args.http {
        vpnd_client::ClientType::Http
    } else {
        vpnd_client::ClientType::Ipc
    };
    let opts = CliOptions {
        client_type,
        verbose: args.verbose,
        user_agent: args.user_agent,
    };

    match args.command {
        Command::Connect(ref connect_args) => connect(opts, connect_args).await?,
        Command::Disconnect { wait } => disconnect(opts, wait).await?,
        Command::Status { listen } => status(listen, opts).await?,
        Command::Info => info(opts.client_type).await?,
        Command::SetNetwork(ref args) => set_network(opts.client_type, args).await?,
        Command::StoreAccount(ref store_args) => store_account(opts, store_args).await?,
        Command::IsAccountStored => is_account_stored(opts.client_type).await?,
        Command::ForgetAccount => forget_account(opts.client_type).await?,
        Command::GetAccountId => get_account_id(opts.client_type).await?,
        Command::GetAccountLinks(ref args) => get_account_links(opts, args).await?,
        Command::GetAccountState => get_account_state(opts.client_type).await?,
        Command::ListEntryGateways(ref list_args) => {
            list_gateways(opts, list_args, GatewayType::MixnetEntry).await?
        }
        Command::ListExitGateways(ref list_args) => {
            list_gateways(opts, list_args, GatewayType::MixnetExit).await?
        }
        Command::ListVpnGateways(ref list_args) => {
            list_gateways(opts, list_args, GatewayType::Wg).await?
        }
        Command::ListEntryCountries(ref list_args) => {
            list_countries(opts, list_args, GatewayType::MixnetEntry).await?
        }
        Command::ListExitCountries(ref list_args) => {
            list_countries(opts, list_args, GatewayType::MixnetExit).await?
        }
        Command::ListVpnCountries(ref list_args) => {
            list_countries(opts, list_args, GatewayType::Wg).await?
        }
        Command::GetDeviceId => get_device_id(opts.client_type).await?,
        Command::Internal(internal) => match internal {
            Internal::GetSystemMessages => get_system_messages(opts.client_type).await?,
            Internal::GetFeatureFlags => get_feature_flags(opts.client_type).await?,
            Internal::SyncAccountState => refresh_account_state(opts.client_type).await?,
            Internal::GetAccountUsage => get_account_usage(opts.client_type).await?,
            Internal::ResetDeviceIdentity(ref args) => {
                reset_device_identity(opts.client_type, args).await?
            }
            Internal::RegisterDevice => register_device(opts.client_type).await?,
            Internal::GetDevices => get_devices(opts.client_type).await?,
            Internal::GetActiveDevices => get_active_devices(opts.client_type).await?,
            Internal::RequestZkNym => request_zk_nym(opts.client_type).await?,
            Internal::GetDeviceZkNym => get_device_zk_nym(opts.client_type).await?,
            Internal::GetZkNymsAvailableForDownload => {
                get_zk_nyms_available_for_download(opts.client_type).await?
            }
            Internal::GetZkNymById(args) => get_zk_nym_by_id(opts.client_type, args).await?,
            Internal::ConfirmZkNymDownloaded(args) => {
                confirm_zk_nym_downloaded(opts.client_type, args).await?
            }
            Internal::GetAvailableTickets => get_available_tickets(opts.client_type).await?,
        },
    }
    Ok(())
}

fn setup_user_agent(opts: &CliOptions, daemon_info: InfoResponse) -> UserAgent {
    opts.user_agent
        .clone()
        .map(nym_vpn_proto::UserAgent::from)
        .unwrap_or_else(|| construct_user_agent(daemon_info))
}

fn construct_user_agent(daemon_info: InfoResponse) -> UserAgent {
    let bin_info = nym_bin_common::bin_info_local_vergen!();
    let version = format!("{} ({})", bin_info.build_version, daemon_info.version);

    // Construct the platform string similar to how user agents are constructed in web browsers
    let name = System::name().unwrap_or("unknown".to_string());
    let os_long = System::long_os_version().unwrap_or("unknown".to_string());
    let arch = System::cpu_arch();
    let platform = format!("{}; {}; {}", name, os_long, arch);

    let git_commit = format!("{} ({})", bin_info.commit_sha, daemon_info.git_commit);
    UserAgent {
        application: bin_info.binary_name.to_string(),
        version,
        platform,
        git_commit,
    }
}

async fn connect(opts: CliOptions, connect_args: &cli::ConnectArgs) -> Result<()> {
    let entry = cli::parse_entry_point(connect_args)?;
    let exit = cli::parse_exit_point(connect_args)?;

    let mut client = vpnd_client::get_client(&opts.client_type).await?;
    let info = client.info(()).await?.into_inner();
    let user_agent = setup_user_agent(&opts, info);

    let request = tonic::Request::new(ConnectRequest {
        entry: entry.map(into_entry_point),
        exit: exit.map(into_exit_point),
        dns: connect_args.dns.map(nym_vpn_proto::Dns::from),
        enable_two_hop: connect_args.enable_two_hop,
        netstack: connect_args.netstack,
        disable_poisson_rate: connect_args.disable_poisson_rate,
        disable_background_cover_traffic: connect_args.disable_background_cover_traffic,
        enable_credentials_mode: connect_args.enable_credentials_mode,
        user_agent: Some(user_agent),
        min_mixnode_performance: connect_args
            .min_mixnode_performance
            .map(nym_vpn_proto::Threshold::from),
        min_gateway_mixnet_performance: connect_args
            .min_gateway_mixnet_performance
            .map(nym_vpn_proto::Threshold::from),
        min_gateway_vpn_performance: connect_args
            .min_gateway_vpn_performance
            .map(nym_vpn_proto::Threshold::from),
    });

    let response = client.vpn_connect(request).await?.into_inner();

    if opts.verbose {
        println!("{:#?}", response);
    }

    if response.success {
        handle_connect_success(opts, connect_args).await
    } else if let Some(error) = response.error {
        handle_connect_failure(error)
    } else {
        println!("Connect command failed with unknown error");
        Ok(())
    }
}

async fn handle_connect_success(opts: CliOptions, connect_args: &cli::ConnectArgs) -> Result<()> {
    if connect_args.wait {
        println!("Successfully sent connect command. Waiting until connected or failed.");
        wait_until_connected(opts).await
    } else {
        println!("Successfully sent connect command");
        Ok(())
    }
}

fn handle_connect_failure(error: nym_vpn_proto::ConnectRequestError) -> Result<()> {
    let kind = nym_vpn_proto::connect_request_error::ConnectRequestErrorType::try_from(error.kind)
        .context("failed to parse connect request error kind")?;
    println!("Connect command failed: {} (id={kind:?})", error.message);
    Ok(())
}

async fn wait_until_connected(opts: CliOptions) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;

    let mut stream = client.listen_to_tunnel_state(()).await?.into_inner();
    while let Some(new_state) = stream.message().await? {
        let new_state = TunnelState::try_from(new_state)?;
        println!("{}", new_state);

        match new_state {
            TunnelState::Connected { .. } => {
                break;
            }
            TunnelState::Offline { reconnect } => {
                if reconnect {
                    println!("Device is offline. Waiting for network connectivity.");
                } else {
                    bail!("Device is offline");
                }
            }
            TunnelState::Error(reason) => {
                bail!("Tunnel entered error state {:?}", reason);
            }
            _ => {}
        }
    }
    Ok(())
}

async fn disconnect(opts: CliOptions, wait: bool) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type.clone()).await?;
    let response = client.vpn_disconnect(()).await?.into_inner();

    if opts.verbose {
        println!("{:#?}", response);
    }

    if response.success {
        if wait {
            println!("Successfully sent disconnect command. Waiting until disconnected.");
            wait_until_disconnected(opts).await
        } else {
            println!("Successfully sent disconnect command");
            Ok(())
        }
    } else {
        println!("Disconnect command failed");
        Ok(())
    }
}

async fn wait_until_disconnected(opts: CliOptions) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;
    let mut stream = client.listen_to_tunnel_state(()).await?.into_inner();
    while let Some(new_state) = stream.message().await? {
        let new_state = TunnelState::try_from(new_state)?;
        println!("{}", new_state);

        match new_state {
            TunnelState::Disconnected | TunnelState::Offline { .. } => {
                break;
            }
            TunnelState::Error(reason) => {
                bail!("Tunnel entered error state: {:?}", reason)
            }
            _ => {}
        }
    }
    Ok(())
}

async fn status(listen: bool, opts: CliOptions) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;

    if listen {
        let mut stream = client.listen_to_tunnel_state(()).await?.into_inner();
        while let Some(new_state) = stream.message().await? {
            let tunnel_state = TunnelState::try_from(new_state)?;
            println!("{}", tunnel_state);
        }
    } else {
        let tunnel_state = TunnelState::try_from(client.get_tunnel_state(()).await?.into_inner())?;
        println!("{}", tunnel_state);
    }

    Ok(())
}

async fn info(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.info(()).await?.into_inner();
    let info = nym_vpn_proto::conversions::InfoResponse::try_from(response)
        .context("failed to parse info response")?;
    println!("{info}");
    Ok(())
}

async fn set_network(client_type: ClientType, args: &cli::SetNetworkArgs) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let request = tonic::Request::new(SetNetworkRequest {
        network: args.network.clone(),
    });
    let response = client.set_network(request).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_system_messages(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_system_messages(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_feature_flags(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_feature_flags(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn store_account(opts: CliOptions, store_args: &cli::StoreAccountArgs) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;
    let request = tonic::Request::new(StoreAccountRequest {
        mnemonic: store_args.mnemonic.clone(),
        nonce: 0,
    });
    let response = client.store_account(request).await?.into_inner();
    if opts.verbose {
        println!("{:#?}", response);
    }
    if response.success {
        println!("Account recovery phrase stored");
    } else {
        let msg = if let Some(error) = response.error {
            let kind = nym_vpn_proto::account_error::AccountErrorType::try_from(error.kind)
                .context("failed to parse account error kind");
            format!("{} (id={:?})", error.message, kind)
        } else {
            "unknown".to_owned()
        };
        println!("Error: {msg}");
    }
    Ok(())
}

async fn refresh_account_state(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.refresh_account_state(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn is_account_stored(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.is_account_stored(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_account_usage(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_account_usage(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn forget_account(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.forget_account(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_account_id(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_account_identity(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_account_links(opts: CliOptions, args: &cli::GetAccountLinksArgs) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;
    let request = tonic::Request::new(GetAccountLinksRequest {
        locale: args.locale.clone(),
    });
    let response = client.get_account_links(request).await?.into_inner();
    if opts.verbose {
        println!("{:#?}", response);
    }
    match response
        .res
        .context("failed to parse get account links response")?
    {
        nym_vpn_proto::get_account_links_response::Res::Links(links) => {
            let links = nym_vpn_network_config::ParsedAccountLinks::try_from(links)
                .context("failed to parse account links into ParsedAccountLinks")?;
            println!("{links}");
        }
        nym_vpn_proto::get_account_links_response::Res::Error(err) => {
            println!("Error: {err:#?}");
        }
    };

    Ok(())
}

async fn get_account_state(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_account_state(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn reset_device_identity(
    client_type: ClientType,
    args: &cli::ResetDeviceIdentityArgs,
) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let request = tonic::Request::new(ResetDeviceIdentityRequest {
        seed: args.seed.as_ref().map(|seed| seed.clone().into_bytes()),
    });
    let response = client.reset_device_identity(request).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_device_id(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_device_identity(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn register_device(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.register_device(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_devices(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_devices(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_active_devices(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_active_devices(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn request_zk_nym(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.request_zk_nym(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_device_zk_nym(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_device_zk_nyms(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_zk_nyms_available_for_download(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client
        .get_zk_nyms_available_for_download(())
        .await?
        .into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_zk_nym_by_id(client_type: ClientType, args: cli::GetZkNymByIdArgs) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let request = tonic::Request::new(GetZkNymByIdRequest {
        id: args.id.clone(),
    });
    let response = client.get_zk_nym_by_id(request).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn confirm_zk_nym_downloaded(
    client_type: ClientType,
    args: cli::ConfirmZkNymDownloadedArgs,
) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let request = tonic::Request::new(ConfirmZkNymDownloadedRequest {
        id: args.id.clone(),
    });
    let response = client
        .confirm_zk_nym_downloaded(request)
        .await?
        .into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn get_available_tickets(client_type: ClientType) -> Result<()> {
    let mut client = vpnd_client::get_client(&client_type).await?;
    let response = client.get_available_tickets(()).await?.into_inner();
    println!("{:#?}", response);
    Ok(())
}

async fn list_gateways(
    opts: CliOptions,
    list_args: &cli::ListGatewaysArgs,
    gw_type: GatewayType,
) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;

    let info = client.info(()).await?.into_inner();
    let user_agent = setup_user_agent(&opts, info);

    let request = tonic::Request::new(ListGatewaysRequest {
        kind: into_gateway_type(gw_type.clone()) as i32,
        user_agent: Some(user_agent),
        min_mixnet_performance: list_args
            .min_mixnet_performance
            .map(nym_vpn_proto::Threshold::from),
        min_vpn_performance: list_args
            .min_vpn_performance
            .map(nym_vpn_proto::Threshold::from),
    });
    let response = client.list_gateways(request).await?.into_inner();
    if opts.verbose {
        println!("{:#?}", response);
    }
    println!("Gateways available for: {gw_type}");
    println!("Total gateways: {}", response.gateways.len());
    for gateway in response.gateways.clone() {
        if let Ok(gateway) = nym_vpnd_types::gateway::Gateway::try_from(gateway)
            .inspect_err(|e| println!("Failed to parse gateway: {e}"))
        {
            println!("  {gateway}");
        }
    }
    Ok(())
}

async fn list_countries(
    opts: CliOptions,
    list_args: &cli::ListCountriesArgs,
    gw_type: GatewayType,
) -> Result<()> {
    let mut client = vpnd_client::get_client(&opts.client_type).await?;

    let info = client.info(()).await?.into_inner();
    let user_agent = setup_user_agent(&opts, info);

    let request = tonic::Request::new(ListCountriesRequest {
        kind: into_gateway_type(gw_type.clone()) as i32,
        user_agent: Some(user_agent),
        min_mixnet_performance: list_args
            .min_mixnet_performance
            .map(nym_vpn_proto::Threshold::from),
        min_vpn_performance: list_args
            .min_vpn_performance
            .map(nym_vpn_proto::Threshold::from),
    });

    let response = client.list_countries(request).await?.into_inner();
    if opts.verbose {
        println!("{:#?}", response);
    }

    let countries = response
        .countries
        .into_iter()
        .map(nym_vpnd_types::gateway::Country::from)
        .collect::<Vec<_>>();

    println!(
        "Countries for {} ({}): {}",
        gw_type,
        countries.len(),
        countries.iter().join(", ")
    );

    Ok(())
}
