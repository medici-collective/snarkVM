// Copyright (C) 2019-2023 Aleo Systems Inc.
// This file is part of the snarkVM library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;

/// Generates a Message locally
#[derive(Debug, Parser)]
pub struct Message {
    #[clap(default_value = "https://api.explorer.aleo.org/v1", long)]
    endpoint: String,
    /// Toggles offline mode.
    #[clap(long)]
    offline: bool,
}

impl Message {
    /// Compiles an Aleo program function with the specified name.
    #[allow(clippy::format_in_format_args)]
    pub fn parse(self) -> Result<String> {
        // Derive the program directory path.
        let path = std::env::current_dir()?;

        // Load the package.
        let package: Package<CurrentNetwork> = Package::open(&path)?;
        // Load the private key.
        // todo (ab): Looking for private key here, may have to remove...
        Ok(format!("hello world"))
    }
}
