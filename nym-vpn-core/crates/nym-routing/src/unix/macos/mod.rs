// Copyright 2016-2024 Mullvad VPN AB. All Rights Reserved.
// Copyright 2024 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use crate::{debounce::BurstGuard, Gateway, MacAddress, NetNode, RequiredRoute, Route};

use futures::{
    future::FutureExt,
    stream::{FusedStream, StreamExt},
};
use ipnetwork::IpNetwork;
use nym_common::ErrorExt;
use std::{
    collections::{BTreeMap, HashSet},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Weak,
    time::Duration,
};
use tokio::sync::mpsc;
use watch::RoutingTable;

use super::{DefaultRouteEvent, RouteManagerCommand};
use data::{Destination, RouteDestination, RouteMessage, RouteSocketMessage};

pub use interface::DefaultRoute;

mod data;
mod interface;
mod routing_socket;
mod watch;

pub use watch::Error as RouteError;

pub type Result<T> = std::result::Result<T, Error>;

const BURST_BUFFER_PERIOD: Duration = Duration::from_millis(200);
const BURST_LONGEST_BUFFER_PERIOD: Duration = Duration::from_secs(2);

/// Errors that can happen in the macOS routing integration.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Encountered an error when interacting with the routing socket
    #[error("Error occurred when interfacing with the routing table")]
    RoutingTable(#[source] watch::Error),

    /// Failed to remove route
    #[error("Error occurred when deleting a route")]
    DeleteRoute(#[source] watch::Error),

    /// Failed to add route
    #[error("Error occurred when adding a route")]
    AddRoute(#[source] watch::Error),

    /// Failed to fetch link addresses
    #[error("Failed to fetch link addresses")]
    FetchLinkAddresses(#[source] std::io::Error),

    /// Received message isn't valid
    #[error("Invalid data")]
    InvalidData(#[source] data::Error),
}

/// Convenience macro to get the current default route. Macro because I don't want to borrow `self`
/// mutably.
macro_rules! get_current_best_default_route {
    ($self:expr, $family:expr) => {{
        match $family {
            interface::Family::V4 => &mut $self.v4_default_route,
            interface::Family::V6 => &mut $self.v6_default_route,
        }
    }};
}

/// Route manager can be in 1 of 4 states -
///  - waiting for a route to be added or removed from the route table
///  - obtaining default routes
///  - applying changes to the route table
///  - shutting down
///
/// Only the _shutting down_ state can be reached from all other states, but during normal
/// operation, the route manager will add all the required routes during startup and will start
/// waiting for changes to the route table.  If any change is detected, it will stop listening for
/// new changes, obtain new default routes and reapply routes that should be routed through the
/// default nodes. Once the routes are reapplied, the route table changes are monitored again.
pub struct RouteManagerImpl {
    routing_table: RoutingTable,
    // Routes that use the default non-tunnel interface
    non_tunnel_routes: HashSet<IpNetwork>,
    v4_tunnel_default_route: Option<data::RouteMessage>,
    v6_tunnel_default_route: Option<data::RouteMessage>,
    applied_routes: BTreeMap<RouteDestination, RouteMessage>,
    v4_default_route: Option<interface::DefaultRoute>,
    v6_default_route: Option<interface::DefaultRoute>,
    update_trigger: BurstGuard,
    default_route_listeners: Vec<mpsc::UnboundedSender<DefaultRouteEvent>>,
    interface_change_listeners: Vec<mpsc::UnboundedSender<super::InterfaceEvent>>,
    check_default_routes_restored: Pin<Box<dyn FusedStream<Item = ()> + Send>>,
    unhandled_default_route_changes: bool,
    primary_interface_monitor: interface::PrimaryInterfaceMonitor,
    interface_change_rx: mpsc::UnboundedReceiver<interface::InterfaceEvent>,
}

impl RouteManagerImpl {
    /// Create new route manager
    #[allow(clippy::unused_async)]
    pub(crate) async fn new(
        manage_tx: Weak<tokio::sync::mpsc::UnboundedSender<RouteManagerCommand>>,
    ) -> Result<Self> {
        let (primary_interface_monitor, interface_change_rx) =
            interface::PrimaryInterfaceMonitor::new();
        let routing_table = RoutingTable::new().map_err(Error::RoutingTable)?;

        let update_trigger = BurstGuard::new(
            BURST_BUFFER_PERIOD,
            BURST_LONGEST_BUFFER_PERIOD,
            move || {
                let Some(manage_tx) = manage_tx.upgrade() else {
                    return;
                };
                let _ = manage_tx.send(RouteManagerCommand::RefreshRoutes);
            },
        );

        Ok(Self {
            routing_table,
            non_tunnel_routes: HashSet::new(),
            v4_tunnel_default_route: None,
            v6_tunnel_default_route: None,
            applied_routes: BTreeMap::new(),
            v4_default_route: None,
            v6_default_route: None,
            update_trigger,
            default_route_listeners: vec![],
            interface_change_listeners: vec![],
            check_default_routes_restored: Box::pin(futures::stream::pending()),
            unhandled_default_route_changes: false,
            primary_interface_monitor,
            interface_change_rx,
        })
    }

    pub(crate) async fn run(mut self, mut manage_rx: mpsc::UnboundedReceiver<RouteManagerCommand>) {
        // Initialize default routes
        // NOTE: This isn't race-free, as we're not listening for route changes before initializing
        self.update_best_default_route(interface::Family::V4)
            .unwrap_or_else(|error| {
                tracing::error!(
                    "{}",
                    error.display_chain_with_msg("Failed to get initial default v4 route")
                );
                false
            });
        self.update_best_default_route(interface::Family::V6)
            .unwrap_or_else(|error| {
                tracing::error!(
                    "{}",
                    error.display_chain_with_msg("Failed to get initial default v6 route")
                );
                false
            });

        self.debug_offline();

        let mut completion_tx = None;

        loop {
            nym_common::detect_flood!();

            tokio::select! {
                biased;
                route_message = self.routing_table.next_message().fuse() => {
                    self.handle_route_message(route_message);
                }

                _ = self.check_default_routes_restored.next() => {
                    nym_common::detect_flood!();

                    if self.check_default_routes_restored.is_terminated() {
                        continue;
                    }
                    if self.try_restore_default_routes().await {
                        tracing::debug!("Unscoped routes were already restored");
                        self.check_default_routes_restored = Box::pin(futures::stream::pending());
                    }
                }

                _event = self.interface_change_rx.recv() => {
                    self.update_trigger.trigger();
                }

                command = manage_rx.recv() => {
                    match command {
                        Some(RouteManagerCommand::Shutdown(tx)) => {
                            completion_tx = Some(tx);
                            break;
                        },

                        Some(RouteManagerCommand::NewDefaultRouteListener(tx)) => {
                            let (events_tx, events_rx) = mpsc::unbounded_channel();
                            self.default_route_listeners.push(events_tx);
                            let _ = tx.send(events_rx);
                        }
                        Some(RouteManagerCommand::GetDefaultRoutes(tx)) => {
                            let v4_route = self.v4_default_route.clone();
                            let v6_route = self.v6_default_route.clone();
                            let _ = tx.send((v4_route, v6_route));
                        }
                        Some(RouteManagerCommand::GetDefaultGateway(tx)) => {
                            let mut v4_gateway = None;
                            let mut v6_gateway = None;

                            if let Some(v4_route) = &self.v4_default_route {
                                v4_gateway = self.get_gateway_link_address(v4_route.router_ip).await;
                            }
                            if let Some(v6_route) = &self.v6_default_route {
                                v6_gateway = self.get_gateway_link_address(v6_route.router_ip).await;
                            }
                            let _ = tx.send((v4_gateway, v6_gateway));
                        }

                        Some(RouteManagerCommand::AddRoutes(routes, tx)) => {
                            if !self.check_default_routes_restored.is_terminated() {
                                tracing::debug!("Cancelling restoration of default routes");
                                self.check_default_routes_restored = Box::pin(futures::stream::pending());
                            }
                            tracing::debug!("Adding routes: {routes:?}");
                            let _ = tx.send(self.add_required_routes(routes).await);
                        }
                        Some(RouteManagerCommand::ClearRoutes) => {
                            if let Err(err) = self.cleanup_routes().await {
                                tracing::error!("Failed to clean up rotues: {err}");
                            }
                        },

                        Some(RouteManagerCommand::NewInterfaceChangeListener(tx)) => {
                            let (events_tx, events_rx) = mpsc::unbounded_channel();
                            self.interface_change_listeners.push(events_tx);
                            let _ = tx.send(events_rx);
                        }

                        Some(RouteManagerCommand::RefreshRoutes) => {
                            if let Err(error) = self.refresh_routes().await {
                                tracing::error!("Failed to refresh routes: {error}");
                            }
                        },
                        None => {
                            break;
                        }
                    }
                },
            };
        }

        if let Err(err) = self.cleanup_routes().await {
            tracing::error!("Failed to clean up routing table when shutting down: {err}");
        }

        self.update_trigger.stop_nonblocking();

        if let Some(tx) = completion_tx {
            let _ = tx.send(());
        }
    }

    async fn get_gateway_link_address(&mut self, gateway_ip: IpAddr) -> Option<Gateway> {
        let gateway_msg = RouteMessage::new_route(Destination::Host(gateway_ip));

        if let Ok(Some(msg)) = self.routing_table.get_route(&gateway_msg).await {
            if let Some(gateway) = msg
                .gateway()
                .and_then(|gateway| gateway.as_link_addr())
                .and_then(|addr| addr.addr())
            {
                let mac_address = MacAddress::from(gateway);
                return Some(Gateway {
                    ip_address: gateway_ip,
                    mac_address,
                });
            }
        }
        None
    }

    async fn add_required_routes(&mut self, required_routes: HashSet<RequiredRoute>) -> Result<()> {
        let mut routes_to_apply = vec![];

        for route in required_routes {
            match route.node {
                NetNode::DefaultNode => {
                    self.non_tunnel_routes.insert(route.prefix);
                }

                NetNode::RealNode(node) => {
                    let mut applied_route = Route::new(node, route.prefix);
                    applied_route.mtu = route.mtu.map(u32::from);
                    routes_to_apply.push(applied_route);
                }
            }
        }

        // Map all interfaces to their link addresses
        let interface_link_addrs =
            interface::get_interface_link_addresses().map_err(Error::FetchLinkAddresses)?;

        // Add routes not using the default interface
        for route in routes_to_apply {
            let mut message = if let Some(ref device) = route.node.device {
                // If we specify route by interface name, use the link address of the given
                // interface
                match interface_link_addrs.get(device) {
                    Some(link_addr) => RouteMessage::new_route(Destination::from(route.prefix))
                        .set_gateway_sockaddr(*link_addr),
                    None => {
                        tracing::error!("Route with unknown device: {route:?}, {device}");
                        continue;
                    }
                }
            } else {
                tracing::error!("Specifying gateway by IP rather than device is unimplemented");
                continue;
            };

            if let Some(mtu) = route.mtu {
                message = message.set_mtu(mtu);
            }

            // Default routes are a special case: We must apply it after replacing the current
            // default route with an ifscope route.
            if route.prefix.prefix() == 0 {
                if route.prefix.is_ipv4() {
                    self.v4_tunnel_default_route = Some(message);
                } else {
                    self.v6_tunnel_default_route = Some(message);
                }
                continue;
            }

            // Add route
            self.add_route_with_record(message).await?;
        }

        self.apply_tunnel_default_route().await?;

        // Add routes that use the default interface
        if let Err(error) = self.apply_non_tunnel_routes().await {
            self.non_tunnel_routes.clear();
            return Err(error);
        }

        Ok(())
    }

    fn handle_route_message(
        &mut self,
        message: std::result::Result<RouteSocketMessage, watch::Error>,
    ) {
        nym_common::detect_flood!();

        match message {
            Ok(RouteSocketMessage::DeleteRoute(route)) => {
                // Forget about applied route, if relevant
                match RouteDestination::try_from(&route).map_err(Error::InvalidData) {
                    Ok(destination) => {
                        self.applied_routes.remove(&destination);
                    }
                    Err(err) => {
                        tracing::error!("Failed to process deleted route: {err}");
                    }
                }
                if route.errno() != 0 {
                    return;
                }
                if route.is_default().unwrap_or(true) {
                    self.unhandled_default_route_changes = true;
                }
                self.update_trigger.trigger();
            }
            Ok(RouteSocketMessage::AddRoute(route))
            | Ok(RouteSocketMessage::ChangeRoute(route)) => {
                if route.errno() != 0 {
                    return;
                }
                if route.is_default().unwrap_or(true) {
                    self.unhandled_default_route_changes = true;
                }
                self.update_trigger.trigger();
            }
            Ok(RouteSocketMessage::AddAddress(_) | RouteSocketMessage::DeleteAddress(_)) => {
                self.update_trigger.trigger();
            }
            Ok(RouteSocketMessage::Interface(iface)) => {
                let Ok(mtu) = u16::try_from(iface.mtu()) else {
                    tracing::warn!("Invalid mtu for interface: {}", iface.index());
                    return;
                };

                self.interface_change_listeners.retain(|tx| {
                    tx.send(super::InterfaceEvent {
                        interface_index: iface.index(),
                        mtu,
                    })
                    .is_ok()
                });
            }
            // ignore all other message types
            Ok(_) => {}
            Err(err) => {
                tracing::error!(
                    "{}",
                    err.display_chain_with_msg(
                        "Failed to receive a message from the routing table"
                    )
                );
            }
        }
    }

    /// Handle changes to the routing table:
    /// * Replace the unscoped default route with a default route for the tunnel interface (i.e.,
    ///   one whose gateway is set to the link address of the tunnel interface).
    /// * At the same time, update the route used by non-tunnel interfaces to reach the relay/VPN
    ///   server. The gateway of the relay route is set to the first interface in the network
    ///   service order that has a working ifscoped default route.
    async fn refresh_routes(&mut self) -> Result<()> {
        nym_common::detect_flood!();

        self.update_best_default_route(interface::Family::V4)?;
        self.update_best_default_route(interface::Family::V6)?;

        self.debug_offline();

        if !self.unhandled_default_route_changes {
            return Ok(());
        }

        // Remove any existing ifscope route that we've added
        self.remove_applied_routes(|route| {
            route.is_ifscope() && route.is_default().unwrap_or(false)
        })
        .await;

        // Substitute route with a tunnel route
        self.apply_tunnel_default_route().await?;

        // Update routes using default interface
        self.apply_non_tunnel_routes().await?;

        self.unhandled_default_route_changes = false;

        Ok(())
    }

    fn debug_offline(&self) {
        if self.v4_default_route.is_none() && self.v6_default_route.is_none() {
            self.primary_interface_monitor.debug();
        }
    }

    /// Figure out what the best default routes to use are, and send updates to default route change
    /// subscribers. The "best routes" are used by the tunnel device to send packets to the VPN
    /// relay.
    ///
    /// The "best route" is determined by the first interface in the network service order that has
    /// a valid IP address and gateway.
    ///
    /// On success, the function returns whether the previously known best default changed.
    fn update_best_default_route(&mut self, family: interface::Family) -> Result<bool> {
        let best_route = self.primary_interface_monitor.get_route(family);

        let current_route = get_current_best_default_route!(self, family);

        tracing::trace!("Best route ({family:?}): {best_route:?}");
        if best_route == *current_route {
            return Ok(false);
        }

        self.unhandled_default_route_changes = true;

        let old_pair = current_route
            .as_ref()
            .map(|r| (r.interface_index, r.router_ip));
        let new_pair = best_route
            .as_ref()
            .map(|r| (r.interface_index, r.router_ip));
        tracing::debug!("Best default route ({family}) changed from {old_pair:?} to {new_pair:?}");
        let _ = std::mem::replace(current_route, best_route);

        let changed = current_route.is_some();
        self.notify_default_route_listeners(family, changed);
        Ok(true)
    }

    fn notify_default_route_listeners(&mut self, family: interface::Family, changed: bool) {
        // Notify default route listeners
        let event = match (family, changed) {
            (interface::Family::V4, true) => DefaultRouteEvent::AddedOrChangedV4,
            (interface::Family::V6, true) => DefaultRouteEvent::AddedOrChangedV6,
            (interface::Family::V4, false) => DefaultRouteEvent::RemovedV4,
            (interface::Family::V6, false) => DefaultRouteEvent::RemovedV6,
        };
        self.default_route_listeners
            .retain(|tx| tx.send(event).is_ok());
    }

    /// Replace the default routes with an ifscope route, and
    /// add a new default tunnel route.
    async fn apply_tunnel_default_route(&mut self) -> Result<()> {
        // As long as the relay route has a way of reaching the internet, we'll want to add a tunnel
        // route for both IPv4 and IPv6.
        // NOTE: This is incorrect. We're assuming that any "default destination" is used for
        // tunneling.
        let (v4_conn, v6_conn) = self
            .non_tunnel_routes
            .iter()
            .fold((false, false), |(v4, v6), route| {
                (v4 || route.is_ipv4(), v6 || route.is_ipv6())
            });
        let relay_route_is_valid = (v4_conn && self.v4_default_route.is_some())
            || (v6_conn && self.v6_default_route.is_some());

        if !relay_route_is_valid {
            return Ok(());
        }

        for tunnel_route in [
            self.v4_tunnel_default_route.clone(),
            self.v6_tunnel_default_route.clone(),
        ] {
            let tunnel_route = match tunnel_route {
                Some(route) => route,
                None => continue,
            };
            let family = if tunnel_route.is_ipv4() {
                interface::Family::V4
            } else {
                interface::Family::V6
            };

            // Replace the default route with an ifscope route
            self.replace_with_scoped_route(family).await?;

            // Make sure there is really no other unscoped default route
            let mut msg = RouteMessage::new_route(family.default_network().into());
            msg = msg.set_gateway_route(true);
            let old_route = self.routing_table.get_route(&msg).await;
            if let Ok(Some(old_route)) = old_route {
                let tun_gateway_link_addr =
                    tunnel_route.gateway().and_then(|addr| addr.as_link_addr());
                let current_link_addr = old_route.gateway().and_then(|addr| addr.as_link_addr());
                if current_link_addr
                    .map(|addr| Some(addr) != tun_gateway_link_addr)
                    .unwrap_or(true)
                {
                    tracing::trace!("Removing existing unscoped default route");
                    let _ = self.routing_table.delete_route(&msg).await;
                } else if !old_route.is_ifscope() {
                    // NOTE: Skipping route
                    continue;
                }
            }

            tracing::debug!("Adding default route for tunnel");
            self.add_route_with_record(tunnel_route).await?;
        }

        Ok(())
    }

    /// Update/add routes that use the default non-tunnel interface. If some applied destination is
    /// a default route, this function replaces the non-tunnel default route with an ifscope route.
    async fn apply_non_tunnel_routes(&mut self) -> Result<()> {
        let v4_gateway = self
            .v4_default_route
            .as_ref()
            .map(|route| SocketAddr::new(route.router_ip, 0));
        let v6_gateway = self
            .v6_default_route
            .as_ref()
            .map(|route| SocketAddr::new(route.router_ip, 0));

        // Reapply routes that use the default (non-tunnel) node
        for dest in self.non_tunnel_routes.clone() {
            let gateway = if dest.is_ipv4() {
                v4_gateway
            } else {
                v6_gateway
            };
            let gateway = match gateway {
                Some(gateway) => gateway,
                None => continue,
            };
            let route =
                RouteMessage::new_route(Destination::Network(dest)).set_gateway_addr(gateway);

            if let Some(dest) = self
                .applied_routes
                .keys()
                .find(|applied_dest| applied_dest.network == dest)
                .cloned()
            {
                let _ = self.routing_table.delete_route(&route).await;
                self.applied_routes.remove(&dest);
            }

            self.add_route_with_record(route).await?;
        }

        Ok(())
    }

    /// Replace a known default route with an ifscope route.
    async fn replace_with_scoped_route(&mut self, family: interface::Family) -> Result<()> {
        let Some(default_route) = get_current_best_default_route!(self, family) else {
            return Ok(());
        };

        let interface_index = default_route.interface_index;
        let default_route = RouteMessage::from(default_route.clone());
        let new_route = default_route.set_ifscope(interface_index);

        tracing::trace!("Setting ifscope: {new_route:?}");

        self.add_route_with_record(new_route).await
    }

    async fn add_route_with_record(&mut self, route: RouteMessage) -> Result<()> {
        let destination = RouteDestination::try_from(&route).map_err(Error::InvalidData)?;

        let add_result = self
            .routing_table
            .add_route(&route)
            .await
            .map_err(Error::AddRoute)?;

        if add_result == watch::AddResult::Ok {
            self.applied_routes.insert(destination, route);
        }
        Ok(())
    }

    async fn cleanup_routes(&mut self) -> Result<()> {
        self.remove_applied_routes(|_| true).await;

        // We have already removed the applied default routes
        self.v4_tunnel_default_route = None;
        self.v6_tunnel_default_route = None;

        self.try_restore_default_routes().await;

        self.check_default_routes_restored = Self::create_default_route_check_timer();

        self.non_tunnel_routes.clear();

        Ok(())
    }

    /// Remove all applied routes for which `filter` returns true
    async fn remove_applied_routes(&mut self, filter: impl Fn(&RouteMessage) -> bool) {
        let mut deleted_routes = vec![];

        self.applied_routes.retain(|_dest, route| {
            if filter(route) {
                deleted_routes.push(route.clone());
                return false;
            }
            true
        });

        for route in deleted_routes {
            tracing::trace!("Removing route: {route:?}");
            match self.routing_table.delete_route(&route).await {
                Ok(_) | Err(watch::Error::RouteNotFound) | Err(watch::Error::Unreachable) => (),
                Err(err) => {
                    tracing::error!("Failed to remove relay route: {err:?}");
                }
            }
        }
    }

    /// FIXME: Hack. Restoring the default routes during cleanup sometimes fails, so repeatedly try
    /// until we have restored unscoped default routes. This function produces a timer for
    /// exponential backoff.
    fn create_default_route_check_timer() -> Pin<Box<dyn FusedStream<Item = ()> + Send>> {
        const RESTORE_HACK_INITIAL_INTERVAL: Duration = Duration::from_millis(500);
        const RESTORE_HACK_INTERVAL_MULTIPLIER: u32 = 5;
        const RESTORE_HACK_MAX_ATTEMPTS: u32 = 3;

        Box::pin(futures::stream::unfold(0, |attempt| async move {
            if attempt >= RESTORE_HACK_MAX_ATTEMPTS {
                return None;
            }

            let next_interval = RESTORE_HACK_INITIAL_INTERVAL
                * RESTORE_HACK_INTERVAL_MULTIPLIER.saturating_pow(attempt);
            tokio::time::sleep(next_interval).await;

            Some(((), attempt + 1))
        }))
    }

    /// Add back unscoped default routes, if they are still missing. This function returns
    /// true when no routes had to be added.
    async fn try_restore_default_routes(&mut self) -> bool {
        self.restore_default_route(interface::Family::V4).await
            && self.restore_default_route(interface::Family::V6).await
    }

    /// Add back unscoped default route for the given `family`, if it is still missing. This
    /// function returns true when no route had to be added.
    async fn restore_default_route(&mut self, family: interface::Family) -> bool {
        let Some(desired_default_route) = self.primary_interface_monitor.get_route(family) else {
            return true;
        };
        let desired_default_route = RouteMessage::from(desired_default_route);

        let current_default_route = RouteMessage::new_route(family.default_network().into());
        if let Ok(Some(current_default)) =
            self.routing_table.get_route(&current_default_route).await
        {
            // We're done if the route we're looking for is already here
            if route_matches_interface(&current_default, &desired_default_route) {
                return true;
            }
            let _ = self
                .routing_table
                .delete_route(&current_default_route)
                .await;
        };

        if let Err(error) = self.routing_table.add_route(&desired_default_route).await {
            tracing::trace!("Failed to add unscoped default {family} route: {error}");
        }

        self.update_trigger.trigger();

        false
    }
}

fn route_matches_interface(default_route: &RouteMessage, interface_route: &RouteMessage) -> bool {
    default_route.gateway_ip() == interface_route.gateway_ip()
        && default_route.interface_index() == interface_route.interface_index()
}
