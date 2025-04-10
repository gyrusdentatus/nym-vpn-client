// Copyright 2023 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

//! The Uniffi generated bindings for the Nym VPN library. The API is designed to be used by
//! frontends to interact with the Nym VPN library. The API is designed to be platform agnostic and
//! should work on any platform that supports the Uniffi FFI bindings.
//!
//! Usage:
//!
//! 1. Initialize the environment: `initEnvironment(..)` or `initFallbackMainnetEnvironment`.
//!
//!    This is required to set the network environment details.
//!
//! 2. Initialise the library: `configureLib(..)`.
//!
//!    This sets up the logger and starts the account controller that runs in the background and
//!    manages the account state.
//!
//! 3. At this point we can interact with the vpn-api and the account controller to do things like:
//!
//!    - Get gateway countries: `getGatewayCountries(..)`.
//!    - Store the account mnemonic: `storeAccountMnemonic(..)`.
//!    - Get the account state: `getAccountState()`.
//!    - Get system messages: `getSystemMessages()`.
//!    - Get account links: `getAccountLinks(..)`.
//!    - ...
//!
//! 3. Start the VPN: `startVPN(..)`.
//!
//!    This will:
//!
//!    1. Check if the account is ready to connect.
//!    2. Request zknym credentials if needed.
//!    3. Start the VPN state machine.
//!
//! 4. Stop the VPN: `stopVPN()`.
//!
//!    This will stop the VPN state machine.
//!
//! 5. Shutdown the library: `shutdown()`.
//!
//!    This will stop the account controller and clean up any resources, including make sure there
//!    are no open DB connections.

#[cfg(target_os = "android")]
pub mod android;
pub(crate) mod error;
pub mod helpers;
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod swift;

mod account;
mod environment;
mod state_machine;
mod uniffi_custom_impls;
mod uniffi_lib_types;

use std::{env, path::PathBuf, sync::Arc};

use account::AccountControllerHandle;
use lazy_static::lazy_static;
use nym_vpn_api_client::types::ScoreThresholds;
use tokio::{runtime::Runtime, sync::Mutex};

use self::error::VpnError;
#[cfg(target_os = "android")]
use crate::tunnel_provider::android::AndroidTunProvider;
#[cfg(target_os = "ios")]
use crate::tunnel_provider::ios::OSTunProvider;
use crate::{
    gateway_directory::GatewayClient, platform::uniffi_custom_impls::NetworkCompatibility,
};
use state_machine::StateMachineHandle;
use uniffi_custom_impls::{
    AccountLinks, AccountStateSummary, EntryPoint, ExitPoint, GatewayInfo, GatewayMinPerformance,
    GatewayType, Location, NetworkEnvironment, SystemMessage, UserAgent,
};
use uniffi_lib_types::TunnelEvent;

lazy_static! {
    static ref RUNTIME: Runtime = Runtime::new().unwrap();
    static ref STATE_MACHINE_HANDLE: Mutex<Option<StateMachineHandle>> = Mutex::new(None);
    static ref ACCOUNT_CONTROLLER_HANDLE: Mutex<Option<AccountControllerHandle>> = Mutex::new(None);
    static ref NETWORK_ENVIRONMENT: Mutex<Option<nym_vpn_network_config::Network>> =
        Mutex::new(None);
}

/// Fetches the network environment details from the network name and initializes the environment,
/// including exporting to the environment
#[allow(non_snake_case)]
#[uniffi::export]
pub fn initEnvironment(network_name: &str) -> Result<(), VpnError> {
    RUNTIME.block_on(environment::init_environment(network_name))
}

/// Async variant of initEnvironment. Fetches the network environment details from the network name
/// and initializes the environment, including exporting to the environment
#[allow(non_snake_case)]
#[uniffi::export]
pub async fn initEnvironmentAsync(network_name: &str) -> Result<(), VpnError> {
    environment::init_environment(network_name).await
}

/// Sets up mainnet defaults without making any network calls. This means no system messages or
/// account links will be available.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn initFallbackMainnetEnvironment() -> Result<(), VpnError> {
    RUNTIME.block_on(environment::init_fallback_mainnet_environment())
}

/// Returns the currently set network environment
#[allow(non_snake_case)]
#[uniffi::export]
pub fn currentEnvironment() -> Result<NetworkEnvironment, VpnError> {
    RUNTIME.block_on(environment::current_environment())
}

/// Setup the library with the given data directory and optionally enable credential mode.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn configureLib(data_dir: String, credential_mode: Option<bool>) -> Result<(), VpnError> {
    RUNTIME.block_on(configure_lib(data_dir, credential_mode))
}

async fn configure_lib(data_dir: String, credential_mode: Option<bool>) -> Result<(), VpnError> {
    let network = environment::current_environment_details().await?;
    account::init_account_controller(PathBuf::from(data_dir), credential_mode, network).await
}

fn init_logger(path: Option<PathBuf>, debug_level: Option<String>) {
    let default_log_level = env::var("RUST_LOG").unwrap_or("info".to_string());
    let log_level = debug_level.unwrap_or(default_log_level);
    tracing::info!("Setting log level: {log_level}, path?: {path:?}");
    #[cfg(target_os = "ios")]
    swift::init_logs(log_level, path);
    #[cfg(target_os = "android")]
    android::init_logs(log_level);
}

/// Additional extra function for when only only want to set the logger without initializing the
/// library. Thus it's only needed when `configureLib` is not used.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn initLogger(path: Option<PathBuf>, debug_level: Option<String>) {
    init_logger(path, debug_level);
}

/// Returns the system messages for the current network environment
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getSystemMessages() -> Result<Vec<SystemMessage>, VpnError> {
    RUNTIME.block_on(environment::get_system_messages())
}

/// Returns the oldest client versions that are compatible with the
/// network environment. (environment must be initialized first)
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getNetworkCompatibilityVersions() -> Result<Option<NetworkCompatibility>, VpnError> {
    RUNTIME.block_on(environment::get_network_compatibility())
}

/// Returns the account links for the current network environment
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getAccountLinks(locale: &str) -> Result<AccountLinks, VpnError> {
    RUNTIME.block_on(environment::get_account_links(locale))
}

/// Returns the account links for the current network environment.
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getAccountLinksRaw(
    account_store_path: &str,
    locale: &str,
) -> Result<AccountLinks, VpnError> {
    RUNTIME.block_on(environment::get_account_links_raw(
        account_store_path,
        locale,
    ))
}

/// Import the account mnemonic
#[allow(non_snake_case)]
#[uniffi::export]
pub fn login(mnemonic: String) -> Result<(), VpnError> {
    RUNTIME.block_on(account::login(&mnemonic))
}

/// Store the account mnemonic
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn loginRaw(mnemonic: String, path: String) -> Result<(), VpnError> {
    RUNTIME.block_on(account::raw::login_raw(&mnemonic, &path))
}

/// Check if the account mnemonic is stored
#[allow(non_snake_case)]
#[uniffi::export]
pub fn isAccountMnemonicStored() -> Result<bool, VpnError> {
    RUNTIME.block_on(account::is_account_mnemonic_stored())
}

/// Check if the account mnemonic is stored
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn isAccountMnemonicStoredRaw(path: String) -> Result<bool, VpnError> {
    RUNTIME.block_on(account::raw::is_account_mnemonic_stored_raw(&path))
}

/// Remove the account mnemonic and all associated keys and files
#[allow(non_snake_case)]
#[uniffi::export]
pub fn forgetAccount() -> Result<(), VpnError> {
    RUNTIME.block_on(account::forget_account())
}

/// Remove the account mnemonic and all associated keys and files.
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn forgetAccountRaw(path: String) -> Result<(), VpnError> {
    RUNTIME.block_on(account::raw::forget_account_raw(&path))
}

/// Get the device identity
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getDeviceIdentity() -> Result<String, VpnError> {
    RUNTIME.block_on(account::get_device_id())
}

/// Get the account identity
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getAccountIdentity() -> Result<String, VpnError> {
    RUNTIME.block_on(get_account_id())
}

/// Get the device identity
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getDeviceIdentityRaw(path: String) -> Result<String, VpnError> {
    RUNTIME.block_on(account::raw::get_device_id_raw(&path))
}

/// Get the account identity
/// This is a version that can be called when the account controller is not running.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getAccountIdentityRaw(path: String) -> Result<String, VpnError> {
    RUNTIME.block_on(account::raw::get_account_id_raw(&path))
}

/// This manually syncs the account state with the server. Normally this is done automatically, but
/// this can be used to manually trigger a sync.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn updateAccountState() -> Result<(), VpnError> {
    RUNTIME.block_on(account::update_account_state())
}

/// Get the account state
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getAccountState() -> Result<AccountStateSummary, VpnError> {
    RUNTIME.block_on(account::get_account_state())
}

/// Get the list of countries that have gateways available of the given type.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getGatewayCountries(
    gw_type: GatewayType,
    user_agent: UserAgent,
    min_gateway_performance: Option<GatewayMinPerformance>,
) -> Result<Vec<Location>, VpnError> {
    RUNTIME.block_on(get_gateway_countries(
        gw_type,
        user_agent,
        min_gateway_performance,
    ))
}

async fn get_account_id() -> Result<String, VpnError> {
    account::get_account_id()
        .await?
        .ok_or(VpnError::NoAccountStored)
}

async fn get_gateway_countries(
    gw_type: GatewayType,
    user_agent: UserAgent,
    min_gateway_performance: Option<GatewayMinPerformance>,
) -> Result<Vec<Location>, VpnError> {
    let network_env = environment::current_environment_details().await?;
    let nyxd_url = network_env.nyxd_url();
    let api_url = network_env.api_url();
    let nym_vpn_api_url = Some(network_env.vpn_api_url());
    let min_gateway_performance = min_gateway_performance.map(|p| p.try_into()).transpose()?;
    let mix_score_thresholds =
        network_env
            .system_configuration
            .as_ref()
            .map(|sc| ScoreThresholds {
                high: sc.mix_thresholds.high,
                medium: sc.mix_thresholds.medium,
                low: sc.mix_thresholds.low,
            });
    let wg_score_thresholds = network_env.system_configuration.map(|sc| ScoreThresholds {
        high: sc.wg_thresholds.high,
        medium: sc.wg_thresholds.medium,
        low: sc.wg_thresholds.low,
    });
    let directory_config = nym_gateway_directory::Config {
        nyxd_url,
        api_url,
        nym_vpn_api_url,
        min_gateway_performance,
        mix_score_thresholds,
        wg_score_thresholds,
    };
    GatewayClient::new(directory_config, user_agent.into())
        .map_err(VpnError::internal)?
        .lookup_countries(gw_type.into())
        .await
        .map(|countries| countries.into_iter().map(Location::from).collect())
        .map_err(|err| VpnError::NetworkConnectionError {
            details: err.to_string(),
        })
}

/// Get the list of gateways available of the given type.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn getGateways(
    gw_type: GatewayType,
    user_agent: UserAgent,
    min_gateway_performance: Option<GatewayMinPerformance>,
) -> Result<Vec<GatewayInfo>, VpnError> {
    RUNTIME.block_on(get_gateways(gw_type, user_agent, min_gateway_performance))
}

async fn get_gateways(
    gw_type: GatewayType,
    user_agent: UserAgent,
    min_gateway_performance: Option<GatewayMinPerformance>,
) -> Result<Vec<GatewayInfo>, VpnError> {
    let network_env = environment::current_environment_details().await?;
    let nyxd_url = network_env.nyxd_url();
    let api_url = network_env.api_url();
    let nym_vpn_api_url = Some(network_env.vpn_api_url());
    let min_gateway_performance = min_gateway_performance.map(|p| p.try_into()).transpose()?;
    let mix_score_thresholds =
        network_env
            .system_configuration
            .as_ref()
            .map(|sc| ScoreThresholds {
                high: sc.mix_thresholds.high,
                medium: sc.mix_thresholds.medium,
                low: sc.mix_thresholds.low,
            });
    let wg_score_thresholds = network_env.system_configuration.map(|sc| ScoreThresholds {
        high: sc.wg_thresholds.high,
        medium: sc.wg_thresholds.medium,
        low: sc.wg_thresholds.low,
    });
    let directory_config = nym_gateway_directory::Config {
        nyxd_url,
        api_url,
        nym_vpn_api_url,
        min_gateway_performance,
        mix_score_thresholds,
        wg_score_thresholds,
    };
    GatewayClient::new(directory_config, user_agent.into())
        .map_err(VpnError::internal)?
        .lookup_gateways(gw_type.into())
        .await
        .map(|gateways| {
            gateways
                .into_inner()
                .into_iter()
                .map(GatewayInfo::from)
                .collect()
        })
        .map_err(|err| VpnError::NetworkConnectionError {
            details: err.to_string(),
        })
}

/// Start the VPN by first establishing that the account is ready to connect, including requesting
/// zknym credentials, and then starting the VPN state machine.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn startVPN(config: VPNConfig) -> Result<(), VpnError> {
    RUNTIME.block_on(start_vpn_inner(config))
}

async fn start_vpn_inner(config: VPNConfig) -> Result<(), VpnError> {
    log_build_info();

    // Get the network environment details. This relies on the network environment being set in
    // advance by calling initEnvironment or initFallbackMainnetEnvironment.
    let network_env = environment::current_environment_details().await?;

    // Enabling credential mode will depend on the network feature flag as well as what is passed
    // in the config.
    let enable_credentials_mode = is_credential_mode_enabled(config.credential_mode).await?;

    let account_controller_tx = account::get_command_sender().await?;

    // Once we have established that the account is ready, we can start the state machine.
    state_machine::init_state_machine(
        config,
        network_env,
        enable_credentials_mode,
        account_controller_tx,
    )
    .await
}

fn log_build_info() {
    let build_info = nym_bin_common::bin_info_local_vergen!();
    tracing::info!(
        "{} {} ({})",
        build_info.binary_name,
        build_info.build_version,
        build_info.commit_sha
    );
}

async fn is_credential_mode_enabled(credential_mode: Option<bool>) -> Result<bool, VpnError> {
    match credential_mode {
        Some(enable_credentials_mode) => Ok(enable_credentials_mode),
        None => environment::get_feature_flag_credential_mode().await,
    }
}

/// Stop the VPN by stopping the VPN state machine.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn stopVPN() -> Result<(), VpnError> {
    RUNTIME.block_on(stop_vpn_inner())
}

async fn stop_vpn_inner() -> Result<(), VpnError> {
    let mut guard = STATE_MACHINE_HANDLE.lock().await;

    match guard.take() {
        Some(state_machine_handle) => {
            // TODO: add timeout
            state_machine_handle.shutdown_and_wait().await;
            Ok(())
        }
        None => Err(VpnError::InvalidStateError {
            details: "State machine is not running.".to_owned(),
        }),
    }
}

/// Shutdown the library by stopping the account controller and cleaning up any resources.
#[allow(non_snake_case)]
#[uniffi::export]
pub fn shutdown() -> Result<(), VpnError> {
    RUNTIME.block_on(account::stop_account_controller())
}

#[derive(uniffi::Record)]
pub struct VPNConfig {
    pub entry_gateway: EntryPoint,
    pub exit_router: ExitPoint,
    pub enable_two_hop: bool,
    #[cfg(target_os = "android")]
    pub tun_provider: Arc<dyn AndroidTunProvider>,
    #[cfg(target_os = "ios")]
    pub tun_provider: Arc<dyn OSTunProvider>,
    pub config_path: Option<PathBuf>,
    pub credential_data_path: Option<PathBuf>,
    pub tun_status_listener: Option<Arc<dyn TunnelStatusListener>>,
    pub credential_mode: Option<bool>,
    pub statistics_recipient: Option<String>,
    pub user_agent: UserAgent,
}

#[uniffi::export(with_foreign)]
pub trait TunnelStatusListener: Send + Sync {
    fn on_event(&self, event: TunnelEvent);
}
