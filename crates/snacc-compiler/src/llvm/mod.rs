//! LLVM backend: checked program to native code (Specification 029 Phase 3).
//!
//! Type mapping, symbol tables, and import declarations live in
//! [`types`]/[`symbols`]/[`imports`]; lowering lives in [`lower`].

use crate::Optimization;
use crate::ast::NumLiteral;
use crate::ast::ParamMode;
use crate::checker::{
    ArithOp, CmpOp, LogicalOp, Place, PlaceRoot, Program, TArg, TBlock, TCleanup, TCondition,
    TExpr, TMethodCall, TParam, TReceiver, TStmt, TSumTypeTest, TTypeTest, TValueIf, Ty,
};
use crate::types::{BoxId, SumId, TypeDef, TypeId};
use inkwell::AddressSpace;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetData, TargetMachine,
};
use inkwell::types::{
    BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, IntType, StructType,
};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue, IntValue,
    PointerValue, StructValue,
};
use inkwell::{FloatPredicate, IntPredicate, OptimizationLevel};
use std::collections::{HashMap, HashSet};

pub(crate) mod imports;
pub(crate) mod lower;
pub(crate) mod symbols;
pub(crate) mod types;

pub(crate) use imports::*;
pub(crate) use lower::*;
pub(crate) use symbols::*;
pub(crate) use types::*;

/// Marks a backend failure that is a compiler bug rather than a property of the
/// program. Specification 010 section 19 phase 5 step 7 requires an LLVM
/// verifier failure -- and, by the same reasoning, every "the checker promised
/// this" violation -- to be classified as an internal compiler error rather
/// than an ordinary backend diagnostic.
pub const INTERNAL_ERROR: &str = "internal compiler error: ";

fn internal(message: impl std::fmt::Display) -> String {
    format!("{INTERNAL_ERROR}{message}")
}

/// Returns the host triple used for native object emission.
pub fn target_triple() -> String {
    TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}

pub fn llvm_version() -> (u32, u32, u32) {
    let mut major = 0;
    let mut minor = 0;
    let mut patch = 0;
    // LLVMGetVersion initializes all three out-parameters and does not retain
    // their addresses.
    unsafe {
        inkwell::llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, &mut patch);
    }
    (major, minor, patch)
}

fn host_machine(optimization: Optimization) -> Result<TargetMachine, String> {
    Target::initialize_x86(&InitializationConfig::default());

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|error| error.to_string())?;
    let optimization = match optimization {
        Optimization::None => OptimizationLevel::None,
        Optimization::Aggressive => OptimizationLevel::Aggressive,
    };
    target
        .create_target_machine(
            &triple,
            "generic",
            "",
            optimization,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| "LLVM could not create a target machine for this host".to_string())
}

/// Compiles the checked program directly to a native object for the host.
pub fn compile(
    program: &Program,
    module_name: &str,
    optimization: Optimization,
) -> Result<(Vec<u8>, String), String> {
    let machine = host_machine(optimization)?;
    let context = Context::create();
    let module = build_module(&context, program, module_name, &machine)?;
    let object = machine
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|error| error.to_string())?;
    Ok((object.as_slice().to_vec(), target_triple()))
}

/// Renders a checked program as LLVM IR. Calling-convention attributes exist
/// only in the IR -- no object file preserves them -- so this is how they are
/// verified.
pub fn compile_to_ir(program: &Program, module_name: &str) -> Result<String, String> {
    let machine = host_machine(Optimization::None)?;
    let context = Context::create();
    let module = build_module(&context, program, module_name, &machine)?;
    Ok(module.print_to_string().to_string())
}
