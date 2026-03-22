pub mod ast;
pub mod bytecode;
pub mod compiler;
pub mod error;
pub mod evaluator;
pub mod lexer;
pub mod native;
pub mod package;
pub mod parser;
pub mod semantic;
pub mod token;
pub mod vm;

pub use bytecode::{disassemble, BytecodeFunction, BytecodeProgram, Instruction};
pub use compiler::{
	compile_program, compile_program_with_options, render_contract_report_text, CompileError,
	CompileOptions, CompileResult, ContractCompileReport, CycleProfile, OptimizationLevel,
};
pub use error::LexerError;
pub use evaluator::{run_program, RuntimeError, Value};
pub use lexer::Lexer;
pub use native::{
	compile_h_to_native_artifacts, compile_h_to_native_artifacts_with_options,
	compile_h_to_native_binary, compile_h_to_native_binary_with_options, NativeBuildArtifacts,
	NativeCompileError,
};
pub use package::{read_package, write_package, PackageError};
pub use parser::{parse_source, ParseError, Parser};
pub use semantic::{analyze, SemanticError};
pub use token::{Span, Token, TokenKind};
pub use vm::{run_bytecode, VmError};
