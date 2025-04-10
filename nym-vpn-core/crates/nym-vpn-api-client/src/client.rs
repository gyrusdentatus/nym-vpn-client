// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{fmt, net::SocketAddr, time::Duration};

use backon::Retryable;
use nym_credential_proxy_requests::api::v1::ticketbook::models::PartialVerificationKeysResponse;
use nym_http_api_client::{ApiClient, HttpClientError, Params, PathSegments, UserAgent, NO_PARAMS};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::{
    error::{Result, VpnApiClientError},
    request::{
        ApplyFreepassRequestBody, CreateSubscriptionKind, CreateSubscriptionRequestBody,
        RegisterDeviceRequestBody, RequestZkNymRequestBody, UpdateDeviceRequestBody,
        UpdateDeviceRequestStatus,
    },
    response::{
        NymDirectoryGatewayCountriesResponse, NymDirectoryGatewaysResponse, NymVpnAccountResponse,
        NymVpnAccountSummaryResponse, NymVpnDevice, NymVpnDevicesResponse, NymVpnHealthResponse,
        NymVpnSubscription, NymVpnSubscriptionResponse, NymVpnSubscriptionsResponse,
        NymVpnUsagesResponse, NymVpnZkNym, NymVpnZkNymPost, NymVpnZkNymResponse,
        NymWellknownDiscoveryItem, StatusOk,
    },
    routes,
    types::{Device, DeviceStatus, GatewayMinPerformance, GatewayType, VpnApiAccount, VpnApiTime},
};

pub(crate) const DEVICE_AUTHORIZATION_HEADER: &str = "x-device-authorization";

// GET requests can unfortunately take a long time over the mixnet
pub(crate) const NYM_VPN_API_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub struct VpnApiClient {
    inner: nym_http_api_client::Client,
}

impl VpnApiClient {
    pub fn new(base_url: Url, user_agent: UserAgent) -> Result<Self> {
        Self::new_with_resolver_overrides(base_url, user_agent, None)
    }

    pub fn new_with_resolver_overrides(
        base_url: Url,
        user_agent: UserAgent,
        static_addresses: Option<&[SocketAddr]>,
    ) -> Result<Self> {
        nym_http_api_client::Client::builder(base_url.clone())
            .map(|builder| {
                let mut builder = builder
                    .with_user_agent(user_agent)
                    .with_timeout(NYM_VPN_API_TIMEOUT);

                if let Some(domain) = base_url.domain() {
                    match static_addresses {
                        Some(static_addresses) if !static_addresses.is_empty() => {
                            tracing::info!(
                                "Enabling DNS resolver overrides: {:?}", static_addresses
                            );
                            builder = builder.resolve_to_addrs(domain, static_addresses);
                        }
                        Some(_) => {
                            tracing::warn!(
                                "Not enabling DNS resolver overrides because static addresses are empty"
                            );
                        }
                        None => {
                            tracing::info!(
                                "Not enabling DNS resolver overrides because static addresses are not set"
                            );
                        }
                    }
                } else {
                    tracing::info!(
                        "Not enabling DNS resolver overrides because domain is not present in base URL"
                    );
                }

                builder
            })
            .and_then(|builder| builder.build())
            .map(|c| Self { inner: c })
            .map_err(VpnApiClientError::FailedToCreateVpnApiClient)
    }

    pub fn swap_inner_client(&mut self, client: &VpnApiClient) {
        self.inner = client.inner.clone();
    }

    pub fn current_url(&self) -> &Url {
        self.inner.current_url()
    }

    fn use_remote_time(remote_time: VpnApiTime) -> bool {
        if remote_time.is_almost_same() {
            tracing::debug!("{remote_time}");
            false
        } else if remote_time.is_acceptable_synced() {
            tracing::info!("{remote_time}");
            false
        } else {
            tracing::warn!(
                "The time skew between the local and remote time is too large, we'll use remote instead for JWT ({remote_time})."
            );
            true
        }
    }

    async fn sync_with_remote_time(&self) -> Result<Option<VpnApiTime>> {
        let time_before = OffsetDateTime::now_utc();
        let remote_timestamp = self.get_health().await?.timestamp_utc;
        let time_after = OffsetDateTime::now_utc();

        let remote_time =
            VpnApiTime::from_remote_timestamp(time_before, remote_timestamp, time_after);

        if Self::use_remote_time(remote_time) {
            Ok(Some(remote_time))
        } else {
            Ok(None)
        }
    }

    async fn get_query<T, E>(
        &self,
        path: PathSegments<'_>,
        account: &VpnApiAccount,
        device: Option<&Device>,
        jwt: Option<VpnApiTime>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        E: fmt::Display + DeserializeOwned,
    {
        let request = self
            .inner
            .create_get_request(path, NO_PARAMS)
            .bearer_auth(account.jwt(jwt).to_string());

        let request = match device {
            Some(device) => request.header(
                DEVICE_AUTHORIZATION_HEADER,
                format!("Bearer {}", device.jwt(jwt)),
            ),
            None => request,
        };
        let response = request.send().await?;
        nym_http_api_client::parse_response(response, false).await
    }

    async fn get_authorized<T, E>(
        &self,
        path: PathSegments<'_>,
        account: &VpnApiAccount,
        device: Option<&Device>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        E: fmt::Display + DeserializeOwned,
    {
        match self.get_query::<T, E>(path, account, device, None).await {
            Ok(response) => Ok(response),
            Err(err) => {
                if let HttpClientError::EndpointFailure { status: _, error } = &err {
                    if jwt_error(&error.to_string()) {
                        tracing::warn!("Encountered possible JWT error: {error}. Retrying query with remote time");
                        if let Ok(Some(jwt)) =
                            self.sync_with_remote_time().await.inspect_err(|err| {
                                tracing::error!(
                                    "Failed to get remote time: {err}. Not retring anymore"
                                )
                            })
                        {
                            // retry with remote vpn api time, and return that only if it succeeds,
                            // otherwise return the initial error
                            let res = self.get_query(path, account, device, Some(jwt)).await;
                            if res.is_ok() {
                                return res;
                            }
                        }
                    }
                }
                Err(err)
            }
        }
    }

    #[allow(unused)]
    async fn get_authorized_debug<T, E>(
        &self,
        path: PathSegments<'_>,
        account: &VpnApiAccount,
        device: Option<&Device>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        E: fmt::Display + DeserializeOwned,
    {
        let request = self
            .inner
            .create_get_request(path, NO_PARAMS)
            .bearer_auth(account.jwt(None).to_string());

        let request = match device {
            Some(device) => request.header(
                DEVICE_AUTHORIZATION_HEADER,
                format!("Bearer {}", device.jwt(None)),
            ),
            None => request,
        };

        let response = request.send().await?;
        let status = response.status();
        tracing::info!("Response status: {:#?}", status);

        // TODO: support this mode in the upstream crate

        if status.is_success() {
            let response_text = response.text().await?;
            tracing::info!("Response: {:#?}", response_text);
            let response_json = serde_json::from_str(&response_text)
                .map_err(|e| HttpClientError::GenericRequestFailure(e.to_string()))?;
            Ok(response_json)
        //} else if status == reqwest::StatusCode::NOT_FOUND {
        //    Err(HttpClientError::NotFound)
        } else {
            let Ok(response_text) = response.text().await else {
                return Err(HttpClientError::RequestFailure { status });
            };

            tracing::info!("Response: {:#?}", response_text);

            if let Ok(request_error) = serde_json::from_str(&response_text) {
                Err(HttpClientError::EndpointFailure {
                    status,
                    error: request_error,
                })
            } else {
                Err(HttpClientError::GenericRequestFailure(response_text))
            }
        }
    }

    async fn get_json_with_retry<T, K, V, E>(
        &self,
        path: PathSegments<'_>,
        params: Params<'_, K, V>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        for<'a> T: Deserialize<'a>,
        K: AsRef<str> + Sync,
        V: AsRef<str> + Sync,
        E: fmt::Display + fmt::Debug + DeserializeOwned,
    {
        let response = (|| async { self.inner.get_json(path, params).await })
            .retry(backon::ConstantBuilder::default())
            .notify(|err: &HttpClientError<E>, dur: Duration| {
                tracing::warn!("Failed to get JSON: {}", err);
                tracing::warn!("retrying {:?} after {:?}", err, dur);
            })
            .await?;
        Ok(response)
    }

    async fn post_query<T, B, E>(
        &self,
        path: PathSegments<'_>,
        json_body: &B,
        account: &VpnApiAccount,
        device: Option<&Device>,
        jwt: Option<VpnApiTime>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        B: Serialize,
        E: fmt::Display + DeserializeOwned,
    {
        let request = self
            .inner
            .create_post_request(path, NO_PARAMS, json_body)
            .bearer_auth(account.jwt(jwt).to_string());

        let request = match device {
            Some(device) => request.header(
                DEVICE_AUTHORIZATION_HEADER,
                format!("Bearer {}", device.jwt(jwt)),
            ),
            None => request,
        };
        let response = request.send().await?;
        nym_http_api_client::parse_response(response, false).await
    }

    async fn post_authorized<T, B, E>(
        &self,
        path: PathSegments<'_>,
        json_body: &B,
        account: &VpnApiAccount,
        device: Option<&Device>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        B: Serialize,
        E: fmt::Display + DeserializeOwned,
    {
        match self
            .post_query::<T, B, E>(path, json_body, account, device, None)
            .await
        {
            Ok(response) => Ok(response),
            Err(err) => {
                if let HttpClientError::EndpointFailure { status: _, error } = &err {
                    if jwt_error(&error.to_string()) {
                        tracing::warn!("Encountered possible JWT error: {error}. Retrying query with remote time");
                        if let Ok(Some(jwt)) =
                            self.sync_with_remote_time().await.inspect_err(|err| {
                                tracing::error!(
                                    "Failed to get remote time: {err}. Not retring anymore"
                                )
                            })
                        {
                            // retry with remote vpn api time, and return that only if it succeeds,
                            // otherwise return the initial error
                            let res = self
                                .post_query(path, json_body, account, device, Some(jwt))
                                .await;
                            if res.is_ok() {
                                return res;
                            }
                        }
                    }
                }
                Err(err)
            }
        }
    }

    async fn delete_query<T, E>(
        &self,
        path: PathSegments<'_>,
        account: &VpnApiAccount,
        device: Option<&Device>,
        jwt: Option<VpnApiTime>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        E: fmt::Display + DeserializeOwned,
    {
        let request = self
            .inner
            .create_delete_request(path, NO_PARAMS)
            .bearer_auth(account.jwt(jwt).to_string());

        let request = match device {
            Some(device) => request.header(
                DEVICE_AUTHORIZATION_HEADER,
                format!("Bearer {}", device.jwt(jwt)),
            ),
            None => request,
        };
        let response = request.send().await?;
        nym_http_api_client::parse_response(response, false).await
    }

    async fn delete_authorized<T, E>(
        &self,
        path: PathSegments<'_>,
        account: &VpnApiAccount,
        device: Option<&Device>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        E: fmt::Display + DeserializeOwned,
    {
        match self.delete_query::<T, E>(path, account, device, None).await {
            Ok(response) => Ok(response),
            Err(err) => {
                if let HttpClientError::EndpointFailure { status: _, error } = &err {
                    if jwt_error(&error.to_string()) {
                        tracing::warn!("Encountered possible JWT error: {error}. Retrying query with remote time");
                        if let Ok(Some(jwt)) =
                            self.sync_with_remote_time().await.inspect_err(|err| {
                                tracing::error!(
                                    "Failed to get remote time: {err}. Not retring anymore"
                                )
                            })
                        {
                            // retry with remote vpn api time, and return that only if it succeeds,
                            // otherwise return the initial error
                            let res = self.delete_query(path, account, device, Some(jwt)).await;
                            if res.is_ok() {
                                return res;
                            }
                        }
                    }
                }
                Err(err)
            }
        }
    }

    async fn patch_query<T, B, E>(
        &self,
        path: PathSegments<'_>,
        json_body: &B,
        account: &VpnApiAccount,
        device: Option<&Device>,
        jwt: Option<VpnApiTime>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        B: Serialize,
        E: fmt::Display + DeserializeOwned,
    {
        let request = self
            .inner
            .create_patch_request(path, NO_PARAMS, json_body)
            .bearer_auth(account.jwt(jwt).to_string());

        let request = match device {
            Some(device) => request.header(
                DEVICE_AUTHORIZATION_HEADER,
                format!("Bearer {}", device.jwt(jwt)),
            ),
            None => request,
        };
        let response = request.send().await?;
        nym_http_api_client::parse_response(response, false).await
    }

    async fn patch_authorized<T, B, E>(
        &self,
        path: PathSegments<'_>,
        json_body: &B,
        account: &VpnApiAccount,
        device: Option<&Device>,
    ) -> std::result::Result<T, HttpClientError<E>>
    where
        T: DeserializeOwned,
        B: Serialize,
        E: fmt::Display + DeserializeOwned,
    {
        match self
            .patch_query::<T, B, E>(path, json_body, account, device, None)
            .await
        {
            Ok(response) => Ok(response),
            Err(err) => {
                if let HttpClientError::EndpointFailure { status: _, error } = &err {
                    if jwt_error(&error.to_string()) {
                        tracing::warn!("Encountered possible JWT error: {error}. Retrying query with remote time");
                        if let Ok(Some(jwt)) =
                            self.sync_with_remote_time().await.inspect_err(|err| {
                                tracing::error!(
                                    "Failed to get remote time: {err}. Not retring anymore"
                                )
                            })
                        {
                            // retry with remote vpn api time, and return that only if it succeeds,
                            // otherwise return the initial error
                            let res = self
                                .patch_query(path, json_body, account, device, Some(jwt))
                                .await;
                            if res.is_ok() {
                                return res;
                            }
                        }
                    }
                }
                Err(err)
            }
        }
    }

    // ACCOUNT

    pub async fn get_account(&self, account: &VpnApiAccount) -> Result<NymVpnAccountResponse> {
        self.get_authorized(
            &[routes::PUBLIC, routes::V1, routes::ACCOUNT, &account.id()],
            account,
            None,
        )
        .await
        .map_err(crate::error::VpnApiClientError::FailedToGetAccount)
    }

    pub async fn get_health(&self) -> Result<NymVpnHealthResponse> {
        self.get_json_with_retry(&[routes::PUBLIC, routes::V1, routes::HEALTH], NO_PARAMS)
            .await
            .map_err(crate::error::VpnApiClientError::FailedToGetHealth)
    }

    pub async fn get_account_summary(
        &self,
        account: &VpnApiAccount,
    ) -> Result<NymVpnAccountSummaryResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::SUMMARY,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetAccountSummary)
    }

    // DEVICES

    pub async fn get_devices(&self, account: &VpnApiAccount) -> Result<NymVpnDevicesResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetDevices)
    }

    pub async fn register_device(
        &self,
        account: &VpnApiAccount,
        device: &Device,
    ) -> Result<NymVpnDevice> {
        let body = RegisterDeviceRequestBody {
            device_identity_key: device.identity_key().to_base58_string(),
            signature: device.sign_identity_key().to_base64_string(),
        };

        self.post_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
            ],
            &body,
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToRegisterDevice)
    }

    pub async fn get_active_devices(
        &self,
        account: &VpnApiAccount,
    ) -> Result<NymVpnDevicesResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                routes::ACTIVE,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetActiveDevices)
    }

    pub async fn get_device_by_id(
        &self,
        account: &VpnApiAccount,
        device: &Device,
    ) -> Result<NymVpnDevice> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetDeviceById)
    }

    pub async fn update_device(
        &self,
        account: &VpnApiAccount,
        device: &Device,
        status: DeviceStatus,
    ) -> Result<NymVpnDevice> {
        let body = UpdateDeviceRequestBody {
            status: UpdateDeviceRequestStatus::from(status),
        };

        self.patch_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
            ],
            &body,
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToUpdateDevice)
    }

    // ZK-NYM

    pub async fn get_device_zk_nyms(
        &self,
        account: &VpnApiAccount,
        device: &Device,
    ) -> Result<NymVpnZkNymResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
                routes::ZKNYM,
            ],
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetDeviceZkNyms)
    }

    pub async fn request_zk_nym(
        &self,
        account: &VpnApiAccount,
        device: &Device,
        withdrawal_request: String,
        ecash_pubkey: String,
        expiration_date: String,
        ticketbook_type: String,
    ) -> Result<NymVpnZkNymPost> {
        tracing::debug!("Requesting zk-nym for type: {}", ticketbook_type);
        let body = RequestZkNymRequestBody {
            withdrawal_request,
            ecash_pubkey,
            expiration_date,
            ticketbook_type,
        };
        tracing::debug!("Request body: {:#?}", body);

        self.post_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
                routes::ZKNYM,
            ],
            &body,
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToRequestZkNym)
    }

    pub async fn get_zk_nyms_available_for_download(
        &self,
        account: &VpnApiAccount,
        device: &Device,
    ) -> Result<NymVpnZkNymResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
                routes::ZKNYM,
                routes::AVAILABLE,
            ],
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetDeviceZkNyms)
    }

    pub async fn get_zk_nym_by_id(
        &self,
        account: &VpnApiAccount,
        device: &Device,
        id: &str,
    ) -> Result<NymVpnZkNym> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
                routes::ZKNYM,
                id,
            ],
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetZkNymById)
    }

    pub async fn confirm_zk_nym_download_by_id(
        &self,
        account: &VpnApiAccount,
        device: &Device,
        id: &str,
    ) -> Result<StatusOk> {
        self.delete_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::DEVICE,
                &device.identity_key().to_string(),
                routes::ZKNYM,
                id,
            ],
            account,
            Some(device),
        )
        .await
        .map_err(VpnApiClientError::FailedToConfirmZkNymDownloadById)
    }

    // FREEPASS

    pub async fn get_free_passes(
        &self,
        account: &VpnApiAccount,
    ) -> Result<NymVpnSubscriptionsResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::FREEPASS,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetFreePasses)
    }

    pub async fn apply_freepass(
        &self,
        account: &VpnApiAccount,
        code: String,
    ) -> Result<NymVpnSubscription> {
        let body = ApplyFreepassRequestBody { code };

        self.post_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::FREEPASS,
            ],
            &body,
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToApplyFreepass)
    }

    // SUBSCRIPTIONS

    pub async fn get_subscriptions(
        &self,
        account: &VpnApiAccount,
    ) -> Result<NymVpnSubscriptionsResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::SUBSCRIPTION,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetSubscriptions)
    }

    pub async fn create_subscription(&self, account: &VpnApiAccount) -> Result<NymVpnSubscription> {
        let body = CreateSubscriptionRequestBody {
            valid_from_utc: "todo".to_string(),
            subscription_kind: CreateSubscriptionKind::OneMonth,
        };

        self.post_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::SUBSCRIPTION,
            ],
            &body,
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToCreateSubscription)
    }

    pub async fn get_active_subscriptions(
        &self,
        account: &VpnApiAccount,
    ) -> Result<NymVpnSubscriptionResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::SUBSCRIPTION,
                routes::ACTIVE,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetActiveSubscriptions)
    }

    pub async fn get_usage(&self, account: &VpnApiAccount) -> Result<NymVpnUsagesResponse> {
        self.get_authorized(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::ACCOUNT,
                &account.id(),
                routes::USAGE,
            ],
            account,
            None,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetUsage)
    }

    // GATEWAYS

    pub async fn get_gateways(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewaysResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetGateways)
    }

    pub async fn get_gateways_by_type(
        &self,
        kind: GatewayType,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewaysResponse> {
        match kind {
            GatewayType::MixnetEntry => self.get_entry_gateways(min_performance).await,
            GatewayType::MixnetExit => self.get_exit_gateways(min_performance).await,
            GatewayType::Wg => self.get_vpn_gateways(min_performance).await,
        }
    }

    pub async fn get_gateway_countries_by_type(
        &self,
        kind: GatewayType,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewayCountriesResponse> {
        match kind {
            GatewayType::MixnetEntry => self.get_entry_gateway_countries(min_performance).await,
            GatewayType::MixnetExit => self.get_exit_gateway_countries(min_performance).await,
            GatewayType::Wg => self.get_vpn_gateway_countries(min_performance).await,
        }
    }

    pub async fn get_vpn_gateways(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewaysResponse> {
        let mut params = min_performance.unwrap_or_default().to_param();
        params.push((routes::SHOW_VPN_ONLY.to_string(), "true".to_string()));
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
            ],
            &params,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetVpnGateways)
    }

    pub async fn get_vpn_gateway_countries(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewayCountriesResponse> {
        let mut params = min_performance.unwrap_or_default().to_param();
        params.push((routes::SHOW_VPN_ONLY.to_string(), "true".to_string()));
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::COUNTRIES,
            ],
            &params,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetVpnGatewayCountries)
    }

    pub async fn get_gateway_countries(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewayCountriesResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::COUNTRIES,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetGatewayCountries)
    }

    pub async fn get_entry_gateways(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewaysResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::ENTRY,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetEntryGateways)
    }

    pub async fn get_entry_gateway_countries(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewayCountriesResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::ENTRY,
                routes::COUNTRIES,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetEntryGatewayCountries)
    }

    pub async fn get_exit_gateways(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewaysResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::EXIT,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetExitGateways)
    }

    pub async fn get_exit_gateway_countries(
        &self,
        min_performance: Option<GatewayMinPerformance>,
    ) -> Result<NymDirectoryGatewayCountriesResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::GATEWAYS,
                routes::EXIT,
                routes::COUNTRIES,
            ],
            &min_performance.unwrap_or_default().to_param(),
        )
        .await
        .map_err(VpnApiClientError::FailedToGetExitGatewayCountries)
    }

    // DIRECTORY ZK-NYM

    pub async fn get_directory_zk_nyms_ticketbook_partial_verification_keys(
        &self,
    ) -> Result<PartialVerificationKeysResponse> {
        self.get_json_with_retry(
            &[
                routes::PUBLIC,
                routes::V1,
                routes::DIRECTORY,
                routes::ZK_NYMS,
                routes::TICKETBOOK,
                routes::PARTIAL_VERIFICATION_KEYS,
            ],
            NO_PARAMS,
        )
        .await
        .map_err(VpnApiClientError::FailedToGetDirectoryZkNymsTicketbookPartialVerificationKeys)
    }

    pub async fn get_wellknown_current_env(&self) -> Result<NymWellknownDiscoveryItem> {
        tracing::debug!("Fetching nym vpn network details");
        self.inner
            .get_json(
                &[
                    routes::PUBLIC,
                    routes::V1,
                    routes::WELLKNOWN,
                    routes::CURRENT_ENV,
                ],
                NO_PARAMS,
            )
            .await
            .map_err(VpnApiClientError::FailedToGetVpnNetworkDetails)
    }
}

fn jwt_error(error: &str) -> bool {
    error.to_lowercase().contains("jwt")
}

#[cfg(test)]
mod tests {
    use nym_crypto::asymmetric::ed25519;

    use super::*;

    const BASE_URL: &str = "https://nymvpn.com/api";

    fn user_agent() -> UserAgent {
        UserAgent {
            version: "0.1.0".to_string(),
            application: "nym".to_string(),
            platform: "linux".to_string(),
            git_commit: "123456".to_string(),
        }
    }

    mod account {
        use super::*;

        // Preview deployment example data
        struct PreviewData {
            base_url: &'static str,
            account_mnemonic: &'static str,
            device_private_key_base58: &'static str,
            device_public_key_base58: &'static str,
        }

        fn preview_data() -> PreviewData {
            #[allow(unreachable_code)]
            PreviewData {
                base_url: todo!(),
                account_mnemonic: todo!(),
                device_private_key_base58: todo!(),
                device_public_key_base58: todo!(),
            }
        }

        fn base_url_preview() -> Url {
            preview_data().base_url.parse().unwrap()
        }

        fn get_mnemonic() -> bip39::Mnemonic {
            preview_data().account_mnemonic.parse().unwrap()
        }

        fn get_ed25519_keypair() -> ed25519::KeyPair {
            let private_key_base58 = preview_data().device_private_key_base58;
            let public_key_base58 = preview_data().device_public_key_base58;

            let private_key = bs58::decode(private_key_base58).into_vec().unwrap();
            let public_key = bs58::decode(public_key_base58).into_vec().unwrap();

            ed25519::KeyPair::from_bytes(&private_key, &public_key).unwrap()
        }

        // These tests are all iffy since we are running against a preview deployment, but they are
        // useful to drive implementetion and to check that the API is working as expected.

        #[ignore]
        #[tokio::test]
        async fn get_account() {
            let account = VpnApiAccount::from(get_mnemonic());
            let client = VpnApiClient::new(base_url_preview(), user_agent()).unwrap();
            let response = client.get_account(&account).await.unwrap();
            dbg!(&response);
        }

        #[ignore]
        #[tokio::test]
        async fn get_account_summary() {
            let account = VpnApiAccount::from(get_mnemonic());
            let client = VpnApiClient::new(base_url_preview(), user_agent()).unwrap();
            let response = client.get_account_summary(&account).await.unwrap();
            dbg!(&response);
        }

        #[ignore]
        #[tokio::test]
        async fn get_devices() {
            let account = VpnApiAccount::from(get_mnemonic());
            let client = VpnApiClient::new(base_url_preview(), user_agent()).unwrap();
            let response = client.get_devices(&account).await.unwrap();
            dbg!(&response);
        }

        #[ignore]
        #[tokio::test]
        async fn get_device_zk_nyms() {
            let account = VpnApiAccount::from(get_mnemonic());
            let device = Device::from(get_ed25519_keypair());
            let client = VpnApiClient::new(base_url_preview(), user_agent()).unwrap();
            let response = client.get_device_zk_nyms(&account, &device).await;
            dbg!(&response);
        }
    }

    mod gateway_directory {
        use super::*;

        // These tests are disabled until mainnet is updated

        #[ignore]
        #[tokio::test]
        async fn get_gateways() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client
                .get_gateways(Some(GatewayMinPerformance::default()))
                .await
                .unwrap();
            assert!(!response.into_inner().is_empty());
        }

        #[ignore]
        #[tokio::test]
        async fn get_entry_gateways() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client.get_entry_gateways(None).await.unwrap();
            assert!(!response.into_inner().is_empty());
        }

        #[ignore]
        #[tokio::test]
        async fn get_exit_gateways() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client.get_entry_gateways(None).await.unwrap();
            assert!(!response.into_inner().is_empty());
        }

        #[ignore]
        #[tokio::test]
        async fn get_gateway_countries() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client.get_gateway_countries(None).await.unwrap();
            assert!(!response.into_inner().is_empty());
        }

        #[ignore]
        #[tokio::test]
        async fn get_entry_gateway_countries() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client.get_entry_gateway_countries(None).await.unwrap();
            assert!(!response.into_inner().is_empty());
        }

        #[ignore]
        #[tokio::test]
        async fn get_exit_gateway_countries() {
            let client = VpnApiClient::new(BASE_URL.parse().unwrap(), user_agent()).unwrap();
            let response = client.get_exit_gateway_countries(None).await.unwrap();
            assert!(!response.into_inner().is_empty());
        }
    }
}
