// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};
#[cfg(unix)]
use std::{os::fd::RawFd, sync::Arc};

use crate::types::Entry;
use anyhow::{anyhow, bail};
use base64::{engine::general_purpose, Engine as _};
use bytes::BytesMut;
use clap::Args;
use futures::StreamExt;
use nym_authenticator_client::{
    AuthClientMixnetListener, AuthenticatorResponse, AuthenticatorVersion, ClientMessage,
};
use nym_authenticator_requests::{v2, v3, v4, v5};
use nym_client_core::config::ForgetMe;
use nym_config::defaults::{
    mixnet_vpn::{NYM_TUN_DEVICE_ADDRESS_V4, NYM_TUN_DEVICE_ADDRESS_V6},
    NymNetworkDetails,
};
use nym_connection_monitor::self_ping_and_wait;
use nym_credentials_interface::TicketType;
use nym_gateway_directory::{
    AuthAddress, Config as GatewayDirectoryConfig, EntryPoint,
    GatewayClient as GatewayDirectoryClient, GatewayList, GatewayMinPerformance,
    IpPacketRouterAddress, NymNode,
};
use nym_ip_packet_client::IprClientConnect;
use nym_ip_packet_requests::{
    codec::MultiIpPacketCodec,
    v8::response::{
        ControlResponse, DataResponse, InfoLevel, IpPacketResponse, IpPacketResponseData,
    },
    IpPair,
};
use nym_mixnet_client::SharedMixnetClient;
use nym_sdk::mixnet::{MixnetClientBuilder, NodeIdentity, ReconstructedMessage};
use nym_wireguard_types::PeerPublicKey;
use tokio_util::{codec::Decoder, sync::CancellationToken};
use tracing::*;
use types::WgProbeResults;

use crate::{
    icmp::{check_for_icmp_beacon_reply, icmp_identifier, send_ping_v4, send_ping_v6},
    types::Exit,
};

use netstack::{
    ffi::{NetstackCall as _, NetstackCallImpl, NetstackRequestGo},
    NetstackRequest,
};

mod error;
mod icmp;
mod netstack;
mod types;

pub use error::{Error, Result};
pub use types::{IpPingReplies, ProbeOutcome, ProbeResult};

#[derive(Args)]
pub struct NetstackArgs {
    #[arg(long, default_value_t = 180)]
    netstack_download_timeout_sec: u64,

    #[arg(long, default_value = "1.1.1.1")]
    netstack_v4_dns: String,

    #[arg(long, default_value = "2606:4700:4700::1111")]
    netstack_v6_dns: String,

    #[arg(long, default_value_t = 5)]
    netstack_num_ping: u8,

    #[arg(long, default_value_t = 3)]
    netstack_send_timeout_sec: u64,

    #[arg(long, default_value_t = 3)]
    netstack_recv_timeout_sec: u64,

    #[arg(long, default_values_t = vec!["nymtech.net".to_string()])]
    netstack_ping_hosts_v4: Vec<String>,

    #[arg(long, default_values_t = vec!["1.1.1.1".to_string()])]
    netstack_ping_ips_v4: Vec<String>,

    #[arg(long, default_values_t = vec!["ipv6.google.com".to_string()])]
    netstack_ping_hosts_v6: Vec<String>,

    #[arg(long, default_values_t = vec!["2001:4860:4860::8888".to_string(), "2606:4700:4700::1111".to_string(), "2620:fe::fe".to_string()])]
    netstack_ping_ips_v6: Vec<String>,
}

#[derive(Args)]
pub struct CredentialArgs {
    #[arg(long)]
    enable_credentials_mode: bool,

    #[arg(long, required_if_eq("enable_credentials_mode", "true"))]
    mnemonic: Option<String>,
}

#[derive(Default, Debug)]
pub enum TestedNode {
    #[default]
    SameAsEntry,
    Custom {
        identity: NodeIdentity,
    },
}

impl TestedNode {
    pub fn is_same_as_entry(&self) -> bool {
        matches!(self, TestedNode::SameAsEntry)
    }
}

#[derive(Debug)]
pub struct TestedNodeDetails {
    identity: NodeIdentity,
    exit_router_address: Option<IpPacketRouterAddress>,
    authenticator_address: Option<AuthAddress>,
    authenticator_version: AuthenticatorVersion,
    ip_address: Option<IpAddr>,
}

impl From<&NymNode> for TestedNodeDetails {
    fn from(node: &NymNode) -> Self {
        TestedNodeDetails {
            identity: node.identity,
            exit_router_address: node.ipr_address,
            authenticator_address: node.authenticator_address,
            authenticator_version: AuthenticatorVersion::from(node.version.as_ref()),
            ip_address: node.ips.first().copied(),
        }
    }
}

/// Obtain nym-node for testing
pub async fn get_nym_node(
    config: GatewayDirectoryConfig,
    identity: NodeIdentity,
) -> anyhow::Result<NymNode> {
    let user_agent = nym_bin_common::bin_info_local_vergen!().into();
    let nodes_client = GatewayDirectoryClient::new(config, user_agent)?;
    let nodes = nodes_client.lookup_all_nymnodes().await?;
    let node = nodes
        .node_with_identity(&identity)
        .ok_or_else(|| anyhow!("did not find the specified node"))?;
    Ok(node.clone())
}

pub async fn fetch_gateways(gateway_config: GatewayDirectoryConfig) -> anyhow::Result<GatewayList> {
    lookup_gateways(gateway_config).await
}

pub async fn fetch_gateways_with_ipr(
    gateway_config: GatewayDirectoryConfig,
) -> anyhow::Result<GatewayList> {
    Ok(lookup_gateways(gateway_config).await?.into_exit_gateways())
}

pub struct Probe {
    entrypoint: EntryPoint,
    tested_node: TestedNode,
    amnezia_args: String,
    netstack_args: NetstackArgs,
    credentials_args: CredentialArgs,
}

impl Probe {
    pub fn new(
        entrypoint: EntryPoint,
        tested_node: TestedNode,
        netstack_args: NetstackArgs,
        credentials_args: CredentialArgs,
    ) -> Self {
        Self {
            entrypoint,
            tested_node,
            amnezia_args: "".into(),
            netstack_args,
            credentials_args,
        }
    }
    pub fn with_amnezia(&mut self, args: &str) -> &Self {
        self.amnezia_args = args.to_string();
        self
    }

    pub async fn probe(
        self,
        gateway_config: GatewayDirectoryConfig,
        ignore_egress_epoch_role: bool,
        only_wireguard: bool,
    ) -> anyhow::Result<ProbeResult> {
        let entry_point = self.entrypoint;

        // Setup the entry gateways
        let gateways = lookup_gateways(gateway_config.clone()).await?;
        let entry_gateway = entry_point.lookup_gateway(&gateways).await?;
        let tested_entry = self.tested_node.is_same_as_entry();

        let node_info: TestedNodeDetails = match self.tested_node {
            TestedNode::Custom { identity } => {
                let node = get_nym_node(gateway_config.clone(), identity).await?;
                info!(
                    "testing node {} (via entry {})",
                    node.identity, entry_gateway.identity
                );
                (&node).into()
            }
            TestedNode::SameAsEntry => (&entry_gateway).into(),
        };

        let mixnet_entry_gateway_id = entry_gateway.identity();

        info!("connecting to entry gateway: {entry_gateway:?}");
        debug!(
            "authenticator version: {:?}",
            node_info.authenticator_version
        );

        // Connect to the mixnet via the entry gateway
        let disconnected_mixnet_client = MixnetClientBuilder::new_ephemeral()
            .request_gateway(mixnet_entry_gateway_id.to_string())
            .network_details(NymNetworkDetails::new_from_env())
            .debug_config(mixnet_debug_config(
                gateway_config.min_gateway_performance,
                ignore_egress_epoch_role,
            ))
            .with_forget_me(ForgetMe::new_all())
            .credentials_mode(self.credentials_args.enable_credentials_mode)
            .build()?;

        if self.credentials_args.enable_credentials_mode {
            let mnemonic = self
                .credentials_args
                .mnemonic
                .expect("Mnemonic should be required if credentials mode is enabled");

            for ticketbook_type in [
                TicketType::V1MixnetEntry,
                TicketType::V1WireguardEntry,
                TicketType::V1WireguardExit,
            ] {
                let bw_client = disconnected_mixnet_client
                    .create_bandwidth_client(mnemonic.clone(), ticketbook_type)
                    .await?;
                bw_client.acquire().await?;
            }
        }
        let mixnet_client = disconnected_mixnet_client.connect_to_mixnet().await;

        let mixnet_client = match mixnet_client {
            Ok(mixnet_client) => mixnet_client,
            Err(err) => {
                error!("Failed to connect to mixnet: {err}");
                return Ok(ProbeResult {
                    node: node_info.identity.to_string(),
                    used_entry: mixnet_entry_gateway_id.to_string(),
                    outcome: ProbeOutcome {
                        as_entry: if tested_entry {
                            Entry::fail_to_connect()
                        } else {
                            Entry::EntryFailure
                        },
                        as_exit: None,
                        wg: None,
                    },
                });
            }
        };

        let nym_address = *mixnet_client.nym_address();
        let entry_gateway = nym_address.gateway().to_base58_string();

        info!("Successfully connected to entry gateway: {entry_gateway}");
        info!("Our nym address: {nym_address}");

        let shared_client = SharedMixnetClient::new(
            mixnet_client,
            #[cfg(unix)]
            Arc::new(|_: RawFd| {}),
        );

        // Now that we have a connected mixnet client, we can start pinging
        let outcome = if only_wireguard {
            Ok(ProbeOutcome {
                as_entry: if tested_entry {
                    Entry::success()
                } else {
                    Entry::NotTested
                },
                as_exit: None,
                wg: None,
            })
        } else {
            do_ping(
                shared_client.clone(),
                node_info.exit_router_address,
                tested_entry,
            )
            .await
        };

        let wg_outcome = if let (Some(authenticator), Some(ip_address)) =
            (node_info.authenticator_address, node_info.ip_address)
        {
            // Start the mixnet listener that the auth clients use to receive messages.
            let mixnet_listener_task = AuthClientMixnetListener::new(shared_client.clone()).start();
            let auth_client = mixnet_listener_task.new_auth_client().await;

            let outcome = wg_probe(
                authenticator,
                auth_client,
                ip_address,
                node_info.authenticator_version,
                self.amnezia_args,
                self.netstack_args,
            )
            .await
            .unwrap_or_default();

            let _ = mixnet_listener_task.cancel().await;

            outcome
        } else {
            WgProbeResults::default()
        };

        shared_client.disconnect().await;

        // Disconnect the mixnet client gracefully
        outcome.map(|mut outcome| {
            outcome.wg = Some(wg_outcome);
            ProbeResult {
                node: node_info.identity.to_string(),
                used_entry: mixnet_entry_gateway_id.to_string(),
                outcome,
            }
        })
    }
}

async fn wg_probe(
    authenticator: AuthAddress,
    mut auth_client: nym_authenticator_client::AuthClient,
    gateway_ip: IpAddr,
    auth_version: AuthenticatorVersion,
    awg_args: String,
    netstack_args: NetstackArgs,
) -> anyhow::Result<WgProbeResults> {
    info!("attempting to use authenticator version {auth_version:?}");

    let mut rng = rand::thread_rng();
    let private_key = nym_crypto::asymmetric::encryption::PrivateKey::new(&mut rng);
    let public_key = private_key.public_key();

    let authenticator_pub_key = PeerPublicKey::new(public_key.to_bytes().into());
    let init_message = match auth_version {
        AuthenticatorVersion::V2 => ClientMessage::Initial(Box::new(
            v2::registration::InitMessage::new(authenticator_pub_key),
        )),
        AuthenticatorVersion::V3 => ClientMessage::Initial(Box::new(
            v3::registration::InitMessage::new(authenticator_pub_key),
        )),
        AuthenticatorVersion::V4 => ClientMessage::Initial(Box::new(
            v4::registration::InitMessage::new(authenticator_pub_key),
        )),
        AuthenticatorVersion::V5 => ClientMessage::Initial(Box::new(
            v5::registration::InitMessage::new(authenticator_pub_key),
        )),
        AuthenticatorVersion::UNKNOWN => bail!("unknown version number"),
    };

    let mut wg_outcome = WgProbeResults::default();

    if let Some(authenticator_address) = authenticator.0 {
        info!("connecting to authenticator: {authenticator_address}...");
        let response = auth_client
            .send(&init_message, authenticator_address)
            .await?;

        let registered_data = match response {
            nym_authenticator_client::AuthenticatorResponse::PendingRegistration(
                pending_registration_response,
            ) => {
                // Unwrap since we have already checked that we have the keypair.
                debug!("Verifying data");
                pending_registration_response.verify(&private_key)?;

                let finalized_message = match auth_version {
                    AuthenticatorVersion::V2 => {
                        ClientMessage::Final(Box::new(v2::registration::FinalMessage {
                            gateway_client: v2::registration::GatewayClient::new(
                                &private_key,
                                pending_registration_response.pub_key().inner(),
                                pending_registration_response.private_ips().ipv4.into(),
                                pending_registration_response.nonce(),
                            ),
                            credential: None,
                        }))
                    }
                    AuthenticatorVersion::V3 => {
                        ClientMessage::Final(Box::new(v3::registration::FinalMessage {
                            gateway_client: v3::registration::GatewayClient::new(
                                &private_key,
                                pending_registration_response.pub_key().inner(),
                                pending_registration_response.private_ips().ipv4.into(),
                                pending_registration_response.nonce(),
                            ),
                            credential: None,
                        }))
                    }
                    AuthenticatorVersion::V4 => {
                        ClientMessage::Final(Box::new(v4::registration::FinalMessage {
                            gateway_client: v4::registration::GatewayClient::new(
                                &private_key,
                                pending_registration_response.pub_key().inner(),
                                pending_registration_response.private_ips().into(),
                                pending_registration_response.nonce(),
                            ),
                            credential: None,
                        }))
                    }
                    AuthenticatorVersion::V5 => {
                        ClientMessage::Final(Box::new(v5::registration::FinalMessage {
                            gateway_client: v5::registration::GatewayClient::new(
                                &private_key,
                                pending_registration_response.pub_key().inner(),
                                pending_registration_response.private_ips(),
                                pending_registration_response.nonce(),
                            ),
                            credential: None,
                        }))
                    }
                    AuthenticatorVersion::UNKNOWN => bail!("Unknown version number"),
                };
                let response = auth_client
                    .send(&finalized_message, authenticator_address)
                    .await?;
                let AuthenticatorResponse::Registered(registered_response) = response else {
                    bail!("Unexpected response");
                };
                registered_response
            }
            nym_authenticator_client::AuthenticatorResponse::Registered(registered_response) => {
                registered_response
            }
            _ => bail!("Unexpected response"),
        };

        let peer_public = registered_data.pub_key().inner();
        let static_private = x25519_dalek::StaticSecret::from(private_key.to_bytes());
        let public_key_bs64 = general_purpose::STANDARD.encode(peer_public.as_bytes());
        let private_key_hex = hex::encode(static_private.to_bytes());
        let public_key_hex = hex::encode(peer_public.as_bytes());

        info!("WG connection details");
        info!("Peer public key: {}", public_key_bs64);
        info!(
            "ips {}(v4) {}(v6), port {}",
            registered_data.private_ips().ipv4,
            registered_data.private_ips().ipv6,
            registered_data.wg_port(),
        );

        let wg_endpoint = format!("{gateway_ip}:{}", registered_data.wg_port());

        info!("Successfully registered with the gateway");

        wg_outcome.can_register = true;

        if wg_outcome.can_register {
            let netstack_request = NetstackRequest::new(
                &registered_data.private_ips().ipv4.to_string(),
                &registered_data.private_ips().ipv6.to_string(),
                &private_key_hex,
                &public_key_hex,
                &wg_endpoint,
                netstack_args.netstack_download_timeout_sec,
                &awg_args,
                netstack_args,
            );

            // Perform IPv4 ping test
            let ipv4_request = NetstackRequestGo::from_rust_v4(&netstack_request);

            let netstack_response_v4 = NetstackCallImpl::ping(&ipv4_request);
            info!(
                "Wireguard probe response for IPv4: {:?}",
                netstack_response_v4
            );
            wg_outcome.can_handshake_v4 = netstack_response_v4.can_handshake;
            wg_outcome.can_resolve_dns_v4 = netstack_response_v4.can_resolve_dns;
            wg_outcome.ping_hosts_performance_v4 =
                netstack_response_v4.received_hosts as f32 / netstack_response_v4.sent_hosts as f32;
            wg_outcome.ping_ips_performance_v4 =
                netstack_response_v4.received_ips as f32 / netstack_response_v4.sent_ips as f32;

            wg_outcome.download_duration_sec_v4 = netstack_response_v4.download_duration_sec;
            wg_outcome.downloaded_file_v4 = netstack_response_v4.downloaded_file;
            wg_outcome.download_error_v4 = netstack_response_v4.download_error;

            // Perform IPv6 ping test
            let ipv6_request = NetstackRequestGo::from_rust_v6(&netstack_request);

            let netstack_response_v6 = NetstackCallImpl::ping(&ipv6_request);
            info!(
                "Wireguard probe response for IPv6: {:?}",
                netstack_response_v6
            );
            wg_outcome.can_handshake_v6 = netstack_response_v6.can_handshake;
            wg_outcome.can_resolve_dns_v6 = netstack_response_v6.can_resolve_dns;
            wg_outcome.ping_hosts_performance_v6 =
                netstack_response_v6.received_hosts as f32 / netstack_response_v6.sent_hosts as f32;
            wg_outcome.ping_ips_performance_v6 =
                netstack_response_v6.received_ips as f32 / netstack_response_v6.sent_ips as f32;

            wg_outcome.download_duration_sec_v6 = netstack_response_v6.download_duration_sec;
            wg_outcome.downloaded_file_v6 = netstack_response_v6.downloaded_file;
            wg_outcome.download_error_v6 = netstack_response_v6.download_error;
        }
    }

    Ok(wg_outcome)
}

async fn lookup_gateways(gateway_config: GatewayDirectoryConfig) -> anyhow::Result<GatewayList> {
    info!("nym-api: {}", gateway_config.api_url());
    info!(
        "nym-vpn-api: {}",
        gateway_config
            .nym_vpn_api_url()
            .map(|url| url.to_string())
            .unwrap_or("unavailable".to_string())
    );

    let user_agent = nym_bin_common::bin_info_local_vergen!().into();
    let gateway_client = GatewayDirectoryClient::new(gateway_config, user_agent)?;
    let gateways = gateway_client.lookup_all_gateways_from_nym_api().await?;
    Ok(gateways)
}

fn mixnet_debug_config(
    min_gateway_performance: Option<GatewayMinPerformance>,
    ignore_egress_epoch_role: bool,
) -> nym_client_core::config::DebugConfig {
    let mut debug_config = nym_client_core::config::DebugConfig::default();
    debug_config
        .traffic
        .disable_main_poisson_packet_distribution = true;
    debug_config.cover_traffic.disable_loop_cover_traffic_stream = true;
    if let Some(minimum_gateway_performance) =
        min_gateway_performance.and_then(|p| p.mixnet_min_performance)
    {
        debug_config.topology.minimum_gateway_performance =
            minimum_gateway_performance.round_to_integer();
    }
    if ignore_egress_epoch_role {
        debug_config.topology.ignore_egress_epoch_role = ignore_egress_epoch_role;
    }

    debug_config
}

async fn do_ping(
    shared_mixnet_client: SharedMixnetClient,
    exit_router_address: Option<IpPacketRouterAddress>,
    tested_entry: bool,
) -> anyhow::Result<ProbeOutcome> {
    // Step 1: confirm that the entry gateway is routing our mixnet traffic
    info!("Sending mixnet ping to ourselves to verify mixnet connection");
    if self_ping_and_wait(
        shared_mixnet_client.nym_address().await,
        shared_mixnet_client.inner(),
    )
    .await
    .is_err()
    {
        return Ok(ProbeOutcome {
            as_entry: if tested_entry {
                Entry::fail_to_connect()
            } else {
                Entry::EntryFailure
            },
            as_exit: None,
            wg: None,
        });
    }
    info!("Successfully mixnet pinged ourselves");

    let as_entry = if tested_entry {
        Entry::success()
    } else {
        Entry::NotTested
    };

    let Some(exit_router_address) = exit_router_address else {
        return Ok(ProbeOutcome {
            as_entry,
            as_exit: None,
            wg: None,
        });
    };

    // Step 2: connect to the exit gateway
    info!(
        "Connecting to exit gateway: {}",
        exit_router_address.gateway().to_base58_string()
    );
    // The IPR supports cancellation, but it's unused in the gateway probe
    let cancel_token = CancellationToken::new();
    let mut ipr_client = IprClientConnect::new(shared_mixnet_client.clone(), cancel_token).await;
    let Ok(our_ips) = ipr_client.connect(exit_router_address).await else {
        return Ok(ProbeOutcome {
            as_entry,
            as_exit: Some(Exit::fail_to_connect()),
            wg: None,
        });
    };
    info!("Successfully connected to exit gateway");
    info!("Using mixnet VPN IP addresses: {our_ips}");

    // Step 3: perform ICMP connectivity checks for the exit gateway
    send_icmp_pings(shared_mixnet_client.clone(), our_ips, exit_router_address).await?;
    listen_for_icmp_ping_replies(shared_mixnet_client.clone(), our_ips, as_entry).await
}

async fn send_icmp_pings(
    shared_mixnet_client: SharedMixnetClient,
    our_ips: IpPair,
    exit_router_address: IpPacketRouterAddress,
) -> anyhow::Result<()> {
    // ipv4 addresses for testing
    let ipr_tun_ip_v4 = NYM_TUN_DEVICE_ADDRESS_V4;
    let external_ip_v4 = Ipv4Addr::new(8, 8, 8, 8);

    // ipv6 addresses for testing
    let ipr_tun_ip_v6 = NYM_TUN_DEVICE_ADDRESS_V6;
    let external_ip_v6 = Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888);

    info!("Sending ICMP echo requests to: {ipr_tun_ip_v4}, {ipr_tun_ip_v6}, {external_ip_v4}, {external_ip_v6}");

    // send ipv4 pings
    for ii in 0..10 {
        send_ping_v4(
            shared_mixnet_client.clone(),
            our_ips,
            ii,
            ipr_tun_ip_v4,
            exit_router_address,
        )
        .await?;
        send_ping_v4(
            shared_mixnet_client.clone(),
            our_ips,
            ii,
            external_ip_v4,
            exit_router_address,
        )
        .await?;
    }

    // send ipv6 pings
    for ii in 0..10 {
        send_ping_v6(
            shared_mixnet_client.clone(),
            our_ips,
            ii,
            ipr_tun_ip_v6,
            exit_router_address,
        )
        .await?;
        send_ping_v6(
            shared_mixnet_client.clone(),
            our_ips,
            ii,
            external_ip_v6,
            exit_router_address,
        )
        .await?;
    }
    Ok(())
}

async fn listen_for_icmp_ping_replies(
    shared_mixnet_client: SharedMixnetClient,
    our_ips: IpPair,
    entry_result: Entry,
) -> anyhow::Result<ProbeOutcome> {
    // HACK: take it out of the shared mixnet client
    let mut mixnet_client = shared_mixnet_client.inner().lock().await.take().unwrap();
    let mut multi_ip_packet_decoder = MultiIpPacketCodec::new();
    let mut registered_replies = IpPingReplies::new();

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                info!("Finished waiting for ICMP echo reply from exit gateway");
                break;
            }
            Some(reconstructed_message) = mixnet_client.next() => {
                let Some(data_response) = unpack_data_response(&reconstructed_message) else {
                    continue;
                };

                // IP packets are bundled together in a mixnet message
                let mut bytes = BytesMut::from(&*data_response.ip_packet);
                while let Ok(Some(packet)) = multi_ip_packet_decoder.decode(&mut bytes) {
                    if let Some(event) = check_for_icmp_beacon_reply(&packet.into_bytes(), icmp_identifier(), our_ips) {
                        info!("Received ICMP echo reply from exit gateway");
                        info!("Connection event: {event:?}");
                        registered_replies.register_event(&event);
                    }
                }
            }
        }
    }

    // HACK: put it back in the shared mixnet client, so it can be properly disconnected
    shared_mixnet_client
        .inner()
        .lock()
        .await
        .replace(mixnet_client);

    Ok(ProbeOutcome {
        as_entry: entry_result,
        as_exit: Some(Exit {
            can_connect: true,
            can_route_ip_v4: registered_replies.ipr_tun_ip_v4,
            can_route_ip_external_v4: registered_replies.external_ip_v4,
            can_route_ip_v6: registered_replies.ipr_tun_ip_v6,
            can_route_ip_external_v6: registered_replies.external_ip_v6,
        }),
        wg: None,
    })
}

fn unpack_data_response(reconstructed_message: &ReconstructedMessage) -> Option<DataResponse> {
    match IpPacketResponse::from_reconstructed_message(reconstructed_message) {
        Ok(response) => match response.data {
            IpPacketResponseData::Data(data_response) => Some(data_response),
            IpPacketResponseData::Control(control) => match *control {
                ControlResponse::Info(info) => {
                    let msg = format!("Received info response from the mixnet: {}", info.reply);
                    match info.level {
                        InfoLevel::Info => info!("{msg}"),
                        InfoLevel::Warn => warn!("{msg}"),
                        InfoLevel::Error => error!("{msg}"),
                    }
                    None
                }
                _ => {
                    info!("Ignoring: {:?}", control);
                    None
                }
            },
        },
        Err(err) => {
            warn!("Failed to parse mixnet message: {err}");
            None
        }
    }
}
