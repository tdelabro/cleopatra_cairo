use num_integer::Integer;
use std::borrow::Cow;
use std::cmp::{max, min};
use std::ops::Shl;

use num_bigint::BigInt;
use num_traits::{One, ToPrimitive, Zero};

use crate::bigint;
use crate::math_utils::safe_div;
use crate::types::instance_definitions::range_check_instance_def::CELLS_PER_RANGE_CHECK;
use crate::types::relocatable::{MaybeRelocatable, Relocatable};
use crate::vm::errors::memory_errors::MemoryError;
use crate::vm::errors::runner_errors::RunnerError;
use crate::vm::vm_core::VirtualMachine;
use crate::vm::vm_memory::memory::{Memory, ValidationRule};
use crate::vm::vm_memory::memory_segments::MemorySegmentManager;

pub struct RangeCheckBuiltinRunner {
    ratio: u32,
    base: isize,
    stop_ptr: Option<usize>,
    _cells_per_instance: u32,
    _n_input_cells: u32,
    inner_rc_bound: BigInt,
    pub _bound: BigInt,
    n_parts: u32,
}

impl RangeCheckBuiltinRunner {
    pub fn new(ratio: u32, n_parts: u32) -> RangeCheckBuiltinRunner {
        let inner_rc_bound = bigint!(1i32 << 16);
        RangeCheckBuiltinRunner {
            ratio,
            base: 0,
            stop_ptr: None,
            _cells_per_instance: CELLS_PER_RANGE_CHECK,
            _n_input_cells: CELLS_PER_RANGE_CHECK,
            inner_rc_bound: inner_rc_bound.clone(),
            _bound: inner_rc_bound.pow(n_parts),
            n_parts,
        }
    }

    pub fn initialize_segments(
        &mut self,
        segments: &mut MemorySegmentManager,
        memory: &mut Memory,
    ) {
        self.base = segments.add(memory).segment_index
    }

    pub fn initial_stack(&self) -> Vec<MaybeRelocatable> {
        vec![MaybeRelocatable::from((self.base, 0))]
    }

    pub fn base(&self) -> isize {
        self.base
    }

    pub fn add_validation_rule(&self, memory: &mut Memory) -> Result<(), RunnerError> {
        let rule: ValidationRule = ValidationRule(Box::new(
            |memory: &Memory,
             address: &MaybeRelocatable|
             -> Result<MaybeRelocatable, MemoryError> {
                match memory.get(address)? {
                    Some(Cow::Owned(MaybeRelocatable::Int(ref num)))
                    | Some(Cow::Borrowed(MaybeRelocatable::Int(ref num))) => {
                        if &BigInt::zero() <= num && num < &BigInt::one().shl(128u8) {
                            Ok(address.to_owned())
                        } else {
                            Err(MemoryError::NumOutOfBounds)
                        }
                    }
                    _ => Err(MemoryError::FoundNonInt),
                }
            },
        ));

        let segment_index: usize = self
            .base
            .try_into()
            .map_err(|_| RunnerError::RunnerInTemporarySegment(self.base))?;

        memory.add_validation_rule(segment_index, rule);

        Ok(())
    }

    pub fn deduce_memory_cell(
        &mut self,
        _address: &Relocatable,
        _memory: &Memory,
    ) -> Result<Option<MaybeRelocatable>, RunnerError> {
        Ok(None)
    }

    pub fn get_allocated_memory_units(&self, vm: &VirtualMachine) -> Result<usize, MemoryError> {
        let value = safe_div(&bigint!(vm.current_step), &bigint!(self.ratio))
            .map_err(|_| MemoryError::ErrorCalculatingMemoryUnits)?;
        match (self._cells_per_instance * value).to_usize() {
            Some(result) => Ok(result),
            _ => Err(MemoryError::ErrorCalculatingMemoryUnits),
        }
    }

    pub fn get_memory_segment_addresses(&self) -> (&'static str, (isize, Option<usize>)) {
        ("range_check", (self.base, self.stop_ptr))
    }

    pub fn get_range_check_usage(&self, memory: &Memory) -> Option<(BigInt, BigInt)> {
        let mut rc_bounds: Option<(BigInt, BigInt)> = None;
        let range_check_segment = memory.data.get(self.base as usize)?;
        for value in range_check_segment {
            //Split val into n_parts parts.
            for _ in 0..self.n_parts {
                let part_val = value
                    .as_ref()?
                    .get_int_ref()
                    .ok()?
                    .mod_floor(&self.inner_rc_bound);
                rc_bounds = Some(match rc_bounds {
                    None => (part_val.clone(), part_val),
                    Some((rc_min, rc_max)) => {
                        let rc_min = min(rc_min, part_val.clone());
                        let rc_max = max(rc_max, part_val);

                        (rc_min, rc_max)
                    }
                });
            }
        }
        rc_bounds
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
    use crate::serde::deserialize_program::ReferenceManager;
    use crate::types::program::Program;
    use crate::vm::runners::cairo_runner::CairoRunner;
    use crate::{bigint, utils::test_utils::*};
    use crate::{
        utils::test_utils::vm, vm::runners::builtin_runner::BuiltinRunner,
        vm::vm_core::VirtualMachine,
    };
    use num_bigint::Sign;

    #[test]
    fn get_allocated_memory_units() {
        let builtin = RangeCheckBuiltinRunner::new(10, 12);

        let mut vm = vm!();

        let program = Program {
            builtins: vec![String::from("pedersen")],
            prime: bigint!(17),
            data: vec_data!(
                (4612671182993129469_i64),
                (5189976364521848832_i64),
                (18446744073709551615_i128),
                (5199546496550207487_i64),
                (4612389712311386111_i64),
                (5198983563776393216_i64),
                (2),
                (2345108766317314046_i64),
                (5191102247248822272_i64),
                (5189976364521848832_i64),
                (7),
                (1226245742482522112_i64),
                ((
                    b"3618502788666131213697322783095070105623107215331596699973092056135872020470",
                    10
                )),
                (2345108766317314046_i64)
            ),
            constants: HashMap::new(),
            main: Some(8),
            hints: HashMap::new(),
            reference_manager: ReferenceManager {
                references: Vec::new(),
            },
            identifiers: HashMap::new(),
        };

        let mut cairo_runner = cairo_runner!(program);

        let hint_processor = BuiltinHintProcessor::new_empty();

        let address = cairo_runner.initialize(&mut vm).unwrap();

        cairo_runner
            .run_until_pc(address, &mut vm, &hint_processor)
            .unwrap();

        assert_eq!(builtin.get_allocated_memory_units(&vm), Ok(1));
    }

    #[test]
    fn initialize_segments_for_range_check() {
        let mut builtin = RangeCheckBuiltinRunner::new(8, 8);
        let mut segments = MemorySegmentManager::new();
        let mut memory = Memory::new();
        builtin.initialize_segments(&mut segments, &mut memory);
        assert_eq!(builtin.base, 0);
    }

    #[test]
    fn get_initial_stack_for_range_check_with_base() {
        let mut builtin = RangeCheckBuiltinRunner::new(8, 8);
        builtin.base = 1;
        let initial_stack = builtin.initial_stack();
        assert_eq!(
            initial_stack[0].clone(),
            MaybeRelocatable::RelocatableValue((builtin.base(), 0).into())
        );
        assert_eq!(initial_stack.len(), 1);
    }

    #[test]
    fn get_memory_segment_addresses() {
        let builtin = RangeCheckBuiltinRunner::new(8, 8);

        assert_eq!(
            builtin.get_memory_segment_addresses(),
            ("range_check", (0, None)),
        );
    }

    #[test]
    fn get_memory_accesses_missing_segment_used_sizes() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let vm = vm!();

        assert_eq!(
            builtin.get_memory_accesses(&vm),
            Err(MemoryError::MissingSegmentUsedSizes),
        );
    }

    #[test]
    fn get_memory_accesses_empty() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![0]);
        assert_eq!(builtin.get_memory_accesses(&vm), Ok(vec![]));
    }

    #[test]
    fn get_memory_accesses() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![4]);
        assert_eq!(
            builtin.get_memory_accesses(&vm),
            Ok(vec![
                (builtin.base(), 0).into(),
                (builtin.base(), 1).into(),
                (builtin.base(), 2).into(),
                (builtin.base(), 3).into(),
            ]),
        );
    }

    #[test]
    fn get_used_cells_missing_segment_used_sizes() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let vm = vm!();

        assert_eq!(
            builtin.get_used_cells(&vm),
            Err(MemoryError::MissingSegmentUsedSizes)
        );
    }

    #[test]
    fn get_used_cells_empty() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![0]);
        assert_eq!(builtin.get_used_cells(&vm), Ok(0));
    }

    #[test]
    fn get_used_cells() {
        let builtin = BuiltinRunner::RangeCheck(RangeCheckBuiltinRunner::new(256, 8));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![4]);
        assert_eq!(builtin.get_used_cells(&vm), Ok(4));
    }

    #[test]
    fn get_range_check_usage_succesful_a() {
        let builtin = RangeCheckBuiltinRunner::new(8, 8);
        let memory = memory![((0, 0), 1), ((0, 1), 2), ((0, 2), 3), ((0, 3), 4)];
        assert_eq!(
            builtin.get_range_check_usage(&memory),
            Some((bigint!(1), bigint!(4)))
        );
    }

    #[test]
    fn get_range_check_usage_succesful_b() {
        let builtin = RangeCheckBuiltinRunner::new(8, 8);
        let memory = memory![
            ((0, 0), 1465218365),
            ((0, 1), 2134570341),
            ((0, 2), 31349610736_i64),
            ((0, 3), 413468326585859_i64)
        ];
        assert_eq!(
            builtin.get_range_check_usage(&memory),
            Some((bigint!(6384), bigint!(62821)))
        );
    }

    #[test]
    fn get_range_check_usage_succesful_c() {
        let builtin = RangeCheckBuiltinRunner::new(8, 8);
        let memory = memory![
            ((0, 0), 634834751465218365_i64),
            ((0, 1), 42876922134570341_i64),
            ((0, 2), 23469831349610736_i64),
            ((0, 3), 23468413468326585859_i128),
            ((0, 4), 75346043276073460326_i128),
            ((0, 5), 87234598724867609478353436890268_i128)
        ];
        assert_eq!(
            builtin.get_range_check_usage(&memory),
            Some((bigint!(10480), bigint!(42341)))
        );
    }

    #[test]
    fn get_range_check_empty_memory() {
        let builtin = RangeCheckBuiltinRunner::new(8, 8);
        let memory = Memory::new();
        assert_eq!(builtin.get_range_check_usage(&memory), None);
    }
}