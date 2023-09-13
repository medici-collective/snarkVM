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

use std::collections::HashMap;
use super::*;

impl<N: Network> Request<N> {
    // TODO @matt -- try implementing this on sign first to get logic right -- then once figure it out with sign -- go back and just add a whole new function
    // Search TODOs to find what's left

    /// Returns the request for a given private key, program ID, function name, inputs, input types, and RNG, where:
    ///     challenge := HashToScalar(r * G, pk_sig, pr_sig, caller, \[tvk, tcm, function ID, input IDs\])
    ///     response := r - challenge * sk_sig
    pub fn sign<R: Rng + CryptoRng>(
        private_key: &PrivateKey<N>,
        program_id: ProgramID<N>,
        function_name: Identifier<N>,
        inputs: impl ExactSizeIterator<Item = impl TryInto<Value<N>>>,
        input_types: &[ValueType<N>],
        rng: &mut R,
    ) -> Result<Self> {
        // Ensure the number of inputs matches the number of input types.
        if input_types.len() != inputs.len() {
            bail!(
                "Function '{}' in the program '{}' expects {} inputs, but {} were provided.",
                function_name,
                program_id,
                input_types.len(),
                inputs.len()
            )
        }

        println!("Inside Request::sign...");

        // Retrieve `sk_sig`.
        // TODO @matt -- swap with seed phrase for a view key
        let sk_sig = private_key.sk_sig();

        // Derive the compute key.
        let compute_key = ComputeKey::try_from(private_key)?;

        // TODO @matt -- test derive a compute key from a seed phrase for view key
        // let compute_key_two = ComputeKey::try_from();

        // Retrieve `pk_sig`.
        let pk_sig = compute_key.pk_sig();
        // Retrieve `pr_sig`.
        let pr_sig = compute_key.pr_sig();

        // Derive the view key.
        let view_key = ViewKey::try_from((private_key, &compute_key))?;
        // Derive `sk_tag` from the graph key.
        let sk_tag = GraphKey::try_from(view_key)?.sk_tag();

        // Sample a random nonce.
        let nonce = Field::<N>::rand(rng);
        // Compute a `r` as `HashToScalar(sk_sig || nonce)`. Note: This is the transition secret key `tsk`.
        // TODO @matt -- use seed phrase from MS view key
        let r = N::hash_to_scalar_psd4(&[N::serial_number_domain(), sk_sig.to_field()?, nonce])?;
        // Compute `g_r` as `r * G`. Note: This is the transition public key `tpk`.
        let g_r = N::g_scalar_multiply(&r);

        // Derive the caller from the compute key.
        // TODO -- make sure  Address here is computed from view key instead of compute key but it also needs to match owner of record
        let caller = Address::try_from(compute_key)?;
        // Compute the transition view key `tvk` as `r * caller`.
        let tvk = (*caller * r).to_x_coordinate();
        // Compute the transition commitment `tcm` as `Hash(tvk)`.
        let tcm = N::hash_psd2(&[tvk])?;

        // Compute the function ID as `Hash(network_id, program_id, function_name)`.
        let function_id = N::hash_bhp1024(
            &(U16::<N>::new(N::ID), program_id.name(), program_id.network(), function_name).to_bits_le(),
        )?;

        // Construct the hash input as `(r * G, pk_sig, pr_sig, caller, [tvk, tcm, function ID, input IDs])`.
        let mut message = Vec::with_capacity(5 + 2 * inputs.len());
        message.extend([g_r, pk_sig, pr_sig, *caller].map(|point| point.to_x_coordinate()));
        message.extend([tvk, tcm, function_id]);

        // todo (ab): can pull out the message here... but then will need to create a separate new path that takes in message...
        println!("Message: {:?}", message);

        // Initialize a vector to store the prepared inputs.
        let mut prepared_inputs = Vec::with_capacity(inputs.len());
        // Initialize a vector to store the input IDs.
        let mut input_ids = Vec::with_capacity(inputs.len());

        // Prepare the inputs.
        for (index, (input, input_type)) in inputs.zip_eq(input_types).enumerate() {
            // Prepare the input.
            let input = input.try_into().map_err(|_| {
                anyhow!("Failed to parse input #{index} ('{input_type}') for '{program_id}/{function_name}'")
            })?;
            // Store the prepared input.
            prepared_inputs.push(input.clone());

            match input_type {
                // A constant input is hashed (using `tcm`) to a field element.
                ValueType::Constant(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tcm || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tcm);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input ID to the inputs.
                    input_ids.push(InputID::Constant(input_hash));
                }
                // A public input is hashed (using `tcm`) to a field element.
                ValueType::Public(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tcm || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tcm);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input ID to the inputs.
                    input_ids.push(InputID::Public(input_hash));
                }
                // A private input is encrypted (using `tvk`) and hashed to a field element.
                ValueType::Private(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Compute the input view key as `Hash(function ID || tvk || index)`. --
                    // TODO @matt -- make sure this works with new tvk
                    let input_view_key = N::hash_psd4(&[function_id, tvk, index])?;
                    // Compute the ciphertext.
                    let ciphertext = match &input {
                        Value::Plaintext(plaintext) => plaintext.encrypt_symmetric(input_view_key)?,
                        // Ensure the input is a plaintext.
                        Value::Record(..) => bail!("Expected a plaintext input, found a record input"),
                    };
                    // Hash the ciphertext to a field element.
                    let input_hash = N::hash_psd8(&ciphertext.to_fields()?)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input hash to the inputs.
                    input_ids.push(InputID::Private(input_hash));
                }
                // A record input is computed to its serial number.
                ValueType::Record(record_name) => {
                    // Retrieve the record.
                    let record = match &input {
                        Value::Record(record) => record,
                        // Ensure the input is a record.
                        Value::Plaintext(..) => bail!("Expected a record input, found a plaintext input"),
                    };
                    // Ensure the record belongs to the caller.
                    ensure!(**record.owner() == caller, "Input record for '{program_id}' must belong to the signer");

                    // Compute the record commitment. --
                    // TODO @matt -- dig through the to_commitment function and see what needs to change
                    let commitment = record.to_commitment(&program_id, record_name)?;

                    // Compute the generator `H` as `HashToGroup(commitment)`.
                    let h = N::hash_to_group_psd2(&[N::serial_number_domain(), commitment])?;
                    // Compute `h_r` as `r * H`.
                    let h_r = h * r;
                    // Compute `gamma` as `sk_sig * H`. --
                    // TODO @matt -- replace sk_sig with seed phrase for view key
                    let gamma = h * sk_sig;

                    // Compute the `serial_number` from `gamma`.
                    let serial_number = Record::<N, Plaintext<N>>::serial_number_from_gamma(&gamma, commitment)?;
                    // Compute the tag.
                    // TODO @matt -- this sk_tag also needs to be replaces smh
                    let tag = Record::<N, Plaintext<N>>::tag(sk_tag, commitment)?;

                    // Add (`H`, `r * H`, `gamma`, `tag`) to the preimage.
                    message.extend([h, h_r, gamma].iter().map(|point| point.to_x_coordinate()));
                    message.push(tag);

                    // Add the input ID.
                    input_ids.push(InputID::Record(commitment, gamma, serial_number, tag));
                }
                // An external record input is hashed (using `tvk`) to a field element.
                ValueType::ExternalRecord(..) => {
                    // Ensure the input is a record.
                    ensure!(matches!(input, Value::Record(..)), "Expected a record input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tvk || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tvk);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input hash to the inputs.
                    input_ids.push(InputID::ExternalRecord(input_hash));
                }
            }
        }
        println!("{:?} before hash to scalar.....", message);

        // Compute `challenge` as `HashToScalar(r * G, pk_sig, pr_sig, caller, [tvk, tcm, function ID, input IDs])`.
        // TODO -- @matt -- make sure this maps correctly
        let challenge = N::hash_to_scalar_psd8(&message)?;

        println!("{:?} after hash to scalar", challenge);
        // Compute `response` as `r - challenge * sk_sig`.
        // TODO -- @matt -- need to replace sk_sig here as well
        let response = r - challenge * sk_sig;

        Ok(Self {
            caller,
            network_id: U16::new(N::ID),
            program_id,
            function_name,
            input_ids,
            inputs: prepared_inputs,
            signature: Signature::from((challenge, response, compute_key)),
            sk_tag,
            tvk,
            tsk: r,
            tcm,
        })
    }
    pub fn frost_sign<R: Rng + CryptoRng>(
        private_key: &PrivateKey<N>,
        program_id: ProgramID<N>,
        function_name: Identifier<N>,
        inputs: impl ExactSizeIterator<Item = impl TryInto<Value<N>>>,
        input_types: &[ValueType<N>],
        rng: &mut R,
    ) -> Result<Vec<u8>> {
        if input_types.len() != inputs.len() {
            bail!(
                "Function '{}' in the program '{}' expects {} inputs, but {} were provided.",
                function_name,
                program_id,
                input_types.len(),
                inputs.len()
            )
        }

        println!("We are frostinng.");

        // Todo: Add if conditional, if we have message, then ...
        // else: return message

        // Retrieve `sk_sig`
        let sk_sig = private_key.sk_sig();

        //Derive the compute key.
        let compute_key = ComputeKey::try_from(private_key)?;

        // Retrieve the `pk_sig`
        let pk_sig = compute_key.pk_sig();

        // Retrieve the `pr_sig`
        let pr_sig = compute_key.pr_sig();

        // Derive the view key
        let view_key = ViewKey::try_from((private_key, &compute_key))?;

        // Derive the `sk_tag` from the graph key.
        let sk_tag = GraphKey::try_from(view_key)?.sk_tag();

        // Sample a random nonce
        let nonce = Field::<N>::rand(rng);

        // Compute a `r` as `HashToScalar(sk_sig || nonce)`. Note: this is the transition secret key `tsk`.
        let r = N::hash_to_scalar_psd4(&[N::serial_number_domain(), sk_sig.to_field()?, nonce])?;

        // Compute `g_r` as `r * G`. Note: this is the transition public key `tpk`.
        let g_r = N::g_scalar_multiply(&r);

        println!("G_R INSIDE REQUEST::SIGN : {:?}", g_r);
        println!("------------------------");

        println!("PK SIG INSIDE Request::Sign: {:?}", pk_sig);
        println!("__________________________________");

        println!("PR SIG INSIDE Request::Sign: {:?}", pr_sig);
        println!("__________________________________");

        println!("__________________________________");

        println!("COMPUTE KEY INSIDE Request::Sign: {:?}", compute_key);
        println!("__________________________________");




        // Derive the caller from the compute key
        let caller = Address::try_from(compute_key)?;

        println!("Address INSIDE Request::Sign: {:?}", caller);
        println!("__________________________________");

        // Compute the transition view key `tvk` as `r * caller`.
        let tvk = (*caller * r).to_x_coordinate();

        println!("TVK: {:?}", tvk);
        println!("__________________________________");

        // Compute the transition commitment `tcm` as `Hash(tvk)`.
        let tcm = N::hash_psd2(&[tvk])?;

        println!("TCM: {:?}", tcm);
        println!("__________________________________");

        // Compute the function ID as `Hash(network_id, program_id, function_name)`.
        let function_id = N::hash_bhp1024(
            &(U16::<N>::new(N::ID), program_id.name(), program_id.network(), function_name).to_bits_le(),
        )?;


        println!("FUNCTION ID: {:?}", function_id);
        println!("__________________________________");

        // Construct the hash input as `(r * G, pk_sig, pr_sig, caller, [tvk, tcm, function ID, input IDs])`
        let mut message = Vec::with_capacity(5 + 2 * inputs.len());
        println!("MESSAGE AFTER VEC INITIALIZATION INSIDE REQUEST::SIGN {:?}", message);
        println!("__________________________________");
        message.extend([g_r, pk_sig, pr_sig, *caller].map(|point|point.to_x_coordinate()));

        println!("MESSAGE AFTER EXTEND INSIDE REQUEST::SIGN {:?}", message);
        println!("__________________________________");

        let mut my_dict: HashMap<String, Value<N>> = HashMap::new();

        for (index, field) in message.clone().into_iter().enumerate() {
            let lit = Literal::Field(field);
            let val = Value::from(&lit); // assuming the conversion takes a reference
            let key = format!("field_{}", index + 1);  // generate key in the format "field_i"
            my_dict.insert(key, val);
        }


        let string_representation: String = my_dict.iter()
        .map(|(k, v)| (k, k.trim_start_matches("field_").parse::<usize>().unwrap_or(0), v)) // extract numeric part
        .sorted_by(|(_, a_num, _), (_, b_num, _)| a_num.cmp(b_num)) // sort by the numeric part
        .map(|(key, _, value)| format!("  {}: {:?}", key, value)) // Use Debug trait for formatting
        .collect::<Vec<String>>()
        .join(",\n");

        // println!("{:?}", string_representation);

        // println!("__________________________________________");


        let result = format!("{{\n{}\n}}", string_representation);
        println!("RESULT: {}", result);

        // string to Result<Value<N>>;
        // let lit_two = Literal::String(&result);
        let val_of_dict: Result<Value<N>> = Value::try_from(&result);

        println!("__________________________________________");

        println!("VAL OF DICT: {:?}", val_of_dict);
        println!("__________________________________________");
        let val_unwrapped = val_of_dict.unwrap();
        println!("Val unwrapped: {:?}", val_unwrapped);

        let mut res = val_unwrapped.to_fields()?;
        println!("RES BABY: {:?}", res);
        println!("__________________________________________");

        let first_four_message = message[0..4].to_vec();
        println!("FIRST_FOUR MES, {:?}", first_four_message);
        println!("__________________________________________");
        &res.splice(0..0, first_four_message);

        message.extend([tvk, tcm, function_id]);
        println!("__________________________________________");
        println!("MESSAGE AFTER TVK, TCM EXTENSION {:?}", message);
        println!("__________________________________________");

        // todo: yeet out tvk, tcm, function_id

        // Initialize a vector to store the prepared inputs.
        let mut prepared_inputs = Vec::with_capacity(inputs.len());
        // Initialize a vector to store the input IDs.
        let mut input_ids = Vec::with_capacity(inputs.len());

        // Prepare the inputs.
        for (index, (input, input_type)) in inputs.zip_eq(input_types).enumerate() {
            // Prepare the input.
            let input = input.try_into().map_err(|_| {
                anyhow!("Failed to parse input #{index} ('{input_type}') for '{program_id}/{function_name}'")
            })?;
            // Store the prepared input.
            prepared_inputs.push(input.clone());

            match input_type {
                // A constant input is hashed (using `tcm`) to a field element.
                ValueType::Constant(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tcm || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tcm);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input ID to the inputs.
                    input_ids.push(InputID::Constant(input_hash));
                }
                // A public input is hashed (using `tcm`) to a field element.
                ValueType::Public(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tcm || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tcm);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input ID to the inputs.
                    input_ids.push(InputID::Public(input_hash));
                }
                // A private input is encrypted (using `tvk`) and hashed to a field element.
                ValueType::Private(..) => {
                    // Ensure the input is a plaintext.
                    ensure!(matches!(input, Value::Plaintext(..)), "Expected a plaintext input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Compute the input view key as `Hash(function ID || tvk || index)`. --
                    // TODO @matt -- make sure this works with new tvk
                    let input_view_key = N::hash_psd4(&[function_id, tvk, index])?;
                    // Compute the ciphertext.
                    let ciphertext = match &input {
                        Value::Plaintext(plaintext) => plaintext.encrypt_symmetric(input_view_key)?,
                        // Ensure the input is a plaintext.
                        Value::Record(..) => bail!("Expected a plaintext input, found a record input"),
                    };
                    // Hash the ciphertext to a field element.
                    let input_hash = N::hash_psd8(&ciphertext.to_fields()?)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // println!("__________________________________________");
                    // println!("PREIMAGE INSIDE FOR LOOP: {:?}", message);
                    // println!("__________________________________________");

                    let scaled = N::hash_to_scalar_psd8(&message);
                    // println!("SCALARED {:?}", scaled);
                    // Add the input hash to the inputs.
                    input_ids.push(InputID::Private(input_hash));
                }
                // A record input is computed to its serial number.
                ValueType::Record(record_name) => {
                    // Retrieve the record.
                    let record = match &input {
                        Value::Record(record) => record,
                        // Ensure the input is a record.
                        Value::Plaintext(..) => bail!("Expected a record input, found a plaintext input"),
                    };
                    // Ensure the record belongs to the caller.
                    ensure!(**record.owner() == caller, "Input record for '{program_id}' must belong to the signer");

                    // Compute the record commitment. --
                    // TODO @matt -- dig through the to_commitment function and see what needs to change
                    let commitment = record.to_commitment(&program_id, record_name)?;

                    // Compute the generator `H` as `HashToGroup(commitment)`.
                    let h = N::hash_to_group_psd2(&[N::serial_number_domain(), commitment])?;
                    // Compute `h_r` as `r * H`.
                    let h_r = h * r;
                    // Compute `gamma` as `sk_sig * H`. --
                    let gamma = h * sk_sig;

                    // Compute the `serial_number` from `gamma`.
                    let serial_number = Record::<N, Plaintext<N>>::serial_number_from_gamma(&gamma, commitment)?;
                    // Compute the tag.
                    let tag = Record::<N, Plaintext<N>>::tag(sk_tag, commitment)?;

                    // Add (`H`, `r * H`, `gamma`, `tag`) to the preimage.
                    message.extend([h, h_r, gamma].iter().map(|point| point.to_x_coordinate()));
                    message.push(tag);

                    // Add the input ID.
                    input_ids.push(InputID::Record(commitment, gamma, serial_number, tag));
                }
                // An external record input is hashed (using `tvk`) to a field element.
                ValueType::ExternalRecord(..) => {
                    // Ensure the input is a record.
                    ensure!(matches!(input, Value::Record(..)), "Expected a record input");

                    // Construct the (console) input index as a field element.
                    let index = Field::from_u16(u16::try_from(index).or_halt_with::<N>("Input index exceeds u16"));
                    // Construct the preimage as `(function ID || input || tvk || index)`.
                    let mut preimage = vec![function_id];
                    preimage.extend(input.to_fields()?);
                    preimage.push(tvk);
                    preimage.push(index);
                    // Hash the input to a field element.
                    let input_hash = N::hash_psd8(&preimage)?;

                    // Add the input hash to the preimage.
                    message.push(input_hash);
                    // Add the input hash to the inputs.
                    input_ids.push(InputID::ExternalRecord(input_hash));
                }
            }
        }

        println!("RES AFTER PREPEND {:?}", res);
        println!("__________________________________________");


        println!("__________________________________________");
        // todo: call hash_to_scalar on above res
        let challenge = N::hash_to_scalar_psd8(&res)?;

        println!("CHALLENGE {:?} after hash to scalar", challenge);
        println!("__________________________________________");

        // Compute `response` as `r - challenge * sk_sig`.
        // TODO -- @matt -- need to replace sk_sig here as well
        let response = r - challenge * sk_sig;

        println!("------------------------");

        println!("RESPONSE: {:?}", response);

        let sig: snarkvm_console_account::Signature<N> = Signature::from((challenge, response, compute_key));

        println!("------------------------");

        println!("SIGNATURE: {:?}", sig);

        // Serialize the message into bytes
        let mut message_bytes = Vec::new();

        for field in message {
            match field.to_bytes_le() {
                Ok(bytes) => message_bytes.extend_from_slice(&bytes),
                Err(e) => {
                }
            }
        }

        println!("__________________________________");
        println!("Ladies and Gentleman, we got the message in bytes? : {:?}", message_bytes);
        println!("------------------------");

        Ok(message_bytes)

    }
}
