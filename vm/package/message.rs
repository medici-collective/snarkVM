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
use crate::prelude::Authorization;

impl<N: Network> Package<N> {
    /// Executes a program function with the given inputs.
    #[allow(clippy::type_complexity)]
    // todo (ab): will need to change struct here when we get right scheme
    pub fn generate_message<A: crate::circuit::Aleo<Network = N, BaseField = N::Field>, R: Rng + CryptoRng>(
        &self,
        endpoint: String,
        private_key: &PrivateKey<N>,
        function_name: Identifier<N>,
        inputs: &[Value<N>],
        rng: &mut R,
    ) -> Result<Authorization<N>> {
        println!("Inside package");

        let process = self.get_process()?;
        // Retrieve the main program.
        let program = self.program();
        // Retrieve the program ID.
        let program_id = program.id();

        let authorization = process.authorize::<A, R>(private_key, program_id, function_name, inputs.iter(), rng)?;
        // process.execute......

        Ok(authorization)
    }
    // todo: frost_execute impl here
}