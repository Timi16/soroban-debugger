//! WASM instruction representation and parsing for debugger stepping

use std::fmt;
use wasmparser::Operator;

/// Owned, lifetime-free representation of the WASM operators the debugger
/// inspects. `wasmparser::Operator` borrows from the module bytes, so storing
/// it past the parse would require the bytes to stay alive; this type copies
/// out the immediates the debugger actually uses instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OwnedOperator {
    Unreachable,
    Nop,
    Block,
    Loop,
    If,
    Else,
    End,
    Br {
        relative_depth: u32,
    },
    BrIf {
        relative_depth: u32,
    },
    BrTable,
    Return,
    Call {
        function_index: u32,
    },
    CallIndirect,
    Drop,
    Select,
    LocalGet {
        local_index: u32,
    },
    LocalSet {
        local_index: u32,
    },
    LocalTee {
        local_index: u32,
    },
    GlobalGet {
        global_index: u32,
    },
    GlobalSet {
        global_index: u32,
    },
    MemorySize,
    MemoryGrow,
    I32Const {
        value: i32,
    },
    I64Const {
        value: i64,
    },
    F32Const {
        bits: u32,
    },
    F64Const {
        bits: u64,
    },
    /// Any memory load, e.g. `i32.load8_u`.
    Load {
        name: &'static str,
        offset: u64,
        align: u8,
    },
    /// Any memory store, e.g. `i64.store32`.
    Store {
        name: &'static str,
        offset: u64,
        align: u8,
    },
    /// Any other operator the debugger only needs a display name for,
    /// e.g. comparisons and arithmetic.
    Simple {
        name: &'static str,
    },
    /// Operators the debugger has no special handling for.
    Other,
}

impl From<Operator<'_>> for OwnedOperator {
    fn from(op: Operator<'_>) -> Self {
        Self::from(&op)
    }
}

impl From<&Operator<'_>> for OwnedOperator {
    fn from(op: &Operator<'_>) -> Self {
        use OwnedOperator as O;
        match op {
            Operator::Unreachable => O::Unreachable,
            Operator::Nop => O::Nop,
            Operator::Block { .. } => O::Block,
            Operator::Loop { .. } => O::Loop,
            Operator::If { .. } => O::If,
            Operator::Else => O::Else,
            Operator::End => O::End,
            Operator::Br { relative_depth } => O::Br {
                relative_depth: *relative_depth,
            },
            Operator::BrIf { relative_depth } => O::BrIf {
                relative_depth: *relative_depth,
            },
            Operator::BrTable { .. } => O::BrTable,
            Operator::Return => O::Return,
            Operator::Call { function_index } => O::Call {
                function_index: *function_index,
            },
            Operator::CallIndirect { .. } => O::CallIndirect,
            Operator::Drop => O::Drop,
            Operator::Select => O::Select,
            Operator::LocalGet { local_index } => O::LocalGet {
                local_index: *local_index,
            },
            Operator::LocalSet { local_index } => O::LocalSet {
                local_index: *local_index,
            },
            Operator::LocalTee { local_index } => O::LocalTee {
                local_index: *local_index,
            },
            Operator::GlobalGet { global_index } => O::GlobalGet {
                global_index: *global_index,
            },
            Operator::GlobalSet { global_index } => O::GlobalSet {
                global_index: *global_index,
            },
            Operator::MemorySize { .. } => O::MemorySize,
            Operator::MemoryGrow { .. } => O::MemoryGrow,
            Operator::I32Const { value } => O::I32Const { value: *value },
            Operator::I64Const { value } => O::I64Const { value: *value },
            Operator::F32Const { value } => O::F32Const { bits: value.bits() },
            Operator::F64Const { value } => O::F64Const { bits: value.bits() },
            Operator::I32Load { memarg } => O::load("i32.load", memarg),
            Operator::I64Load { memarg } => O::load("i64.load", memarg),
            Operator::F32Load { memarg } => O::load("f32.load", memarg),
            Operator::F64Load { memarg } => O::load("f64.load", memarg),
            Operator::I32Load8S { memarg } => O::load("i32.load8_s", memarg),
            Operator::I32Load8U { memarg } => O::load("i32.load8_u", memarg),
            Operator::I32Load16S { memarg } => O::load("i32.load16_s", memarg),
            Operator::I32Load16U { memarg } => O::load("i32.load16_u", memarg),
            Operator::I64Load8S { memarg } => O::load("i64.load8_s", memarg),
            Operator::I64Load8U { memarg } => O::load("i64.load8_u", memarg),
            Operator::I64Load16S { memarg } => O::load("i64.load16_s", memarg),
            Operator::I64Load16U { memarg } => O::load("i64.load16_u", memarg),
            Operator::I64Load32S { memarg } => O::load("i64.load32_s", memarg),
            Operator::I64Load32U { memarg } => O::load("i64.load32_u", memarg),
            Operator::I32Store { memarg } => O::store("i32.store", memarg),
            Operator::I64Store { memarg } => O::store("i64.store", memarg),
            Operator::F32Store { memarg } => O::store("f32.store", memarg),
            Operator::F64Store { memarg } => O::store("f64.store", memarg),
            Operator::I32Store8 { memarg } => O::store("i32.store8", memarg),
            Operator::I32Store16 { memarg } => O::store("i32.store16", memarg),
            Operator::I64Store8 { memarg } => O::store("i64.store8", memarg),
            Operator::I64Store16 { memarg } => O::store("i64.store16", memarg),
            Operator::I64Store32 { memarg } => O::store("i64.store32", memarg),
            Operator::I32Eqz => O::Simple { name: "i32.eqz" },
            Operator::I32Eq => O::Simple { name: "i32.eq" },
            Operator::I32Ne => O::Simple { name: "i32.ne" },
            Operator::I32LtS => O::Simple { name: "i32.lt_s" },
            Operator::I32LtU => O::Simple { name: "i32.lt_u" },
            Operator::I32GtS => O::Simple { name: "i32.gt_s" },
            Operator::I32GtU => O::Simple { name: "i32.gt_u" },
            Operator::I32LeS => O::Simple { name: "i32.le_s" },
            Operator::I32LeU => O::Simple { name: "i32.le_u" },
            Operator::I32GeS => O::Simple { name: "i32.ge_s" },
            Operator::I32GeU => O::Simple { name: "i32.ge_u" },
            Operator::I64Eqz => O::Simple { name: "i64.eqz" },
            Operator::I64Eq => O::Simple { name: "i64.eq" },
            Operator::I64Ne => O::Simple { name: "i64.ne" },
            Operator::I64LtS => O::Simple { name: "i64.lt_s" },
            Operator::I64LtU => O::Simple { name: "i64.lt_u" },
            Operator::I64GtS => O::Simple { name: "i64.gt_s" },
            Operator::I64GtU => O::Simple { name: "i64.gt_u" },
            Operator::I64LeS => O::Simple { name: "i64.le_s" },
            Operator::I64LeU => O::Simple { name: "i64.le_u" },
            Operator::I64GeS => O::Simple { name: "i64.ge_s" },
            Operator::I64GeU => O::Simple { name: "i64.ge_u" },
            Operator::F32Eq => O::Simple { name: "f32.eq" },
            Operator::F32Ne => O::Simple { name: "f32.ne" },
            Operator::F32Lt => O::Simple { name: "f32.lt" },
            Operator::F32Gt => O::Simple { name: "f32.gt" },
            Operator::F32Le => O::Simple { name: "f32.le" },
            Operator::F32Ge => O::Simple { name: "f32.ge" },
            Operator::F64Eq => O::Simple { name: "f64.eq" },
            Operator::F64Ne => O::Simple { name: "f64.ne" },
            Operator::F64Lt => O::Simple { name: "f64.lt" },
            Operator::F64Gt => O::Simple { name: "f64.gt" },
            Operator::F64Le => O::Simple { name: "f64.le" },
            Operator::F64Ge => O::Simple { name: "f64.ge" },
            Operator::I32Clz => O::Simple { name: "i32.clz" },
            Operator::I32Ctz => O::Simple { name: "i32.ctz" },
            Operator::I32Popcnt => O::Simple { name: "i32.popcnt" },
            Operator::I32Add => O::Simple { name: "i32.add" },
            Operator::I32Sub => O::Simple { name: "i32.sub" },
            Operator::I32Mul => O::Simple { name: "i32.mul" },
            Operator::I32DivS => O::Simple { name: "i32.div_s" },
            Operator::I32DivU => O::Simple { name: "i32.div_u" },
            Operator::I32RemS => O::Simple { name: "i32.rem_s" },
            Operator::I32RemU => O::Simple { name: "i32.rem_u" },
            Operator::I32And => O::Simple { name: "i32.and" },
            Operator::I32Or => O::Simple { name: "i32.or" },
            Operator::I32Xor => O::Simple { name: "i32.xor" },
            Operator::I32Shl => O::Simple { name: "i32.shl" },
            Operator::I32ShrS => O::Simple { name: "i32.shr_s" },
            Operator::I32ShrU => O::Simple { name: "i32.shr_u" },
            Operator::I32Rotl => O::Simple { name: "i32.rotl" },
            Operator::I32Rotr => O::Simple { name: "i32.rotr" },
            _ => O::Other,
        }
    }
}

impl OwnedOperator {
    fn load(name: &'static str, memarg: &wasmparser::MemArg) -> Self {
        Self::Load {
            name,
            offset: memarg.offset,
            align: memarg.align,
        }
    }

    fn store(name: &'static str, memarg: &wasmparser::MemArg) -> Self {
        Self::Store {
            name,
            offset: memarg.offset,
            align: memarg.align,
        }
    }
}

/// Represents a single WASM instruction with debugging context
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Byte offset in the WASM binary
    pub offset: usize,
    /// The WASM operator/opcode
    pub operator: OwnedOperator,
    /// Function index this instruction belongs to
    pub function_index: u32,
    /// Local instruction index within the function
    pub local_index: u32,
}

impl Instruction {
    /// Create a new instruction
    pub fn new(
        offset: usize,
        operator: impl Into<OwnedOperator>,
        function_index: u32,
        local_index: u32,
    ) -> Self {
        Self {
            offset,
            operator: operator.into(),
            function_index,
            local_index,
        }
    }

    /// Get the instruction name
    pub fn name(&self) -> &'static str {
        match &self.operator {
            OwnedOperator::Unreachable => "unreachable",
            OwnedOperator::Nop => "nop",
            OwnedOperator::Block => "block",
            OwnedOperator::Loop => "loop",
            OwnedOperator::If => "if",
            OwnedOperator::Else => "else",
            OwnedOperator::End => "end",
            OwnedOperator::Br { .. } => "br",
            OwnedOperator::BrIf { .. } => "br_if",
            OwnedOperator::BrTable => "br_table",
            OwnedOperator::Return => "return",
            OwnedOperator::Call { .. } => "call",
            OwnedOperator::CallIndirect => "call_indirect",
            OwnedOperator::Drop => "drop",
            OwnedOperator::Select => "select",
            OwnedOperator::LocalGet { .. } => "local.get",
            OwnedOperator::LocalSet { .. } => "local.set",
            OwnedOperator::LocalTee { .. } => "local.tee",
            OwnedOperator::GlobalGet { .. } => "global.get",
            OwnedOperator::GlobalSet { .. } => "global.set",
            OwnedOperator::MemorySize => "memory.size",
            OwnedOperator::MemoryGrow => "memory.grow",
            OwnedOperator::I32Const { .. } => "i32.const",
            OwnedOperator::I64Const { .. } => "i64.const",
            OwnedOperator::F32Const { .. } => "f32.const",
            OwnedOperator::F64Const { .. } => "f64.const",
            OwnedOperator::Load { name, .. }
            | OwnedOperator::Store { name, .. }
            | OwnedOperator::Simple { name } => name,
            OwnedOperator::Other => "unknown",
        }
    }

    /// Get operand description for display
    pub fn operands(&self) -> String {
        match &self.operator {
            OwnedOperator::Call { function_index } => format!("func_{}", function_index),
            OwnedOperator::LocalGet { local_index }
            | OwnedOperator::LocalSet { local_index }
            | OwnedOperator::LocalTee { local_index } => {
                format!("${}", local_index)
            }
            OwnedOperator::GlobalGet { global_index }
            | OwnedOperator::GlobalSet { global_index } => {
                format!("global_{}", global_index)
            }
            OwnedOperator::Br { relative_depth } | OwnedOperator::BrIf { relative_depth } => {
                format!("{}", relative_depth)
            }
            OwnedOperator::I32Const { value } => format!("{}", value),
            OwnedOperator::I64Const { value } => format!("{}", value),
            OwnedOperator::F32Const { bits } => format!("{}", f32::from_bits(*bits)),
            OwnedOperator::F64Const { bits } => format!("{}", f64::from_bits(*bits)),
            OwnedOperator::Load { offset, align, .. }
            | OwnedOperator::Store { offset, align, .. } => {
                format!("offset={} align={}", offset, align)
            }
            _ => String::new(),
        }
    }

    /// Check if this instruction is a control flow instruction
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self.operator,
            OwnedOperator::Br { .. }
                | OwnedOperator::BrIf { .. }
                | OwnedOperator::BrTable
                | OwnedOperator::Return
                | OwnedOperator::Call { .. }
                | OwnedOperator::CallIndirect
                | OwnedOperator::If
                | OwnedOperator::Else
                | OwnedOperator::End
        )
    }

    /// Check if this instruction is a function call
    pub fn is_call(&self) -> bool {
        matches!(
            self.operator,
            OwnedOperator::Call { .. } | OwnedOperator::CallIndirect
        )
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operands = self.operands();
        if operands.is_empty() {
            write!(f, "{:08x}: {}", self.offset, self.name())
        } else {
            write!(f, "{:08x}: {} {}", self.offset, self.name(), operands)
        }
    }
}

/// Parser for WASM instructions
pub struct InstructionParser {
    instructions: Vec<Instruction>,
}

impl InstructionParser {
    /// Create a new instruction parser
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
        }
    }

    /// Parse WASM bytecode and extract all instructions
    pub fn parse(&mut self, wasm_bytes: &[u8]) -> Result<&[Instruction], String> {
        use wasmparser::{Parser, Payload};

        self.instructions.clear();

        let parser = Parser::new(0);

        for payload in parser.parse_all(wasm_bytes) {
            let payload = payload.map_err(|e| format!("WASM parsing error: {}", e))?;

            if let Payload::CodeSectionEntry(body) = payload {
                let function_index = self.instructions.len() as u32; // Simplified function indexing
                self.parse_function_body(body, function_index)?;
            }
        }

        Ok(&self.instructions)
    }

    /// Parse a single function body
    fn parse_function_body(
        &mut self,
        body: wasmparser::FunctionBody,
        function_index: u32,
    ) -> Result<(), String> {
        let mut reader = body
            .get_operators_reader()
            .map_err(|e| format!("Failed to get operators reader: {}", e))?;

        let mut local_index = 0;

        while !reader.eof() {
            let offset = reader.original_position();
            let op = reader
                .read()
                .map_err(|e| format!("Failed to read operator: {}", e))?;

            let instruction = Instruction::new(offset, &op, function_index, local_index);
            self.instructions.push(instruction);

            local_index += 1;
        }

        Ok(())
    }

    /// Get parsed instructions
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
}

impl Default for InstructionParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_display() {
        let inst = Instruction::new(0x100, Operator::I32Const { value: 42 }, 0, 0);
        assert_eq!(format!("{}", inst), "00000100: i32.const 42");
    }

    #[test]
    fn test_instruction_operands() {
        let inst = Instruction::new(0x100, Operator::LocalGet { local_index: 5 }, 0, 0);
        assert_eq!(inst.operands(), "$5");
    }

    #[test]
    fn test_control_flow_detection() {
        let call_inst = Instruction::new(0x100, Operator::Call { function_index: 1 }, 0, 0);
        assert!(call_inst.is_control_flow());
        assert!(call_inst.is_call());

        let add_inst = Instruction::new(0x104, Operator::I32Add, 0, 1);
        assert!(!add_inst.is_control_flow());
        assert!(!add_inst.is_call());
    }

    #[test]
    fn test_parsed_instructions_outlive_module_bytes() {
        // Minimal module with one function: (func (result i32) i32.const 7)
        let wasm = wat_module();
        let mut parser = InstructionParser::new();
        parser.parse(&wasm).expect("parse should succeed");
        drop(wasm);

        // Instructions must stay valid after the module bytes are gone.
        let printed: Vec<String> = parser
            .instructions()
            .iter()
            .map(|inst| inst.to_string())
            .collect();
        assert!(!printed.is_empty());
        assert!(printed.iter().any(|line| line.contains("i32.const")));
    }

    fn wat_module() -> Vec<u8> {
        // Hand-assembled module: (module (func (result i32) i32.const 7 end))
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // type section: () -> i32
            0x03, 0x02, 0x01, 0x00, // function section
            0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x07, 0x0b, // code: i32.const 7; end
        ]
    }
}
