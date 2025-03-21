// Copyright 2025 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use super::{FirewallArguments, FirewallPolicy};

/// Stub error type for Firewall errors on Android.
#[derive(Debug, thiserror::Error)]
#[error("Unknown iOS Firewall error")]
pub struct Error;

/// The Android stub implementation for the firewall.
pub struct Firewall;

impl Firewall {
    pub fn from_args(_args: FirewallArguments) -> Result<Self, Error> {
        Ok(Firewall)
    }

    pub fn new() -> Result<Self, Error> {
        Ok(Firewall)
    }

    pub fn apply_policy(&mut self, _policy: FirewallPolicy) -> Result<(), Error> {
        Ok(())
    }

    pub fn reset_policy(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
