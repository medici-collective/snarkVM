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
use snarkvm_console_account::{private_key::*, signature::*, Address};
use snarkvm_console_network::prelude::*;
use snarkvm_console_program::{Value};
use snarkvm::prelude::Testnet3;

use std::convert::TryFrom;

fn main() {
    // todo (ab): add message into fn. params
    let rng = &mut TestRng::default();

    // Generate a random private key.
    let private_key = PrivateKey::<Testnet3>::new(rng).unwrap();

    // Create a value to be signed.
    let value = Value::from_str("{ recipient: aleo1hy0uyudcr24q8nmxr8nlk82penl8jtqyfyuyz6mr5udlt0g3vyfqt9l7ew, amount: 10u128 }").unwrap();

    // Transform the value into a message (a sequence of fields).
    let message = value.to_fields().unwrap();

    // Produce a signature.
    let signature =
        Signature::<Testnet3>::sign(&private_key, &message, rng).unwrap();

    // Verify the signature.
    let address = Address::try_from(&private_key).unwrap();
    assert!(signature.verify(&address, &message));

    // Print the results.
    print!("{signature}");
    print!(" {address}");
    print!(" \"{value}\"")
}