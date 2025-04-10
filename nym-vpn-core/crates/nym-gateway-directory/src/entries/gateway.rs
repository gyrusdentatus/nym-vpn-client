// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use itertools::Itertools;
use nym_sdk::mixnet::NodeIdentity;
use nym_topology::{NodeId, RoutingNode};
use nym_vpn_api_client::types::{NaiveFloat, Percent, ScoreThresholds};
use rand::seq::IteratorRandom;
use std::{fmt, net::IpAddr};
use tracing::error;

use crate::{error::Result, AuthAddress, Country, Error, IpPacketRouterAddress};

use super::score::{Score, HIGH_SCORE_THRESHOLD, LOW_SCORE_THRESHOLD, MEDIUM_SCORE_THRESHOLD};

pub type NymNode = Gateway;

#[derive(Clone)]
pub struct Gateway {
    pub identity: NodeIdentity,
    pub moniker: String,
    pub location: Option<Location>,
    pub ipr_address: Option<IpPacketRouterAddress>,
    pub authenticator_address: Option<AuthAddress>,
    pub last_probe: Option<Probe>,
    pub ips: Vec<IpAddr>,
    pub host: Option<String>,
    pub clients_ws_port: Option<u16>,
    pub clients_wss_port: Option<u16>,
    pub mixnet_performance: Option<Percent>,
    pub wg_performance: Option<Percent>,
    pub wg_score: Option<Score>,
    pub mixnet_score: Option<Score>,
    pub version: Option<String>,
}

impl fmt::Debug for Gateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gateway")
            .field("identity", &self.identity.to_base58_string())
            .field("location", &self.location)
            .field("ipr_address", &self.ipr_address)
            .field("authenticator_address", &self.authenticator_address)
            .field("last_probe", &self.last_probe)
            .field("host", &self.host)
            .field("clients_ws_port", &self.clients_ws_port)
            .field("clients_wss_port", &self.clients_wss_port)
            .field("mixnet_performance", &self.mixnet_performance)
            .finish()
    }
}

impl Gateway {
    pub fn identity(&self) -> NodeIdentity {
        self.identity
    }

    pub fn two_letter_iso_country_code(&self) -> Option<&str> {
        self.location
            .as_ref()
            .map(|l| l.two_letter_iso_country_code.as_str())
    }

    pub fn is_two_letter_iso_country_code(&self, code: &str) -> bool {
        self.two_letter_iso_country_code() == Some(code)
    }

    pub fn has_ipr_address(&self) -> bool {
        self.ipr_address.is_some()
    }

    pub fn has_authenticator_address(&self) -> bool {
        self.authenticator_address.is_some()
    }

    pub fn host(&self) -> Option<&String> {
        self.host.as_ref()
    }

    pub fn lookup_ip(&self) -> Option<IpAddr> {
        self.ips.first().copied()
    }

    pub fn clients_address_no_tls(&self) -> Option<String> {
        match (&self.host, &self.clients_ws_port) {
            (Some(host), Some(port)) => Some(format!("ws://{}:{}", host, port)),
            _ => None,
        }
    }

    pub fn clients_address_tls(&self) -> Option<String> {
        match (&self.host, &self.clients_wss_port) {
            (Some(host), Some(port)) => Some(format!("wss://{}:{}", host, port)),
            _ => None,
        }
    }

    pub fn update_to_new_thresholds(
        &mut self,
        mix_thresholds: Option<ScoreThresholds>,
        wg_thresholds: Option<ScoreThresholds>,
    ) {
        if let (Some(mix_thresholds), Some(score)) = (mix_thresholds, self.mixnet_score.as_mut()) {
            score.update_to_new_thresholds(mix_thresholds);
        }
        if let (Some(wg_thresholds), Some(score)) = (wg_thresholds, self.wg_score.as_mut()) {
            score.update_to_new_thresholds(wg_thresholds);
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Location {
    pub two_letter_iso_country_code: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub last_updated_utc: String,
    pub outcome: ProbeOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeOutcome {
    pub as_entry: Entry,
    pub as_exit: Option<Exit>,
    pub wg: Option<WgProbeResults>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub can_connect: bool,
    pub can_route: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exit {
    pub can_connect: bool,
    pub can_route_ip_v4: bool,
    pub can_route_ip_external_v4: bool,
    pub can_route_ip_v6: bool,
    pub can_route_ip_external_v6: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WgProbeResults {
    pub can_register: bool,
    pub can_handshake: bool,
    pub can_resolve_dns: bool,
    pub ping_hosts_performance: f32,
    pub ping_ips_performance: f32,
}

impl From<nym_vpn_api_client::response::Location> for Location {
    fn from(location: nym_vpn_api_client::response::Location) -> Self {
        Location {
            two_letter_iso_country_code: location.two_letter_iso_country_code,
            latitude: location.latitude,
            longitude: location.longitude,
        }
    }
}

impl From<nym_vpn_api_client::response::Probe> for Probe {
    fn from(probe: nym_vpn_api_client::response::Probe) -> Self {
        Probe {
            last_updated_utc: probe.last_updated_utc,
            outcome: ProbeOutcome::from(probe.outcome),
        }
    }
}

impl From<Percent> for Score {
    fn from(percent: Percent) -> Self {
        let rounded_percent = percent.round_to_integer();
        if rounded_percent >= HIGH_SCORE_THRESHOLD {
            Score::High(rounded_percent)
        } else if rounded_percent >= MEDIUM_SCORE_THRESHOLD {
            Score::Medium(rounded_percent)
        } else if rounded_percent > LOW_SCORE_THRESHOLD {
            Score::Low(rounded_percent)
        } else {
            Score::None
        }
    }
}

impl From<nym_vpn_api_client::response::ProbeOutcome> for ProbeOutcome {
    fn from(outcome: nym_vpn_api_client::response::ProbeOutcome) -> Self {
        ProbeOutcome {
            as_entry: Entry::from(outcome.as_entry),
            as_exit: outcome.as_exit.map(Exit::from),
            wg: outcome.wg.map(WgProbeResults::from),
        }
    }
}

impl From<nym_vpn_api_client::response::Entry> for Entry {
    fn from(entry: nym_vpn_api_client::response::Entry) -> Self {
        Entry {
            can_connect: entry.can_connect,
            can_route: entry.can_route,
        }
    }
}

impl From<nym_vpn_api_client::response::Exit> for Exit {
    fn from(exit: nym_vpn_api_client::response::Exit) -> Self {
        Exit {
            can_connect: exit.can_connect,
            can_route_ip_v4: exit.can_route_ip_v4,
            can_route_ip_external_v4: exit.can_route_ip_external_v4,
            can_route_ip_v6: exit.can_route_ip_v6,
            can_route_ip_external_v6: exit.can_route_ip_external_v6,
        }
    }
}

impl From<nym_vpn_api_client::response::WgProbeResults> for WgProbeResults {
    fn from(results: nym_vpn_api_client::response::WgProbeResults) -> Self {
        WgProbeResults {
            can_register: results.can_register,
            can_handshake: results.can_handshake,
            can_resolve_dns: results.can_resolve_dns,
            ping_hosts_performance: results.ping_hosts_performance,
            ping_ips_performance: results.ping_ips_performance,
        }
    }
}

impl TryFrom<nym_vpn_api_client::response::NymDirectoryGateway> for Gateway {
    type Error = Error;

    fn try_from(gateway: nym_vpn_api_client::response::NymDirectoryGateway) -> Result<Self> {
        let identity =
            NodeIdentity::from_base58_string(&gateway.identity_key).map_err(|source| {
                Error::NodeIdentityFormattingError {
                    identity: gateway.identity_key,
                    source,
                }
            })?;

        let ipr_address = gateway
            .ip_packet_router
            .and_then(|ipr| IpPacketRouterAddress::try_from_base58_string(&ipr.address).ok());

        let authenticator_address = gateway
            .authenticator
            .and_then(|auth| AuthAddress::try_from_base58_string(&auth.address).ok());

        let hostname = gateway.entry.hostname;
        let first_ip_address = gateway
            .ip_addresses
            .first()
            .cloned()
            .map(|ip| ip.to_string());
        let host = hostname.or(first_ip_address);
        let wg_performance = gateway.last_probe.as_ref().and_then(|probe| {
            probe
                .outcome
                .wg
                .as_ref()
                .and_then(|p| Percent::naive_try_from_f64(p.ping_hosts_performance as f64).ok())
        });

        Ok(Gateway {
            identity,
            moniker: gateway.name,
            location: Some(gateway.location.into()),
            ipr_address,
            authenticator_address,
            last_probe: gateway.last_probe.map(Probe::from),
            ips: gateway.ip_addresses,
            host,
            clients_ws_port: Some(gateway.entry.ws_port),
            clients_wss_port: gateway.entry.wss_port,
            mixnet_performance: Some(gateway.performance),
            mixnet_score: Some(Score::from(gateway.performance)),
            wg_performance,
            wg_score: wg_performance.map(Score::from),
            version: gateway.build_information.map(|info| info.build_version),
        })
    }
}

impl TryFrom<nym_validator_client::models::NymNodeDescription> for Gateway {
    type Error = Error;

    fn try_from(
        node_description: nym_validator_client::models::NymNodeDescription,
    ) -> Result<Self> {
        let identity = node_description.description.host_information.keys.ed25519;
        let location = node_description
            .description
            .auxiliary_details
            .location
            .map(|l| Location {
                two_letter_iso_country_code: l.alpha2.to_string(),
                ..Default::default()
            });
        let ipr_address = node_description
            .description
            .ip_packet_router
            .as_ref()
            .and_then(|ipr| {
                IpPacketRouterAddress::try_from_base58_string(&ipr.address)
                    .inspect_err(|err| error!("Failed to parse IPR address: {err}"))
                    .ok()
            });
        let authenticator_address = node_description
            .description
            .authenticator
            .as_ref()
            .and_then(|a| {
                AuthAddress::try_from_base58_string(&a.address)
                    .inspect_err(|err| error!("Failed to parse authenticator address: {err}"))
                    .ok()
            });
        let version = Some(node_description.version().to_string());
        let role = if node_description.description.declared_role.entry {
            nym_validator_client::nym_nodes::NodeRole::EntryGateway
        } else if node_description.description.declared_role.exit_ipr
            || node_description.description.declared_role.exit_nr
        {
            nym_validator_client::nym_nodes::NodeRole::ExitGateway
        } else {
            nym_validator_client::nym_nodes::NodeRole::Inactive
        };

        let gateway =
            RoutingNode::try_from(&node_description.to_skimmed_node(role, Default::default()))
                .map_err(|_| Error::MalformedGateway)?;

        let host = gateway.ws_entry_address(false);
        let entry_info = &gateway.entry;
        let clients_ws_port = entry_info.as_ref().map(|g| g.clients_ws_port);
        let clients_wss_port = entry_info.as_ref().and_then(|g| g.clients_wss_port);
        let ips = node_description.description.host_information.ip_address;
        Ok(Gateway {
            identity,
            moniker: String::new(),
            location,
            ipr_address,
            authenticator_address,
            last_probe: None,
            ips,
            host,
            clients_ws_port,
            clients_wss_port,
            mixnet_performance: None,
            wg_performance: None,
            wg_score: None,
            mixnet_score: None,
            version,
        })
    }
}

pub type NymNodeList = GatewayList;

#[derive(Debug, Clone)]
pub struct GatewayList {
    gateways: Vec<Gateway>,
}

impl GatewayList {
    pub fn new(gateways: Vec<Gateway>) -> Self {
        GatewayList { gateways }
    }

    // Returns a list of all locations of the gateways, including duplicates
    fn all_locations(&self) -> impl Iterator<Item = &Location> {
        self.gateways
            .iter()
            .filter_map(|gateway| gateway.location.as_ref())
    }

    pub fn all_countries(&self) -> Vec<Country> {
        self.all_locations()
            .cloned()
            .map(Country::from)
            .unique()
            .collect()
    }

    pub fn all_iso_codes(&self) -> Vec<String> {
        self.all_countries()
            .into_iter()
            .map(|country| country.iso_code().to_string())
            .collect()
    }

    pub fn node_with_identity(&self, identity: &NodeIdentity) -> Option<&NymNode> {
        self.gateways
            .iter()
            .find(|node| &node.identity() == identity)
    }

    pub fn gateway_with_identity(&self, identity: &NodeIdentity) -> Option<&Gateway> {
        self.node_with_identity(identity)
    }

    pub fn gateways_located_at(&self, code: String) -> impl Iterator<Item = &Gateway> {
        self.gateways.iter().filter(move |gateway| {
            gateway
                .two_letter_iso_country_code()
                .is_some_and(|gw_code| gw_code == code)
        })
    }

    pub fn random_gateway(&self) -> Option<Gateway> {
        self.gateways
            .iter()
            .choose(&mut rand::thread_rng())
            .cloned()
    }

    pub fn random_gateway_located_at(&self, code: String) -> Option<Gateway> {
        self.gateways_located_at(code)
            .choose(&mut rand::thread_rng())
            .cloned()
    }

    pub fn remove_gateway(&mut self, entry_gateway: &Gateway) {
        self.gateways
            .retain(|gateway| gateway.identity() != entry_gateway.identity());
    }

    pub fn len(&self) -> usize {
        self.gateways.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gateways.is_empty()
    }

    pub fn into_exit_gateways(self) -> GatewayList {
        let gw = self
            .gateways
            .into_iter()
            .filter(Gateway::has_ipr_address)
            .collect();
        Self::new(gw)
    }

    pub fn into_vpn_gateways(self) -> GatewayList {
        let gw = self
            .gateways
            .into_iter()
            .filter(Gateway::has_authenticator_address)
            .collect();
        Self::new(gw)
    }

    pub fn into_countries(self) -> Vec<Country> {
        self.all_countries()
    }

    pub fn into_inner(self) -> Vec<Gateway> {
        self.gateways
    }

    pub(crate) async fn random_low_latency_gateway(&self) -> Result<Gateway> {
        let mut rng = rand::rngs::OsRng;
        nym_client_core::init::helpers::choose_gateway_by_latency(&mut rng, &self.gateways, false)
            .await
            .map_err(|err| Error::FailedToSelectGatewayBasedOnLowLatency { source: err })
    }
}

impl IntoIterator for GatewayList {
    type Item = Gateway;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.gateways.into_iter()
    }
}

impl nym_client_core::init::helpers::ConnectableGateway for Gateway {
    #[allow(unconditional_recursion)]
    fn node_id(&self) -> NodeId {
        self.node_id()
    }

    fn identity(&self) -> nym_sdk::mixnet::NodeIdentity {
        self.identity()
    }

    fn clients_address(&self, _prefer_ipv6: bool) -> Option<String> {
        // This is a bit of a sharp edge, but temporary until we can remove Option from host
        // and tls port when we add these to the vpn API endpoints.
        Some(
            self.clients_address_tls()
                .or(self.clients_address_no_tls())
                .unwrap_or("ws://".to_string()),
        )
    }

    fn is_wss(&self) -> bool {
        self.clients_address_tls().is_some()
    }
}

#[derive(Debug, Clone)]
pub enum GatewayType {
    MixnetEntry,
    MixnetExit,
    Wg,
}

impl fmt::Display for GatewayType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatewayType::MixnetEntry => write!(f, "mixnet entry"),
            GatewayType::MixnetExit => write!(f, "mixnet exit"),
            GatewayType::Wg => write!(f, "vpn"),
        }
    }
}

impl From<nym_vpn_api_client::types::GatewayType> for GatewayType {
    fn from(gateway_type: nym_vpn_api_client::types::GatewayType) -> Self {
        match gateway_type {
            nym_vpn_api_client::types::GatewayType::MixnetEntry => GatewayType::MixnetEntry,
            nym_vpn_api_client::types::GatewayType::MixnetExit => GatewayType::MixnetExit,
            nym_vpn_api_client::types::GatewayType::Wg => GatewayType::Wg,
        }
    }
}

impl From<GatewayType> for nym_vpn_api_client::types::GatewayType {
    fn from(gateway_type: GatewayType) -> Self {
        match gateway_type {
            GatewayType::MixnetEntry => nym_vpn_api_client::types::GatewayType::MixnetEntry,
            GatewayType::MixnetExit => nym_vpn_api_client::types::GatewayType::MixnetExit,
            GatewayType::Wg => nym_vpn_api_client::types::GatewayType::Wg,
        }
    }
}
