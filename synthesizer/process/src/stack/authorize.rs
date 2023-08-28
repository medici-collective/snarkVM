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

impl<N: Network> Stack<N> {
    /// Authorizes a call to the program function for the given inputs.
    #[inline]
    pub fn authorize<A: circuit::Aleo<Network = N>, R: Rng + CryptoRng>(
        &self,
        private_key: &PrivateKey<N>,
        function_name: impl TryInto<Identifier<N>>,
        inputs: impl ExactSizeIterator<Item = impl TryInto<Value<N>>>,
        rng: &mut R,
    ) -> Result<Authorization<N>> {
        let timer = timer!("Stack::authorize");
        println!("Inside authorizing the call");
        // Prepare the function name.
        println!("Preparing the function name...");
        let function_name = function_name.try_into().map_err(|_| anyhow!("Invalid function name"))?;
        // Retrieve the input types.
        println!("Getting the function inputs....");
        let input_types = self.get_function(&function_name)?.input_types();
        lap!(timer, "Retrieve the input types");

        // Compute the request.
        println!("Compute the request....");
        /*
         * TODO (ab): First pass through just return message from Request::sign then create second path that goes back into sign
         * but takes in message in the function signature. It sounds as if we're going to need 2 different frost_signs?
         * 1. for before we have a message constructed so allow Request::sign to construct the message for us
         * 2. for when we have the message constructed. we will need to pass it back into sign as there is other important stuff in there it seems.
         * Unless we can create an auxiliary function that solely handles message returning? would be a cleaner approach. Then we could really only have
         * 1 frost_authorize function. If we don't have message, create message via auxiliary function then return. If we do then proceed as normal into Request::sign but our
         * Request::frost_sign which takes in message as a param.
         */

        let request = Request::sign(private_key, *self.program.id(), function_name, inputs, &input_types, rng)?;
        lap!(timer, "Compute the request");
        // Initialize the authorization.
        let authorization = Authorization::from(request.clone());

        // This logic is only executed if the program contains external calls.
        if self.get_number_of_calls(&function_name)? > 1 {
            // Construct the call stack.
            println!("Authorize the call stack if program contains external calls..");
            let call_stack = CallStack::Authorize(vec![request], *private_key, authorization.clone());
            // Construct the authorization from the function.
            let _response = self.execute_function::<A>(call_stack)?;
        }
        finish!(timer, "Construct the authorization from the function");

        // Return the authorization.
        Ok(authorization)
    }

pub fn frost_authorize<A: circuit::Aleo<Network = N>, R: Rng + CryptoRng>(
        &self,
        private_key: &PrivateKey<N>,
        function_name: impl TryInto<Identifier<N>>,
        inputs: impl ExactSizeIterator<Item = impl TryInto<Value<N>>>,
        rng: &mut R, // todo (ab): may need to rethink return here
        // initial idea is to perhaps have message here. if we have it continue onwards, if not generate and return it.
    ) -> Result<Vec<Field<N>>> {
        /*
        * create new method return_input_message
        * if we don't have a message, get the message and return
        * if we do have a message, proceed as normal to Request::sign()
        */
        let timer = timer!("Stack::authorize");
        println!("Inside frost, authorizing the call");
        // Prepare the function name.
        let function_name = function_name.try_into().map_err(|_| anyhow!("Invalid function name"))?;
        // Retrieve the input types.
        let input_types = self.get_function(&function_name)?.input_types();
        lap!(timer, "Retrieve the input types");

        // Compute the request.
        let message = Request::frost_sign(private_key, *self.program.id(), function_name, inputs, &input_types, rng)?;
        lap!(timer, "Compute the request");

        Ok(message)

    }
}
