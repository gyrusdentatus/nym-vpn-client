// Copyright 2025 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use nym_vpn_lib_types::{
    ActionAfterDisconnect, ClientErrorReason, ConnectionData, ForgetAccountError, Gateway,
    MixnetConnectionData, RegisterDeviceError, StoreAccountError, SyncAccountError,
    SyncDeviceError, TunnelConnectionData, TunnelState, VpnApiErrorResponse,
    WireguardConnectionData, WireguardNode,
};

use crate::{
    tunnel_connection_data::{
        Mixnet as ProtoMixnetConnectionDataVariant, State as ProtoTunnelConnectionDataState,
        Wireguard as ProtoWireguardConnectionDataVariant,
    },
    tunnel_state::{
        ActionAfterDisconnect as ProtoActionAfterDisconnect, Connected as ProtoConnected,
        Connecting as ProtoConnecting, Disconnected as ProtoDisconnected,
        Disconnecting as ProtoDisconnecting, Error as ProtoError, ErrorStateReason,
        Offline as ProtoOffline, State as ProtoState,
    },
    Address as ProtoAddress, ConnectionData as ProtoConnectionData,
    ForgetAccountError as ProtoForgetAccountError, Gateway as ProtoGateway,
    MixnetConnectionData as ProtoMixnetConnectionData,
    RegisterDeviceError as ProtoRegisterDeviceError, StoreAccountError as ProtoStoreAccountError,
    SyncAccountError as ProtoSyncAccountError, SyncDeviceError as ProtoSyncDeviceError,
    TunnelConnectionData as ProtoTunnelConnectionData, TunnelState as ProtoTunnelState,
    VpnApiErrorResponse as ProtoVpnApiErrorResponse,
    WireguardConnectionData as ProtoWireguardConnectionData, WireguardNode as ProtoWireguardNode,
};

impl From<ActionAfterDisconnect> for ProtoActionAfterDisconnect {
    fn from(value: ActionAfterDisconnect) -> Self {
        match value {
            ActionAfterDisconnect::Error => Self::Error,
            ActionAfterDisconnect::Nothing => Self::Nothing,
            ActionAfterDisconnect::Offline => Self::Offline,
            ActionAfterDisconnect::Reconnect => Self::Reconnect,
        }
    }
}

impl From<ClientErrorReason> for ProtoError {
    fn from(value: ClientErrorReason) -> Self {
        match value {
            ClientErrorReason::Firewall => ProtoError {
                reason: ErrorStateReason::Firewall.into(),
                detail: None,
            },
            ClientErrorReason::Routing => ProtoError {
                reason: ErrorStateReason::Routing.into(),
                detail: None,
            },
            ClientErrorReason::SameEntryAndExitGateway => ProtoError {
                reason: ErrorStateReason::SameEntryAndExitGateway.into(),
                detail: None,
            },
            ClientErrorReason::InvalidEntryGatewayCountry => ProtoError {
                reason: ErrorStateReason::InvalidEntryGatewayCountry.into(),
                detail: None,
            },
            ClientErrorReason::InvalidExitGatewayCountry => ProtoError {
                reason: ErrorStateReason::InvalidExitGatewayCountry.into(),
                detail: None,
            },
            ClientErrorReason::MaxDevicesReached => ProtoError {
                reason: ErrorStateReason::MaxDevicesReached.into(),
                detail: None,
            },
            ClientErrorReason::BandwidthExceeded => ProtoError {
                reason: ErrorStateReason::BandwidthExceeded.into(),
                detail: None,
            },
            ClientErrorReason::SubscriptionExpired => ProtoError {
                reason: ErrorStateReason::SubscriptionExpired.into(),
                detail: None,
            },
            ClientErrorReason::Dns(detail) => ProtoError {
                reason: ErrorStateReason::Dns.into(),
                detail,
            },
            ClientErrorReason::Api(detail) => ProtoError {
                reason: ErrorStateReason::Api.into(),
                detail,
            },
            ClientErrorReason::Internal(detail) => ProtoError {
                reason: ErrorStateReason::Internal.into(),
                detail,
            },
        }
    }
}

impl From<StoreAccountError> for ProtoStoreAccountError {
    fn from(value: StoreAccountError) -> Self {
        match value {
            StoreAccountError::Storage(err) => ProtoStoreAccountError {
                error_detail: Some(crate::store_account_error::ErrorDetail::StorageError(err)),
            },
            StoreAccountError::GetAccountEndpointFailure(vpn_api_endpoint_failure) => {
                ProtoStoreAccountError {
                    error_detail: Some(crate::store_account_error::ErrorDetail::ErrorResponse(
                        vpn_api_endpoint_failure.into(),
                    )),
                }
            }
            StoreAccountError::UnexpectedResponse(err) => ProtoStoreAccountError {
                error_detail: Some(crate::store_account_error::ErrorDetail::UnexpectedResponse(
                    err,
                )),
            },
        }
    }
}

impl From<SyncAccountError> for ProtoSyncAccountError {
    fn from(value: SyncAccountError) -> Self {
        match value {
            SyncAccountError::NoAccountStored => ProtoSyncAccountError {
                error_detail: Some(crate::sync_account_error::ErrorDetail::NoAccountStored(
                    true,
                )),
            },
            SyncAccountError::SyncAccountEndpointFailure(vpn_api_endpoint_failure) => {
                ProtoSyncAccountError {
                    error_detail: Some(crate::sync_account_error::ErrorDetail::ErrorResponse(
                        vpn_api_endpoint_failure.into(),
                    )),
                }
            }
            SyncAccountError::UnexpectedResponse(err) => ProtoSyncAccountError {
                error_detail: Some(crate::sync_account_error::ErrorDetail::UnexpectedResponse(
                    err,
                )),
            },
            SyncAccountError::Internal(err) => ProtoSyncAccountError {
                error_detail: Some(crate::sync_account_error::ErrorDetail::Internal(err)),
            },
        }
    }
}

impl From<SyncDeviceError> for ProtoSyncDeviceError {
    fn from(value: SyncDeviceError) -> Self {
        match value {
            SyncDeviceError::NoAccountStored => ProtoSyncDeviceError {
                error_detail: Some(crate::sync_device_error::ErrorDetail::NoAccountStored(true)),
            },
            SyncDeviceError::NoDeviceStored => ProtoSyncDeviceError {
                error_detail: Some(crate::sync_device_error::ErrorDetail::NoDeviceStored(true)),
            },
            SyncDeviceError::SyncDeviceEndpointFailure(vpn_api_endpoint_failure) => {
                ProtoSyncDeviceError {
                    error_detail: Some(crate::sync_device_error::ErrorDetail::ErrorResponse(
                        vpn_api_endpoint_failure.into(),
                    )),
                }
            }
            SyncDeviceError::UnexpectedResponse(err) => ProtoSyncDeviceError {
                error_detail: Some(crate::sync_device_error::ErrorDetail::UnexpectedResponse(
                    err,
                )),
            },
            SyncDeviceError::Internal(err) => ProtoSyncDeviceError {
                error_detail: Some(crate::sync_device_error::ErrorDetail::Internal(err)),
            },
        }
    }
}

impl From<RegisterDeviceError> for ProtoRegisterDeviceError {
    fn from(value: RegisterDeviceError) -> Self {
        match value {
            RegisterDeviceError::NoAccountStored => ProtoRegisterDeviceError {
                error_detail: Some(crate::register_device_error::ErrorDetail::NoAccountStored(
                    true,
                )),
            },
            RegisterDeviceError::NoDeviceStored => ProtoRegisterDeviceError {
                error_detail: Some(crate::register_device_error::ErrorDetail::NoDeviceStored(
                    true,
                )),
            },
            RegisterDeviceError::RegisterDeviceEndpointFailure(vpn_api_endpoint_failure) => {
                ProtoRegisterDeviceError {
                    error_detail: Some(crate::register_device_error::ErrorDetail::ErrorResponse(
                        vpn_api_endpoint_failure.into(),
                    )),
                }
            }
            RegisterDeviceError::UnexpectedResponse(err) => ProtoRegisterDeviceError {
                error_detail: Some(
                    crate::register_device_error::ErrorDetail::UnexpectedResponse(err),
                ),
            },
            RegisterDeviceError::Internal(err) => ProtoRegisterDeviceError {
                error_detail: Some(crate::register_device_error::ErrorDetail::Internal(err)),
            },
        }
    }
}

impl From<ForgetAccountError> for ProtoForgetAccountError {
    fn from(value: ForgetAccountError) -> Self {
        match value {
            ForgetAccountError::RegistrationInProgress => Self {
                error_detail: Some(
                    crate::forget_account_error::ErrorDetail::RegistrationInProgress(true),
                ),
            },
            ForgetAccountError::UpdateDeviceErrorResponse(vpn_api_endpoint_failure) => Self {
                error_detail: Some(crate::forget_account_error::ErrorDetail::ErrorResponse(
                    vpn_api_endpoint_failure.into(),
                )),
            },
            ForgetAccountError::UnexpectedResponse(err) => Self {
                error_detail: Some(
                    crate::forget_account_error::ErrorDetail::UnexpectedResponse(err),
                ),
            },
            ForgetAccountError::RemoveAccount(err) => Self {
                error_detail: Some(crate::forget_account_error::ErrorDetail::RemoveAccount(err)),
            },
            ForgetAccountError::RemoveDeviceKeys(err) => Self {
                error_detail: Some(crate::forget_account_error::ErrorDetail::RemoveDeviceKeys(
                    err,
                )),
            },
            ForgetAccountError::ResetCredentialStorage(err) => Self {
                error_detail: Some(
                    crate::forget_account_error::ErrorDetail::ResetCredentialStore(err),
                ),
            },
            ForgetAccountError::RemoveAccountFiles(err) => Self {
                error_detail: Some(
                    crate::forget_account_error::ErrorDetail::RemoveAccountFiles(err),
                ),
            },
            ForgetAccountError::InitDeviceKeys(err) => Self {
                error_detail: Some(crate::forget_account_error::ErrorDetail::InitDeviceKeys(
                    err,
                )),
            },
        }
    }
}

impl From<VpnApiErrorResponse> for ProtoVpnApiErrorResponse {
    fn from(value: VpnApiErrorResponse) -> Self {
        Self {
            message: value.message,
            message_id: value.message_id,
            code_reference_id: value.code_reference_id,
        }
    }
}

impl From<TunnelState> for ProtoTunnelState {
    fn from(value: TunnelState) -> ProtoTunnelState {
        let proto_state: ProtoState = match value {
            TunnelState::Disconnected => ProtoState::Disconnected(ProtoDisconnected {}),
            TunnelState::Connecting { connection_data } => {
                ProtoState::Connecting(ProtoConnecting {
                    connection_data: connection_data.map(ProtoConnectionData::from),
                })
            }
            TunnelState::Connected { connection_data } => ProtoState::Connected(ProtoConnected {
                connection_data: Some(ProtoConnectionData::from(connection_data)),
            }),
            TunnelState::Disconnecting { after_disconnect } => {
                ProtoState::Disconnecting(ProtoDisconnecting {
                    after_disconnect: ProtoActionAfterDisconnect::from(after_disconnect) as i32,
                })
            }
            TunnelState::Offline { reconnect } => ProtoState::Offline(ProtoOffline { reconnect }),
            TunnelState::Error(reason) => ProtoState::Error(ProtoError::from(reason)),
        };

        ProtoTunnelState {
            state: Some(proto_state),
        }
    }
}

impl From<WireguardNode> for ProtoWireguardNode {
    fn from(value: WireguardNode) -> ProtoWireguardNode {
        ProtoWireguardNode {
            public_key: value.public_key,
            endpoint: value.endpoint.to_string(),
            private_ipv4: value.private_ipv4.to_string(),
            private_ipv6: value.private_ipv6.to_string(),
        }
    }
}

impl From<ConnectionData> for ProtoConnectionData {
    fn from(value: ConnectionData) -> ProtoConnectionData {
        ProtoConnectionData {
            entry_gateway: Some(ProtoGateway::from(value.entry_gateway)),
            exit_gateway: Some(ProtoGateway::from(value.exit_gateway)),
            connected_at: value
                .connected_at
                .map(crate::conversions::prost::offset_datetime_into_proto_timestamp),
            tunnel: Some(ProtoTunnelConnectionData::from(value.tunnel)),
        }
    }
}

impl From<MixnetConnectionData> for ProtoMixnetConnectionData {
    fn from(value: MixnetConnectionData) -> ProtoMixnetConnectionData {
        ProtoMixnetConnectionData {
            nym_address: Some(ProtoAddress {
                nym_address: value.nym_address.to_string(),
                gateway_id: value.nym_address.gateway_id,
            }),
            exit_ipr: Some(ProtoAddress {
                nym_address: value.exit_ipr.to_string(),
                gateway_id: value.exit_ipr.gateway_id,
            }),
            entry_ip: value.entry_ip.to_string(),
            exit_ip: value.exit_ip.to_string(),
            ipv4: value.ipv4.to_string(),
            ipv6: value.ipv6.to_string(),
        }
    }
}

impl From<WireguardConnectionData> for ProtoWireguardConnectionData {
    fn from(value: WireguardConnectionData) -> ProtoWireguardConnectionData {
        ProtoWireguardConnectionData {
            entry: Some(ProtoWireguardNode::from(value.entry)),
            exit: Some(ProtoWireguardNode::from(value.exit)),
        }
    }
}

impl From<TunnelConnectionData> for ProtoTunnelConnectionData {
    fn from(value: TunnelConnectionData) -> ProtoTunnelConnectionData {
        let state = match value {
            TunnelConnectionData::Mixnet(data) => {
                ProtoTunnelConnectionDataState::Mixnet(ProtoMixnetConnectionDataVariant {
                    data: Some(ProtoMixnetConnectionData::from(data)),
                })
            }
            TunnelConnectionData::Wireguard(data) => {
                ProtoTunnelConnectionDataState::Wireguard(ProtoWireguardConnectionDataVariant {
                    data: Some(ProtoWireguardConnectionData::from(data)),
                })
            }
        };

        ProtoTunnelConnectionData { state: Some(state) }
    }
}

impl From<Gateway> for ProtoGateway {
    fn from(value: Gateway) -> Self {
        Self { id: value.id }
    }
}
