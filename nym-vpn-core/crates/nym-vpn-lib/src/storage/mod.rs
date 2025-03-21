// Copyright 2024 - Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use nym_vpn_store::{
    keys::{
        persistence::{DeviceKeysPaths, OnDiskKeysError},
        DeviceKeys, KeyStore,
    },
    mnemonic::{on_disk::OnDiskMnemonicStorageError, Mnemonic, MnemonicStorage},
};

mod helpers;

const MNEMONIC_FILE_NAME: &str = "mnemonic.json";

pub struct VpnClientOnDiskStorage {
    key_store: nym_vpn_store::keys::persistence::OnDiskKeys,
    mnemonic_storage: nym_vpn_store::mnemonic::on_disk::OnDiskMnemonicStorage,
}

impl VpnClientOnDiskStorage {
    pub fn new<P: AsRef<Path>>(base_data_directory: P) -> Self {
        let device_key_paths = DeviceKeysPaths::new(&base_data_directory);
        let key_store = nym_vpn_store::keys::persistence::OnDiskKeys::new(device_key_paths);

        let mnemonic_storage_path = base_data_directory.as_ref().join(MNEMONIC_FILE_NAME);
        let mnemonic_storage =
            nym_vpn_store::mnemonic::on_disk::OnDiskMnemonicStorage::new(mnemonic_storage_path);

        VpnClientOnDiskStorage {
            key_store,
            mnemonic_storage,
        }
    }
}

impl nym_vpn_store::VpnStorage for VpnClientOnDiskStorage {}

impl KeyStore for VpnClientOnDiskStorage {
    type StorageError = OnDiskKeysError;

    async fn load_keys(&self) -> Result<DeviceKeys, Self::StorageError> {
        self.key_store.load_keys().await
    }

    async fn store_keys(&self, keys: &DeviceKeys) -> Result<(), Self::StorageError> {
        self.key_store.store_keys(keys).await
    }

    async fn init_keys(&self, seed: Option<[u8; 32]>) -> Result<(), Self::StorageError> {
        self.key_store.init_keys(seed).await
    }

    async fn reset_keys(&self, seed: Option<[u8; 32]>) -> Result<(), Self::StorageError> {
        self.key_store.reset_keys(seed).await
    }

    async fn remove_keys(&self) -> Result<(), Self::StorageError> {
        self.key_store.remove_keys().await
    }
}

impl MnemonicStorage for VpnClientOnDiskStorage {
    type StorageError = OnDiskMnemonicStorageError;

    async fn load_mnemonic(&self) -> Result<Mnemonic, Self::StorageError> {
        self.mnemonic_storage.load_mnemonic().await
    }

    async fn store_mnemonic(&self, mnemonic: Mnemonic) -> Result<(), Self::StorageError> {
        self.mnemonic_storage.store_mnemonic(mnemonic).await
    }

    async fn remove_mnemonic(&self) -> Result<(), Self::StorageError> {
        self.mnemonic_storage.remove_mnemonic().await
    }
}
