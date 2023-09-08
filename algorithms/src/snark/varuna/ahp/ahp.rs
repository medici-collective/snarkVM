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

use crate::{
    fft::{
        domain::{FFTPrecomputation, IFFTPrecomputation},
        EvaluationDomain,
    },
    polycommit::sonic_pc::{LCTerm, LabeledPolynomial, LinearCombination},
    r1cs::SynthesisError,
    snark::varuna::{
        ahp::{verifier, AHPError, CircuitId, CircuitInfo},
        prover,
        SNARKMode,
    },
};
use snarkvm_fields::{Field, PrimeField};

use core::{borrow::Borrow, marker::PhantomData};
use itertools::Itertools;
use std::collections::BTreeMap;

/// The algebraic holographic proof defined in [CHMMVW19](https://eprint.iacr.org/2019/1047).
/// Currently, this AHP only supports inputs of size one
/// less than a power of 2 (i.e., of the form 2^n - 1).
pub struct AHPForR1CS<F: Field, MM: SNARKMode> {
    field: PhantomData<F>,
    mode: PhantomData<MM>,
}

#[derive(Debug, Clone, Copy)]
struct VerifierChallenges<F: Field> {
    alpha: F,
    beta: F,
    gamma: F,
}

pub(crate) fn witness_label(circuit_id: CircuitId, poly: &str, i: usize) -> String {
    format!("circuit_{circuit_id}_{poly}_{i:0>8}")
}

pub(crate) struct NonZeroDomains<F: PrimeField> {
    pub(crate) max_non_zero_domain: Option<EvaluationDomain<F>>,
    pub(crate) domain_a: EvaluationDomain<F>,
    pub(crate) domain_b: EvaluationDomain<F>,
    pub(crate) domain_c: EvaluationDomain<F>,
}

impl<F: PrimeField, MM: SNARKMode> AHPForR1CS<F, MM> {
    /// The linear combinations that are statically known to evaluate to zero.
    /// These correspond to the virtual commitments as noted in the Aleo varuna protocol docs
    pub const LC_WITH_ZERO_EVAL: [&'static str; 3] = ["matrix_sumcheck", "lineval_sumcheck", "rowcheck_zerocheck"];

    pub fn zk_bound() -> Option<usize> {
        MM::ZK.then_some(1)
    }

    /// Check that the (formatted) public input is of the form 2^n for some integer n.
    pub fn num_formatted_public_inputs_is_admissible(num_inputs: usize) -> Result<(), AHPError> {
        match num_inputs.count_ones() == 1 {
            true => Ok(()),
            false => Err(AHPError::InvalidPublicInputLength),
        }
    }

    /// Check that the (formatted) public input is of the form 2^n for some integer n.
    pub fn formatted_public_input_is_admissible(input: &[F]) -> Result<(), AHPError> {
        Self::num_formatted_public_inputs_is_admissible(input.len())
    }

    /// The maximum degree of polynomials produced by the indexer and prover
    /// of this protocol.
    /// The number of the variables must include the "one" variable. That is, it
    /// must be with respect to the number of formatted public inputs.
    pub fn max_degree(num_constraints: usize, num_variables: usize, num_non_zero: usize) -> Result<usize, AHPError> {
        let zk_bound = Self::zk_bound().unwrap_or(0);
        let constraint_domain_size =
            EvaluationDomain::<F>::compute_size_of_domain(num_constraints).ok_or(AHPError::PolynomialDegreeTooLarge)?;
        let variable_domain_size =
            EvaluationDomain::<F>::compute_size_of_domain(num_variables).ok_or(AHPError::PolynomialDegreeTooLarge)?;
        let non_zero_domain_size =
            EvaluationDomain::<F>::compute_size_of_domain(num_non_zero).ok_or(AHPError::PolynomialDegreeTooLarge)?;

        // these should correspond with the bounds set in the <round>.rs files
        Ok(*[
            2 * constraint_domain_size + 2 * zk_bound - 2,
            2 * variable_domain_size + 2 * zk_bound - 2,
            if MM::ZK { variable_domain_size + 3 } else { 0 }, // mask_poly
            variable_domain_size,
            constraint_domain_size,
            non_zero_domain_size - 1, // non-zero polynomials
        ]
        .iter()
        .max()
        .unwrap())
    }

    /// Get all the strict degree bounds enforced in the AHP.
    pub fn get_degree_bounds(info: &CircuitInfo) -> [usize; 4] {
        let num_variables = info.num_variables;
        let num_non_zero_a = info.num_non_zero_a;
        let num_non_zero_b = info.num_non_zero_b;
        let num_non_zero_c = info.num_non_zero_c;
        [
            EvaluationDomain::<F>::compute_size_of_domain(num_variables).unwrap() - 2,
            EvaluationDomain::<F>::compute_size_of_domain(num_non_zero_a).unwrap() - 2,
            EvaluationDomain::<F>::compute_size_of_domain(num_non_zero_b).unwrap() - 2,
            EvaluationDomain::<F>::compute_size_of_domain(num_non_zero_c).unwrap() - 2,
        ]
    }

    pub(crate) fn cmp_non_zero_domains(
        info: &CircuitInfo,
        max_candidate: Option<EvaluationDomain<F>>,
    ) -> Result<NonZeroDomains<F>, SynthesisError> {
        let domain_a = EvaluationDomain::new(info.num_non_zero_a).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
        let domain_b = EvaluationDomain::new(info.num_non_zero_b).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
        let domain_c = EvaluationDomain::new(info.num_non_zero_c).ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
        let new_candidate = [domain_a, domain_b, domain_c].into_iter().max_by_key(|d| d.size()).unwrap();
        let mut max_non_zero_domain = Some(new_candidate);
        if max_candidate.is_some() && max_candidate.unwrap().size() > new_candidate.size() {
            max_non_zero_domain = max_candidate;
        }
        Ok(NonZeroDomains { max_non_zero_domain, domain_a, domain_b, domain_c })
    }

    pub fn fft_precomputation(
        constraint_domain_size: usize,
        variable_domain_size: usize,
        non_zero_a_domain_size: usize,
        non_zero_b_domain_size: usize,
        non_zero_c_domain_size: usize,
    ) -> Option<(FFTPrecomputation<F>, IFFTPrecomputation<F>)> {
        let largest_domain_size = [
            2 * constraint_domain_size,
            2 * variable_domain_size,
            2 * non_zero_a_domain_size,
            2 * non_zero_b_domain_size,
            2 * non_zero_c_domain_size,
        ]
        .into_iter()
        .max()?;
        let largest_mul_domain = EvaluationDomain::new(largest_domain_size)?;

        let fft_precomputation = largest_mul_domain.precompute_fft();
        let ifft_precomputation = fft_precomputation.to_ifft_precomputation();
        Some((fft_precomputation, ifft_precomputation))
    }

    /// Construct the linear combinations that are checked by the AHP.
    /// Public input should be unformatted.
    /// We construct the linear combinations as per section 5 of our protocol documentation.
    /// We can distinguish between:
    /// (1) simple comitments: $\{\cm{g_A}, \cm{g_B}, \cm{g_C}\}$ and $\{\cm{\hat{z}_{B,i,j}}\}_{i \in {[\mathcal{D}]}}$, $\cm{g_1}$
    /// (2) virtual commitments for the lincheck_sumcheck and matrix_sumcheck. These are linear combinations of the simple commitments
    #[allow(non_snake_case)]
    pub fn construct_linear_combinations<E: EvaluationsProvider<F>>(
        public_inputs: &BTreeMap<CircuitId, Vec<Vec<F>>>,
        evals: &E,
        prover_third_message: &prover::ThirdMessage<F>,
        prover_fourth_message: &prover::FourthMessage<F>,
        state: &verifier::State<F, MM>,
    ) -> Result<BTreeMap<String, LinearCombination<F>>, AHPError> {
        assert!(!public_inputs.is_empty());
        let max_constraint_domain = state.max_constraint_domain;
        let max_variable_domain = state.max_variable_domain;
        let max_non_zero_domain = state.max_non_zero_domain;
        let public_inputs = state
            .circuit_specific_states
            .iter()
            .map(|(circuit_id, circuit_state)| {
                let input_domain = circuit_state.input_domain;
                let public_inputs = public_inputs[circuit_id]
                    .iter()
                    .map(|p| {
                        let public_input = prover::ConstraintSystem::format_public_input(p);
                        Self::formatted_public_input_is_admissible(&public_input).map(|_| public_input)
                    })
                    .collect::<Result<Vec<_>, _>>();
                assert_eq!(public_inputs.as_ref().unwrap()[0].len(), input_domain.size());
                public_inputs
            })
            .collect::<Result<Vec<_>, _>>()?;

        let first_round_msg = state.first_round_message.as_ref().unwrap();
        let second_round_msg = state.second_round_message.as_ref().unwrap();
        let alpha = second_round_msg.alpha;
        let eta_b = second_round_msg.eta_b;
        let eta_c = second_round_msg.eta_c;
        let batch_combiners = &first_round_msg.batch_combiners;
        let sums_fourth_msg = &prover_fourth_message.sums;

        let batch_lineval_sum =
            prover_third_message.sum(batch_combiners, eta_b, eta_c) * state.max_variable_domain.size_inv;

        let verifier::FourthMessage { delta_a, delta_b, delta_c } = state.fourth_round_message.as_ref().unwrap();
        let beta = state.third_round_message.unwrap().beta;
        let gamma = state.gamma.unwrap();

        let mut linear_combinations = BTreeMap::new();
        let mut selectors = BTreeMap::new();

        // We're now going to calculate the rowcheck_zerocheck
        let rowcheck_time = start_timer!(|| "Rowcheck");

        let v_R_at_alpha_time = start_timer!(|| "v_R_at_alpha");
        let v_R_at_alpha = max_constraint_domain.evaluate_vanishing_polynomial(alpha);
        end_timer!(v_R_at_alpha_time);

        let rowcheck_zerocheck = {
            let mut rowcheck_zerocheck = LinearCombination::empty("rowcheck_zerocheck");
            for (i, (id, c)) in batch_combiners.iter().enumerate() {
                let mut circuit_term = LinearCombination::empty(format!("rowcheck_zerocheck term {id}"));
                let third_sums_i = &prover_third_message.sums[i];
                let circuit_state = &state.circuit_specific_states[id];

                for (j, instance_combiner) in c.instance_combiners.iter().enumerate() {
                    let mut rowcheck = LinearCombination::empty(format!("rowcheck term {id}"));
                    let sum_a_third = third_sums_i[j].sum_a;
                    let sum_b_third = third_sums_i[j].sum_b;
                    let sum_c_third = third_sums_i[j].sum_c;

                    rowcheck.add(sum_a_third * sum_b_third - sum_c_third, LCTerm::One);

                    circuit_term += (*instance_combiner, &rowcheck);
                }
                let constraint_domain = circuit_state.constraint_domain;
                let selector = selector_evals(&mut selectors, &max_constraint_domain, &constraint_domain, alpha);
                circuit_term *= selector;
                rowcheck_zerocheck += (c.circuit_combiner, &circuit_term);
            }
            rowcheck_zerocheck.add(-v_R_at_alpha, "h_0");
            rowcheck_zerocheck
        };

        debug_assert!(evals.get_lc_eval(&rowcheck_zerocheck, alpha)?.is_zero());
        linear_combinations.insert("rowcheck_zerocheck".into(), rowcheck_zerocheck);
        end_timer!(rowcheck_time);

        // Lineval sumcheck:
        let lineval_time = start_timer!(|| "Lineval");

        let g_1 = LinearCombination::new("g_1", [(F::one(), "g_1")]);

        let v_C_at_beta = max_variable_domain.evaluate_vanishing_polynomial(beta);
        let v_K_at_gamma = max_non_zero_domain.evaluate_vanishing_polynomial(gamma);

        let v_X_at_beta_time = start_timer!(|| "v_X_at_beta");
        let v_X_at_beta = state
            .circuit_specific_states
            .iter()
            .map(|(circuit_id, circuit_state)| {
                let v_X_i_at_beta = circuit_state.input_domain.evaluate_vanishing_polynomial(beta);
                (circuit_id, v_X_i_at_beta)
            })
            .collect::<BTreeMap<_, _>>();
        end_timer!(v_X_at_beta_time);

        let x_at_betas = state
            .circuit_specific_states
            .iter()
            .enumerate()
            .map(|(i, (circuit_id, circuit_state))| {
                let lag_at_beta = circuit_state.input_domain.evaluate_all_lagrange_coefficients(beta);
                let x_at_beta = public_inputs[i]
                    .iter()
                    .map(|x| x.iter().zip_eq(&lag_at_beta).map(|(x, l)| *x * l).sum::<F>())
                    .collect_vec();
                (circuit_id, x_at_beta)
            })
            .collect::<BTreeMap<_, _>>();

        let g_1_at_beta = evals.get_lc_eval(&g_1, beta)?;

        // We're now going to calculate the lineval_sumcheck
        let lineval_sumcheck = {
            let mut lineval_sumcheck = LinearCombination::empty("lineval_sumcheck");
            if MM::ZK {
                lineval_sumcheck.add(F::one(), "mask_poly");
            }
            for (i, (id, c)) in batch_combiners.iter().enumerate() {
                let mut circuit_term = LinearCombination::empty(format!("lineval_sumcheck term {id}"));
                let fourth_sums_i = &sums_fourth_msg[i];
                let circuit_state = &state.circuit_specific_states[id];

                for (j, instance_combiner) in c.instance_combiners.iter().enumerate() {
                    let w_j = witness_label(*id, "w", j);
                    let mut lineval = LinearCombination::empty(format!("lineval term {j}"));
                    let sum_a_fourth = fourth_sums_i.sum_a * circuit_state.non_zero_a_domain.size_as_field_element;
                    let sum_b_fourth = fourth_sums_i.sum_b * circuit_state.non_zero_b_domain.size_as_field_element;
                    let sum_c_fourth = fourth_sums_i.sum_c * circuit_state.non_zero_c_domain.size_as_field_element;

                    lineval.add(sum_a_fourth * x_at_betas[id][j], LCTerm::One);
                    lineval.add(sum_a_fourth * v_X_at_beta[id], w_j.clone());

                    lineval.add(sum_b_fourth * eta_b * x_at_betas[id][j], LCTerm::One);
                    lineval.add(sum_b_fourth * eta_b * v_X_at_beta[id], w_j.clone());

                    lineval.add(sum_c_fourth * eta_c * x_at_betas[id][j], LCTerm::One);
                    lineval.add(sum_c_fourth * eta_c * v_X_at_beta[id], w_j);

                    circuit_term += (*instance_combiner, &lineval);
                }
                let variable_domain = circuit_state.variable_domain;
                let selector = selector_evals(&mut selectors, &max_variable_domain, &variable_domain, beta);
                circuit_term *= selector;

                lineval_sumcheck += (c.circuit_combiner, &circuit_term);
            }
            lineval_sumcheck
                .add(-v_C_at_beta, "h_1")
                .add(-beta * g_1_at_beta, LCTerm::One)
                .add(-batch_lineval_sum, LCTerm::One);
            lineval_sumcheck
        };
        debug_assert!(evals.get_lc_eval(&lineval_sumcheck, beta)?.is_zero());

        linear_combinations.insert("g_1".into(), g_1);
        linear_combinations.insert("lineval_sumcheck".into(), lineval_sumcheck);
        end_timer!(lineval_time);

        //  Matrix sumcheck:
        let mut matrix_sumcheck = LinearCombination::empty("matrix_sumcheck");

        for (i, (&id, circuit_state)) in state.circuit_specific_states.iter().enumerate() {
            let v_R_i_at_alpha_time = start_timer!(|| format!("v_R_i_at_alpha {id}"));
            let v_R_i_at_alpha = circuit_state.constraint_domain.evaluate_vanishing_polynomial(alpha);
            end_timer!(v_R_i_at_alpha_time);

            let v_C_i_at_beta_time = start_timer!(|| format!("v_C_i_at_beta {id}"));
            let v_C_i_at_beta = circuit_state.variable_domain.evaluate_vanishing_polynomial(beta);
            end_timer!(v_C_i_at_beta_time);

            let v_rc = v_R_i_at_alpha * v_C_i_at_beta;
            let RC = circuit_state.constraint_domain.size_as_field_element
                * circuit_state.variable_domain.size_as_field_element;

            let s_a = selector_evals(&mut selectors, &max_non_zero_domain, &circuit_state.non_zero_a_domain, gamma);
            let s_b = selector_evals(&mut selectors, &max_non_zero_domain, &circuit_state.non_zero_b_domain, gamma);
            let s_c = selector_evals(&mut selectors, &max_non_zero_domain, &circuit_state.non_zero_c_domain, gamma);

            let challenges = VerifierChallenges { alpha, beta, gamma };

            let d_a = delta_a[i];
            let d_b = delta_b[i];
            let d_c = delta_c[i];

            let g_a_label = witness_label(id, "g_a", 0);
            let g_a = LinearCombination::new(g_a_label.clone(), [(F::one(), g_a_label)]);
            let g_a_at_gamma = evals.get_lc_eval(&g_a, challenges.gamma)?;
            let g_b_label = witness_label(id, "g_b", 0);
            let g_b = LinearCombination::new(g_b_label.clone(), [(F::one(), g_b_label)]);
            let g_b_at_gamma = evals.get_lc_eval(&g_b, challenges.gamma)?;
            let g_c_label = witness_label(id, "g_c", 0);
            let g_c = LinearCombination::new(g_c_label.clone(), [(F::one(), g_c_label)]);
            let g_c_at_gamma = evals.get_lc_eval(&g_c, challenges.gamma)?;

            let sum_a = sums_fourth_msg[i].sum_a;
            let sum_b = sums_fourth_msg[i].sum_b;
            let sum_c = sums_fourth_msg[i].sum_c;

            Self::add_g_m_term(&mut matrix_sumcheck, id, "a", challenges, g_a_at_gamma, v_rc, d_a, sum_a, s_a, RC)?;
            Self::add_g_m_term(&mut matrix_sumcheck, id, "b", challenges, g_b_at_gamma, v_rc, d_b, sum_b, s_b, RC)?;
            Self::add_g_m_term(&mut matrix_sumcheck, id, "c", challenges, g_c_at_gamma, v_rc, d_c, sum_c, s_c, RC)?;

            linear_combinations.insert(g_a.label.clone(), g_a);
            linear_combinations.insert(g_b.label.clone(), g_b);
            linear_combinations.insert(g_c.label.clone(), g_c);
        }

        matrix_sumcheck -= &LinearCombination::new("h_2", [(v_K_at_gamma, "h_2")]);
        debug_assert!(evals.get_lc_eval(&matrix_sumcheck, gamma)?.is_zero());

        linear_combinations.insert("matrix_sumcheck".into(), matrix_sumcheck);

        Ok(linear_combinations)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_g_m_term(
        matrix_sumcheck: &mut LinearCombination<F>,
        id: CircuitId,
        m: &str,
        challenges: VerifierChallenges<F>,
        g_at_gamma: F,
        v_rc_at_alpha_beta: F,
        delta: F,
        sum: F,
        selector_at_gamma: F,
        rc_size: F,
    ) -> Result<(), AHPError> {
        let b_term = challenges.gamma * g_at_gamma + sum; // Xg_m(\gamma) + sum_m/|K_M|

        let lhs = Self::construct_lhs(id, m, challenges, v_rc_at_alpha_beta, b_term, selector_at_gamma, rc_size);
        *matrix_sumcheck += (delta, &lhs);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn construct_lhs(
        id: CircuitId,
        m: &str,
        challenges: VerifierChallenges<F>,
        v_rc_at_alpha_beta: F,
        b_term: F,
        selector_at_gamma: F,
        rc_size: F,
    ) -> LinearCombination<F> {
        let label_row = format!("circuit_{id}_row_{m}");
        let label_col = format!("circuit_{id}_col_{m}");
        let label_row_col_val = format!("circuit_{id}_row_col_val_{m}");
        let label_row_col = format!("circuit_{id}_row_col_{m}");
        let label_a_poly = format!("circuit_{id}_a_poly_{m}");
        let label_denom = format!("circuit_{id}_denom_{m}");

        // recall that row_col_val(X) is M_{i,j}*rowcol(X)
        let a = LinearCombination::new(label_a_poly, [(v_rc_at_alpha_beta, label_row_col_val)]);

        let alpha_beta = challenges.alpha * challenges.beta;
        let mut b = LinearCombination::new(label_denom, [
            (alpha_beta, LCTerm::One),
            (-challenges.alpha, (label_col).into()),
            (-challenges.beta, (label_row).into()),
            (F::one(), (label_row_col).into()),
        ]);

        b *= rc_size;
        b *= b_term; // Xg_m(X) + sum_m

        let mut lhs = a;
        lhs -= &b;
        lhs *= selector_at_gamma;
        lhs
    }
}

fn selector_evals<F: PrimeField>(
    cached_selector_evaluations: &mut BTreeMap<(u64, u64, F), F>,
    largest_domain: &EvaluationDomain<F>,
    target_domain: &EvaluationDomain<F>,
    challenge: F,
) -> F {
    *cached_selector_evaluations
        .entry((target_domain.size, largest_domain.size, challenge))
        .or_insert_with(|| largest_domain.evaluate_selector_polynomial(*target_domain, challenge))
}

/// Abstraction that provides evaluations of (linear combinations of) polynomials
///
/// Intended to provide a common interface for both the prover and the verifier
/// when constructing linear combinations via `AHPForR1CS::construct_linear_combinations`.
pub trait EvaluationsProvider<F: PrimeField>: core::fmt::Debug {
    /// Get the evaluation of linear combination `lc` at `point`.
    fn get_lc_eval(&self, lc: &LinearCombination<F>, point: F) -> Result<F, AHPError>;
}

/// The `EvaluationsProvider` used by the verifier
impl<F: PrimeField> EvaluationsProvider<F> for crate::polycommit::sonic_pc::Evaluations<F> {
    fn get_lc_eval(&self, lc: &LinearCombination<F>, point: F) -> Result<F, AHPError> {
        let key = (lc.label.clone(), point);
        self.get(&key).copied().ok_or_else(|| AHPError::MissingEval(lc.label.clone()))
    }
}

/// The `EvaluationsProvider` used by the prover
impl<F, T> EvaluationsProvider<F> for Vec<T>
where
    F: PrimeField,
    T: Borrow<LabeledPolynomial<F>> + core::fmt::Debug,
{
    fn get_lc_eval(&self, lc: &LinearCombination<F>, point: F) -> Result<F, AHPError> {
        let mut eval = F::zero();
        for (coeff, term) in lc.iter() {
            let value = if let LCTerm::PolyLabel(label) = term {
                self.iter()
                    .find(|p| (*p).borrow().label() == label)
                    .ok_or_else(|| AHPError::MissingEval(format!("Missing {} for {}", label, lc.label)))?
                    .borrow()
                    .evaluate(point)
            } else {
                assert!(term.is_one());
                F::one()
            };
            eval += &(*coeff * value)
        }
        Ok(eval)
    }
}

/// Given two domains H and K such that H \subseteq K,
/// construct polynomial that outputs 0 on all elements in K \ H, but 1 on all elements of H.
pub trait SelectorPolynomial<F: PrimeField> {
    fn evaluate_selector_polynomial(&self, other: EvaluationDomain<F>, point: F) -> F;
}

impl<F: PrimeField> SelectorPolynomial<F> for EvaluationDomain<F> {
    fn evaluate_selector_polynomial(&self, other: EvaluationDomain<F>, point: F) -> F {
        let numerator = self.evaluate_vanishing_polynomial(point) * other.size_as_field_element;
        let denominator = other.evaluate_vanishing_polynomial(point) * self.size_as_field_element;
        numerator / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fft::{DensePolynomial, Evaluations};
    use snarkvm_curves::bls12_377::fr::Fr;
    use snarkvm_fields::{One, Zero};
    use snarkvm_utilities::rand::TestRng;

    #[test]
    fn test_summation() {
        let rng = &mut TestRng::default();
        let size = 1 << 4;
        let domain = EvaluationDomain::<Fr>::new(1 << 4).unwrap();
        let size_as_fe = domain.size_as_field_element;
        let poly = DensePolynomial::rand(size, rng);

        let mut sum: Fr = Fr::zero();
        for eval in domain.elements().map(|e| poly.evaluate(e)) {
            sum += &eval;
        }
        let first = poly.coeffs[0] * size_as_fe;
        let last = *poly.coeffs.last().unwrap() * size_as_fe;
        assert_eq!(sum, first + last);
    }

    #[test]
    fn test_alternator_polynomial() {
        let mut rng = TestRng::default();

        for i in 1..10 {
            for j in 1..i {
                let domain_i = EvaluationDomain::<Fr>::new(1 << i).unwrap();
                let domain_j = EvaluationDomain::<Fr>::new(1 << j).unwrap();
                let point = domain_j.sample_element_outside_domain(&mut rng);
                let j_elements = domain_j.elements().collect::<Vec<_>>();
                let slow_selector = {
                    let evals = domain_i
                        .elements()
                        .map(|e| if j_elements.contains(&e) { Fr::one() } else { Fr::zero() })
                        .collect();
                    Evaluations::from_vec_and_domain(evals, domain_i).interpolate()
                };
                assert_eq!(slow_selector.evaluate(point), domain_i.evaluate_selector_polynomial(domain_j, point));

                for element in domain_i.elements() {
                    if j_elements.contains(&element) {
                        assert_eq!(slow_selector.evaluate(element), Fr::one(), "failed for {i} vs {j}");
                    } else {
                        assert_eq!(slow_selector.evaluate(element), Fr::zero());
                    }
                }
            }
        }
    }
}
