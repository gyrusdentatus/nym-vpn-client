// Copyright 2016-2025 Mullvad VPN AB. All Rights Reserved.
// Copyright 2025 Nym Technologies SA <contact@nymtech.net>
// SPDX-License-Identifier: GPL-3.0-only

use std::{io, process::Command};

/// Helper for getting stdout of some command as a String. Ignores the exit code of the command.
pub fn command_stdout_lossy(cmd: &str, args: &[&str]) -> io::Result<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
