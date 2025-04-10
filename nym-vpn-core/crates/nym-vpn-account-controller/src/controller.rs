// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{path::PathBuf, sync::Arc, time::Duration};

use nym_http_api_client::UserAgent;
use nym_vpn_api_client::{
    response::{NymVpnAccountResponse, NymVpnDevice, NymVpnUsage},
    types::{DeviceStatus, VpnApiAccount},
};
use nym_vpn_lib_types::{
    AccountCommandError, ForgetAccountError, StoreAccountError, VpnApiErrorResponse,
};
use nym_vpn_network_config::Network;
use nym_vpn_store::{mnemonic::Mnemonic, VpnStorage};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;

use crate::{
    commands::{
        register_device::RegisterDeviceCommandHandler,
        request_zknym::WaitingRequestZkNymCommandHandler,
        sync_account::WaitingSyncAccountCommandHandler,
        sync_device::WaitingSyncDeviceCommandHandler, AccountCommand, AccountCommandResult,
        Command, RunningCommands,
    },
    error::Error,
    shared_state::{MnemonicState, ReadyToRegisterDevice, ReadyToRequestZkNym, SharedAccountState},
    storage::{AccountStorage, VpnCredentialStorage},
    AccountControllerCommander, AvailableTicketbooks,
};

// The interval at which we automatically request zk-nyms
const ZK_NYM_AUTOMATIC_REQUEST_INTERVAL: Duration = Duration::from_secs(60);

// The interval at which we update the account state
const ACCOUNT_UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub struct AccountController<S>
where
    S: VpnStorage,
{
    // The storage used for the account and device keys
    account_storage: AccountStorage<S>,

    // Storage used for credentials.
    credential_storage: Arc<tokio::sync::Mutex<VpnCredentialStorage>>,

    // The data directory where we store the account and device keys.
    data_dir: PathBuf,

    // The API client used to interact with the nym-vpn-api
    vpn_api_client: nym_vpn_api_client::VpnApiClient,

    // The current state of the account
    account_state: SharedAccountState,

    // Receiver channel used to receive commands from the consumer
    command_rx: UnboundedReceiver<AccountCommand>,

    // Sender channel primarily used when the consumer requests a channel to talk to the
    // controller, but also to queue up commands to itself
    command_tx: UnboundedSender<AccountCommand>,

    // List of currently running command tasks and their type
    running_commands: RunningCommands,

    // Command tasks that are currently running
    running_command_tasks: JoinSet<AccountCommandResult>,

    // Account sync command handler state reused between runs
    waiting_sync_account_command_handler: WaitingSyncAccountCommandHandler,

    // Device sync command handler state reused between runs
    waiting_sync_device_command_handler: WaitingSyncDeviceCommandHandler,

    // Zk-nym request command handler state reused between runs
    waiting_request_zknym_command_handler: WaitingRequestZkNymCommandHandler,

    // When credential mode is disabled we don't automatically request zk-nyms. We can still do
    // so manually, but we don't want to do it automatically
    background_zk_nym_refresh: bool,

    // Listen for cancellation signals
    cancel_token: CancellationToken,

    // User agent used by api client.
    user_agent: UserAgent,
}

impl<S> AccountController<S>
where
    S: VpnStorage,
{
    pub async fn new(
        storage: Arc<tokio::sync::Mutex<S>>,
        data_dir: PathBuf,
        user_agent: UserAgent,
        credentials_mode: Option<bool>,
        network_env: Network,
        cancel_token: CancellationToken,
    ) -> Result<Self, Error> {
        let credentials_mode = credentials_mode.unwrap_or_else(|| {
            network_env
                .get_feature_flag_credential_mode()
                .unwrap_or(false)
        });

        tracing::info!("Starting account controller");
        tracing::info!("Account controller: data directory: {:?}", data_dir);
        tracing::info!("Account controller: credential mode: {}", credentials_mode);

        let account_storage = AccountStorage::from(storage);

        // Generate the device keys if we don't already have them
        account_storage.init_keys().await?;

        // Load the account id if we have one stored
        let mnemonic_state = account_storage
            .load_account_id()
            .await
            .map(|id| MnemonicState::Stored { id })
            .unwrap_or(MnemonicState::NotStored);

        let credential_storage = Arc::new(tokio::sync::Mutex::new(
            VpnCredentialStorage::setup_from_path(data_dir.clone()).await?,
        ));

        // Client to query the VPN API
        let vpn_api_client =
            nym_vpn_api_client::VpnApiClient::new(network_env.vpn_api_url(), user_agent.clone())
                .map_err(Error::SetupVpnApiClient)?;

        // We expose the account state as a shared object that can be queried without having to ask
        // the controller
        let account_state = SharedAccountState::new(mnemonic_state);

        // The channels used to communicate with the controller
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();

        let waiting_sync_account_command_handler =
            WaitingSyncAccountCommandHandler::new(account_state.clone(), vpn_api_client.clone());
        let waiting_sync_device_command_handler =
            WaitingSyncDeviceCommandHandler::new(account_state.clone(), vpn_api_client.clone());
        let waiting_request_zknym_command_handler = WaitingRequestZkNymCommandHandler::new(
            credential_storage.clone(),
            account_state.clone(),
            vpn_api_client.clone(),
        );

        Ok(AccountController {
            account_storage,
            credential_storage,
            data_dir,
            vpn_api_client,
            account_state,
            command_rx,
            command_tx,
            running_commands: Default::default(),
            running_command_tasks: JoinSet::new(),
            waiting_sync_account_command_handler,
            waiting_sync_device_command_handler,
            waiting_request_zknym_command_handler,
            background_zk_nym_refresh: credentials_mode,
            cancel_token,
            user_agent,
        })
    }

    pub fn shared_state(&self) -> SharedAccountState {
        self.account_state.clone()
    }

    pub fn commander(&self) -> AccountControllerCommander {
        AccountControllerCommander {
            command_tx: self.command_tx.clone(),
            shared_state: self.account_state.clone(),
        }
    }

    async fn handle_request_zk_nym(&mut self, command: AccountCommand) {
        let account = match self.update_mnemonic_state().await {
            Ok(account) => account,
            Err(err) => {
                command.return_no_account(err);
                return;
            }
        };

        let device = match self.account_storage.load_device_keys().await {
            Ok(device) => device,
            Err(err) => {
                command.return_no_device(err);
                return;
            }
        };

        let command_handler = self
            .waiting_request_zknym_command_handler
            .build(account, device);
        if self.running_commands.add(command).await == Command::IsFirst {
            self.running_command_tasks.spawn(command_handler.run());
        }
    }

    async fn update_mnemonic_state(&self) -> Result<VpnApiAccount, Error> {
        match self.account_storage.load_account().await {
            Ok(account) => {
                tracing::debug!("Our account id: {}", account.id());
                self.account_state
                    .set_mnemonic(MnemonicState::Stored { id: account.id() })
                    .await;
                Ok(account)
            }
            Err(err) => {
                tracing::debug!("No account stored: {err}");
                self.account_state.reset_to(MnemonicState::NotStored).await;
                Err(err)
            }
        }
    }

    async fn register_device_if_ready(&self) {
        match self.shared_state().ready_to_register_device().await {
            ReadyToRegisterDevice::Ready => {
                self.queue_command(AccountCommand::RegisterDevice(None));
            }
            not_ready => {
                tracing::debug!("Not trying to register device: {not_ready}");
            }
        }
    }

    async fn is_background_zk_nym_refresh_active(&self) -> bool {
        self.background_zk_nym_refresh
            && !self
                .waiting_request_zknym_command_handler
                .max_fails_reached()
                .await
    }

    async fn is_all_ticket_types_above_soft_threshold(&self) -> Result<bool, AccountCommandError> {
        self.credential_storage
            .lock()
            .await
            .is_all_ticket_types_above_soft_threshold()
            .await
            .map_err(AccountCommandError::internal)
    }

    async fn request_zk_nym_if_ready(&self) {
        if !self.is_background_zk_nym_refresh_active().await {
            return;
        }
        if self
            .is_all_ticket_types_above_soft_threshold()
            .await
            .unwrap_or(false)
        {
            tracing::debug!("All ticket types are above soft threshold, not requesting zk-nym");
            return;
        }
        match self.shared_state().ready_to_request_zk_nym().await {
            ReadyToRequestZkNym::Ready => {
                self.queue_command(AccountCommand::RequestZkNym(None));
            }
            not_ready => {
                tracing::debug!("Not ready to try to request zk-nym: {not_ready}");
            }
        }
    }

    async fn get_account_by_mnemonic(
        &self,
        mnemonic: Mnemonic,
    ) -> Result<NymVpnAccountResponse, AccountCommandError> {
        let account = VpnApiAccount::from(mnemonic);
        self.vpn_api_client
            .get_account(&account)
            .await
            .map_err(|e| {
                VpnApiErrorResponse::try_from(e)
                    .map(StoreAccountError::GetAccountEndpointFailure)
                    .unwrap_or_else(|e| StoreAccountError::UnexpectedResponse(e.to_string()))
                    .into()
            })
    }

    async fn handle_store_account(&self, mnemonic: Mnemonic) -> Result<(), AccountCommandError> {
        //get account to check that it is a valid account before storing
        self.get_account_by_mnemonic(mnemonic.clone()).await?;
        self.account_storage
            .store_account(mnemonic)
            .await
            .map_err(|err| StoreAccountError::Storage(err.to_string()))?;

        self.update_mnemonic_state()
            .await
            .map_err(AccountCommandError::internal)?;

        // We don't need to wait for the sync to finish, so queue it up and return
        self.queue_command(AccountCommand::SyncAccountState(None));
        self.queue_command(AccountCommand::SyncDeviceState(None));

        Ok(())
    }

    async fn handle_forget_account(&mut self) -> Result<(), AccountCommandError> {
        tracing::info!("REMOVING ACCOUNT AND ALL ASSOCIATED DATA");

        // TODO: here we should put the controller in some sort of idle state, and wait for all
        // currently running operations to finish before proceeding with the reset

        //delete device from nym vpn api
        match self.unregister_device_from_api().await {
            Ok(_) => {
                tracing::info!("Device has been unregistered");
            }
            Err(error) => {
                tracing::error!("Failed to unregister device: {error:?}");
            }
        }

        self.account_storage
            .remove_account()
            .await
            .map_err(|source| {
                tracing::error!("Failed to remove account: {source:?}");
                ForgetAccountError::RemoveAccount(source.to_string())
            })?;

        self.account_storage
            .remove_device_keys()
            .await
            .map_err(|source| {
                tracing::error!("Failed to remove device identity: {source:?}");
                ForgetAccountError::RemoveDeviceKeys(source.to_string())
            })?;

        self.credential_storage
            .lock()
            .await
            .reset()
            .await
            .map_err(|source| {
                tracing::error!("Failed to reset credential storage: {source:?}");
                ForgetAccountError::ResetCredentialStorage(source.to_string())
            })?;

        // Purge all files in the data directory that we are not explicitly deleting through it's
        // owner. Ideally we should strive for this to be removed.
        // If this fails, we still need to continue with the remaining steps
        let remove_files_result = crate::storage_cleanup::remove_files_for_account(&self.data_dir)
            .inspect_err(|err| {
                tracing::error!("Failed to remove files for account: {err:?}");
            });

        // Once we have removed or reset all storage, we need to reset the account state
        self.waiting_request_zknym_command_handler.reset();
        self.account_state.reset().await;

        // And now we are ready to start reconstructing
        let reinit_keys_result = self
            .account_storage
            .init_keys()
            .await
            .inspect_err(|source| {
                tracing::error!("Failed to reinitialize device keys: {source:?}");
            });

        // And conclude by syncing with the remote state
        self.handle_sync_account_state(AccountCommand::SyncAccountState(None))
            .await;

        if let Err(err) = remove_files_result {
            return Err(ForgetAccountError::RemoveAccountFiles(format!(
                "Failed to remove files for account: {err}"
            ))
            .into());
        }

        if let Err(err) = reinit_keys_result {
            return Err(ForgetAccountError::InitDeviceKeys(format!(
                "Failed to reinitialize device keys: {err}"
            ))
            .into());
        }

        Ok(())
    }

    async fn unregister_device_from_api(&self) -> Result<NymVpnDevice, AccountCommandError> {
        tracing::info!("Unregistering device from API");
        if self.shared_state().ready_to_register_device().await == ReadyToRegisterDevice::InProgress
        {
            return Err(ForgetAccountError::RegistrationInProgress.into());
        }

        let device = self
            .account_storage
            .load_device_keys()
            .await
            .map_err(|_err| AccountCommandError::NoDeviceStored)?;

        let account = self
            .account_storage
            .load_account()
            .await
            .map_err(|_err| AccountCommandError::NoAccountStored)?;

        self.vpn_api_client
            .update_device(&account, &device, DeviceStatus::DeleteMe)
            .await
            .map_err(|err| {
                VpnApiErrorResponse::try_from(err)
                    .map(ForgetAccountError::UpdateDeviceErrorResponse)
                    .unwrap_or_else(|err| ForgetAccountError::UnexpectedResponse(err.to_string()))
                    .into()
            })
    }

    async fn handle_sync_account_state(&mut self, command: AccountCommand) {
        let account = match self.update_mnemonic_state().await {
            Ok(account) => account,
            Err(err) => {
                command.return_no_account(err);
                return;
            }
        };

        let command_handler = self.waiting_sync_account_command_handler.build(account);

        if self.running_commands.add(command).await == Command::IsFirst {
            self.running_command_tasks.spawn(command_handler.run());
        }
    }

    async fn handle_sync_device_state(&mut self, command: AccountCommand) {
        let account = match self.update_mnemonic_state().await {
            Ok(account) => account,
            Err(err) => {
                command.return_no_account(err);
                return;
            }
        };

        let device = match self.account_storage.load_device_keys().await {
            Ok(device) => device,
            Err(err) => {
                command.return_no_device(err);
                return;
            }
        };

        let command_handler = self
            .waiting_sync_device_command_handler
            .build(account, device);

        if self.running_commands.add(command).await == Command::IsFirst {
            self.running_command_tasks.spawn(command_handler.run());
        }
    }

    async fn handle_get_usage(&self) -> Result<Vec<NymVpnUsage>, AccountCommandError> {
        let account = self
            .account_storage
            .load_account()
            .await
            .map_err(|err| AccountCommandError::Storage(err.to_string()))?;
        let usage = self
            .vpn_api_client
            .get_usage(&account)
            .await
            .map_err(|err| {
                VpnApiErrorResponse::try_from(err)
                    .map(AccountCommandError::from)
                    .unwrap_or_else(AccountCommandError::internal)
            })?;
        tracing::info!("Usage: {:#?}", usage);
        Ok(usage.items)
    }

    async fn handle_get_device_identity(&self) -> Result<String, AccountCommandError> {
        let device = self
            .account_storage
            .load_device_id()
            .await
            .map_err(|_err| AccountCommandError::NoDeviceStored)?;

        tracing::info!("Device identity: {device:?}");
        Ok(device)
    }

    async fn handle_register_device(&mut self, command: AccountCommand) {
        let account = match self.update_mnemonic_state().await {
            Ok(account) => account,
            Err(err) => {
                command.return_no_account(err);
                return;
            }
        };

        let device = match self.account_storage.load_device_keys().await {
            Ok(device) => device,
            Err(err) => {
                command.return_no_device(err);
                return;
            }
        };

        let command_handler = RegisterDeviceCommandHandler::new(
            account,
            device,
            self.account_state.clone(),
            self.vpn_api_client.clone(),
        );
        if self.running_commands.add(command).await == Command::IsFirst {
            self.running_command_tasks.spawn(command_handler.run());
        }
    }

    async fn handle_get_devices(&mut self) -> Result<Vec<NymVpnDevice>, AccountCommandError> {
        tracing::info!("Getting devices from API");

        let account = self
            .account_storage
            .load_account()
            .await
            .map_err(|err| AccountCommandError::Storage(err.to_string()))?;

        let devices = self
            .vpn_api_client
            .get_devices(&account)
            .await
            .map_err(|err| {
                VpnApiErrorResponse::try_from(err)
                    .map(AccountCommandError::from)
                    .unwrap_or_else(AccountCommandError::internal)
            })?;

        tracing::info!("The account has the following devices associated to it:");
        // TODO: pagination
        for device in &devices.items {
            tracing::info!("{:?}", device);
        }
        Ok(devices.items)
    }

    async fn handle_get_active_devices(
        &mut self,
    ) -> Result<Vec<NymVpnDevice>, AccountCommandError> {
        tracing::info!("Getting active devices from API");

        let account = self
            .account_storage
            .load_account()
            .await
            .map_err(|err| AccountCommandError::Storage(err.to_string()))?;

        let devices = self
            .vpn_api_client
            .get_active_devices(&account)
            .await
            .map_err(|err| {
                VpnApiErrorResponse::try_from(err)
                    .map(AccountCommandError::from)
                    .unwrap_or_else(AccountCommandError::internal)
            })?;

        tracing::info!("The account has the following active devices associated to it:");
        // TODO: pagination
        for device in &devices.items {
            tracing::info!("{:?}", device);
        }
        Ok(devices.items)
    }

    async fn handle_get_device_zk_nym(&mut self) -> Result<(), Error> {
        tracing::info!("Getting device zk-nym from API");

        let account = self.account_storage.load_account().await?;
        let device = self.account_storage.load_device_keys().await?;

        let reported_device_zk_nyms = self
            .vpn_api_client
            .get_device_zk_nyms(&account, &device)
            .await
            .map_err(Error::GetZkNyms)?;

        tracing::info!("The device as the following zk-nyms associated to it on the account:");
        // TODO: pagination
        for zk_nym in &reported_device_zk_nyms.items {
            tracing::info!("{:?}", zk_nym);
        }
        Ok(())
    }

    async fn handle_get_zk_nyms_available_for_download(&self) -> Result<(), Error> {
        tracing::info!("Getting zk-nyms available for download from API");

        let account = self.account_storage.load_account().await?;
        let device = self.account_storage.load_device_keys().await?;

        let reported_device_zk_nyms = self
            .vpn_api_client
            .get_zk_nyms_available_for_download(&account, &device)
            .await
            .map_err(Error::GetZkNyms)?;

        tracing::info!("The device as the following zk-nyms available to download:");
        // TODO: pagination
        for zk_nym in &reported_device_zk_nyms.items {
            tracing::info!("{:?}", zk_nym);
        }

        Ok(())
    }

    async fn handle_get_zk_nym_by_id(&self, id: &str) -> Result<(), Error> {
        tracing::info!("Getting zk-nym by id from API");

        let account = self.account_storage.load_account().await?;
        let device = self.account_storage.load_device_keys().await?;

        let reported_device_zk_nyms = self
            .vpn_api_client
            .get_zk_nym_by_id(&account, &device, id)
            .await
            .map_err(Error::GetZkNyms)?;

        tracing::info!(
            "The device as the following zk-nym available to download: {:#?}",
            reported_device_zk_nyms
        );

        Ok(())
    }

    async fn handle_confirm_zk_nym_downloaded(&self, id: String) -> Result<(), Error> {
        tracing::info!("Confirming zk-nym downloaded: {}", id);

        let account = self.account_storage.load_account().await?;
        let device = self.account_storage.load_device_keys().await?;

        let response = self
            .vpn_api_client
            .confirm_zk_nym_download_by_id(&account, &device, &id)
            .await
            .map_err(Error::ConfirmZkNymDownload)?;

        tracing::info!("Confirmed zk-nym downloaded: {:?}", response);

        Ok(())
    }

    async fn handle_get_available_tickets(
        &self,
    ) -> Result<AvailableTicketbooks, AccountCommandError> {
        tracing::debug!("Getting available tickets from local credential storage");
        let guard = self.credential_storage.lock().await;
        guard
            .print_info()
            .await
            .map_err(|err| AccountCommandError::Storage(err.to_string()))?;
        guard
            .get_available_ticketbooks()
            .await
            .map_err(|err| AccountCommandError::Storage(err.to_string()))
    }

    fn queue_command(&self, command: AccountCommand) {
        if let Err(err) = self.command_tx.send(command) {
            tracing::error!("Failed to queue command: {:#?}", err);
        }
    }

    async fn handle_command(&mut self, command: AccountCommand) {
        tracing::info!("← {}", command);
        match command {
            AccountCommand::StoreAccount(result_tx, mnemonic) => {
                let result = self.handle_store_account(mnemonic).await;
                result_tx.send(result);
            }
            AccountCommand::ForgetAccount(result_tx) => {
                let result = self.handle_forget_account().await;
                result_tx.send(result);
            }
            AccountCommand::SyncAccountState(_) => {
                self.handle_sync_account_state(command).await;
            }
            AccountCommand::SyncDeviceState(_) => {
                self.handle_sync_device_state(command).await;
            }
            AccountCommand::GetUsage(result_tx) => {
                let result = self.handle_get_usage().await;
                result_tx.send(result);
            }
            AccountCommand::GetDeviceIdentity(result_tx) => {
                let result = self.handle_get_device_identity().await;
                result_tx.send(result);
            }
            AccountCommand::RegisterDevice(_) => {
                self.handle_register_device(command).await;
            }
            AccountCommand::GetDevices(result_tx) => {
                let result = self.handle_get_devices().await;
                result_tx.send(result);
            }
            AccountCommand::GetActiveDevices(result_tx) => {
                let result = self.handle_get_active_devices().await;
                result_tx.send(result);
            }
            AccountCommand::RequestZkNym(_) => {
                self.handle_request_zk_nym(command).await;
            }
            AccountCommand::GetDeviceZkNym => {
                self.handle_get_device_zk_nym()
                    .await
                    .inspect_err(|err| {
                        tracing::error!("Failed to get device zk-nym: {:#?}", err);
                    })
                    .ok();
            }
            AccountCommand::GetZkNymsAvailableForDownload => {
                self.handle_get_zk_nyms_available_for_download()
                    .await
                    .inspect_err(|err| {
                        tracing::error!("Failed to get zk-nyms available for download: {:#?}", err);
                    })
                    .ok();
            }
            AccountCommand::GetZkNymById(id) => {
                self.handle_get_zk_nym_by_id(&id)
                    .await
                    .inspect_err(|err| {
                        tracing::error!("Failed to get zk-nym by id: {:#?}", err);
                    })
                    .ok();
            }
            AccountCommand::ConfirmZkNymIdDownloaded(id) => {
                self.handle_confirm_zk_nym_downloaded(id)
                    .await
                    .inspect_err(|err| {
                        tracing::error!("Failed to confirm zk-nym downloaded: {:#?}", err);
                    })
                    .ok();
            }
            AccountCommand::GetAvailableTickets(result_tx) => {
                let result = self.handle_get_available_tickets().await;
                result_tx.send(result);
            }
            AccountCommand::SetStaticApiAddresses(result_tx, static_api_addresses) => {
                result_tx.send(
                    nym_vpn_api_client::VpnApiClient::new_with_resolver_overrides(
                        self.vpn_api_client.current_url().clone(),
                        self.user_agent.clone(),
                        static_api_addresses.as_deref(),
                    )
                    .map(|new_vpn_api_client| {
                        self.vpn_api_client.swap_inner_client(&new_vpn_api_client);
                        self.waiting_sync_account_command_handler
                            .update_vpn_api_client(&new_vpn_api_client);
                        self.waiting_sync_device_command_handler
                            .update_vpn_api_client(&new_vpn_api_client);
                        self.waiting_request_zknym_command_handler
                            .update_vpn_api_client(&new_vpn_api_client);
                    })
                    .map_err(|e| {
                        AccountCommandError::internal(format!(
                            "Failed to set static addresses: {e}",
                        ))
                    }),
                );
            }
        };
    }

    async fn handle_command_result(&self, result: Result<AccountCommandResult, JoinError>) {
        // WIP: this can be a problem. We need to remove the commands from the running_commands for
        // this error case
        let Ok(result) = result else {
            tracing::error!("Joining task failed: {result:?}");
            return;
        };

        match result {
            AccountCommandResult::SyncAccountState(r) => {
                tracing::debug!("Account sync task: {r:?}");
                let commands = self
                    .running_commands
                    .remove(&AccountCommand::SyncAccountState(None))
                    .await;
                for command in commands {
                    if let AccountCommand::SyncAccountState(Some(tx)) = command {
                        tx.send(r.clone());
                    }
                }
                if r.is_ok() {
                    self.register_device_if_ready().await;
                    self.request_zk_nym_if_ready().await;
                }
            }
            AccountCommandResult::SyncDeviceState(r) => {
                tracing::debug!("Device sync task: {r:?}");
                let commands = self
                    .running_commands
                    .remove(&AccountCommand::SyncDeviceState(None))
                    .await;
                for command in commands {
                    if let AccountCommand::SyncDeviceState(Some(tx)) = command {
                        tx.send(r.clone());
                    }
                }
                if r.is_ok() {
                    self.register_device_if_ready().await;
                    self.request_zk_nym_if_ready().await;
                }
            }
            AccountCommandResult::RegisterDevice(r) => {
                tracing::debug!("Device register task: {r:?}");
                let commands = self
                    .running_commands
                    .remove(&AccountCommand::RegisterDevice(None))
                    .await;
                for command in commands {
                    if let AccountCommand::RegisterDevice(Some(tx)) = command {
                        tx.send(r.clone());
                    }
                }
                if r.is_ok() {
                    self.queue_command(AccountCommand::SyncAccountState(None));
                    self.request_zk_nym_if_ready().await;
                }
            }
            AccountCommandResult::RequestZkNym(r) => {
                tracing::debug!("Request zk-nym task: {r:?}");
                let commands = self
                    .running_commands
                    .remove(&AccountCommand::RequestZkNym(None))
                    .await;
                for command in commands {
                    if let AccountCommand::RequestZkNym(Some(tx)) = command {
                        tx.send(r.clone());
                    }
                }
            }
        }
    }

    async fn cleanup(mut self) {
        let timeout = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(timeout);
        while !self.running_command_tasks.is_empty() {
            tokio::select! {
                _ = &mut timeout => {
                    tracing::warn!("Timeout waiting for polling tasks to finish, pending zk-nym's not imported into local credential store!");
                    break;
                },
                Some(result) = self.running_command_tasks.join_next() => {
                    self.handle_command_result(result).await
                },
            }
        }
    }

    async fn print_info(&self) {
        let account_id = self
            .account_storage
            .load_account_id()
            .await
            .ok()
            .unwrap_or_else(|| "(unset)".to_string());
        let device_id = self
            .account_storage
            .load_device_id()
            .await
            .ok()
            .unwrap_or_else(|| "(unset)".to_string());

        tracing::info!("Account id: {}", account_id);
        tracing::info!("Device id: {}", device_id);

        if let Err(err) = self.credential_storage.lock().await.print_info().await {
            tracing::error!("Failed to print credential storage info: {:#?}", err);
        }
    }

    pub async fn run(mut self) {
        tracing::debug!("Account controller initialized successfully");
        self.print_info().await;

        // Timer to check if any command tasks have finished. This just needs to be something small
        // so that we periodically check the results without interfering with other tasks
        let mut command_finish_timer = tokio::time::interval(Duration::from_millis(500));

        // Timer to periodically sync the remote account state
        let mut sync_account_state_timer = tokio::time::interval(ACCOUNT_UPDATE_INTERVAL);

        // Timer to periodically check if we need to request more zk-nyms
        let mut update_zk_nym_timer = tokio::time::interval(ZK_NYM_AUTOMATIC_REQUEST_INTERVAL);

        loop {
            tokio::select! {
                // Handle incoming commands
                Some(command) = self.command_rx.recv() => {
                    self.handle_command(command).await;
                }
                // Check the results of finished tasks
                _ = command_finish_timer.tick() => {
                    while let Some(result) = self.running_command_tasks.try_join_next() {
                        self.handle_command_result(result).await;
                    }
                }
                // On a timer we want to sync the account and device state
                _ = sync_account_state_timer.tick() => {
                    self.queue_command(AccountCommand::SyncAccountState(None));
                    self.queue_command(AccountCommand::SyncDeviceState(None));
                }
                // On a timer to check if we need to request more zk-nyms
                _ = update_zk_nym_timer.tick() => {
                    self.request_zk_nym_if_ready().await;
                }
                _ = self.cancel_token.cancelled() => {
                    tracing::trace!("Received cancellation signal");
                    break;
                }
                else => {
                    tracing::debug!("Account controller channel closed");
                    break;
                }
            }
        }

        self.cleanup().await;
        tracing::debug!("Account controller is exiting");
    }
}
