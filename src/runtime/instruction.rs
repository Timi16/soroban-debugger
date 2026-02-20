//! WASM instruction representation and parsing for debugger stepping

use std::fmt;
use wasmparser::Operator;

/// Represents a single WASM instruction with debugging context
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Byte offset in the WASM binary
    pub offset: usize,
    /// The WASM operator/opcode
    pub operator: Operator<'static>,
    /// Function index this instruction belongs to
    pub function_index: u32,
    /// Local instruction index within the function
    pub local_index: u32,
}

impl Instruction {
    /// Create a new instruction
    pub fn new(
        offset: usize,
        operator: Operator<'static>,
        function_index: u32,
        local_index: u32,
    ) -> Self {
        Self {
            offset,
            operator,
            function_index,
            local_index,
        }
    }

    /// Get the instruction name
    pub fn name(&self) -> &'static str {
        match &self.operator {
            Operator::Unreachable => "unreachable",
            Operator::Nop => "nop",
            Operator::Block { .. } => "block",
            Operator::Loop { .. } => "loop",
            Operator::If { .. } => "if",
            Operator::Else => "else",
            Operator::End => "end",
            Operator::Br { .. } => "br",
            Operator::BrIf { .. } => "br_if",
            Operator::BrTable { .. } => "br_table",
            Operator::Return => "return",
            Operator::Call { .. } => "call",
            Operator::CallIndirect { .. } => "call_indirect",
            Operator::Drop => "drop",
            Operator::Select => "select",
            Operator::LocalGet { .. } => "local.get",
            Operator::LocalSet { .. } => "local.set",
            Operator::LocalTee { .. } => "local.tee",
            Operator::GlobalGet { .. } => "global.get",
            Operator::GlobalSet { .. } => "global.set",
            Operator::I32Load { .. } => "i32.load",
            Operator::I64Load { .. } => "i64.load",
            Operator::F32Load { .. } => "f32.load",
            Operator::F64Load { .. } => "f64.load",
            Operator::I32Load8S { .. } => "i32.load8_s",
            Operator::I32Load8U { .. } => "i32.load8_u",
            Operator::I32Load16S { .. } => "i32.load16_s",
            Operator::I32Load16U { .. } => "i32.load16_u",
            Operator::I64Load8S { .. } => "i64.load8_s",
            Operator::I64Load8U { .. } => "i64.load8_u",
            Operator::I64Load16S { .. } => "i64.load16_s",
            Operator::I64Load16U { .. } => "i64.load16_u",
            Operator::I64Load32S { .. } => "i64.load32_s",
            Operator::I64Load32U { .. } => "i64.load32_u",
            Operator::I32Store { .. } => "i32.store",
            Operator::I64Store { .. } => "i64.store",
            Operator::F32Store { .. } => "f32.store",
            Operator::F64Store { .. } => "f64.store",
            Operator::I32Store8 { .. } => "i32.store8",
            Operator::I32Store16 { .. } => "i32.store16",
            Operator::I64Store8 { .. } => "i64.store8",
            Operator::I64Store16 { .. } => "i64.store16",
            Operator::I64Store32 { .. } => "i64.store32",
            Operator::MemorySize { .. } => "memory.size",
            Operator::MemoryGrow { .. } => "memory.grow",
            Operator::I32Const { .. } => "i32.const",
            Operator::I64Const { .. } => "i64.const",
            Operator::F32Const { .. } => "f32.const",
            Operator::F64Const { .. } => "f64.const",
            Operator::I32Eqz => "i32.eqz",
            Operator::I32Eq => "i32.eq",
            Operator::I32Ne => "i32.ne",
            Operator::I32LtS => "i32.lt_s",
            Operator::I32LtU => "i32.lt_u",
            Operator::I32GtS => "i32.gt_s",
            Operator::I32GtU => "i32.gt_u",
            Operator::I32LeS => "i32.le_s",
            Operator::I32LeU => "i32.le_u",
            Operator::I32GeS => "i32.ge_s",
            Operator::I32GeU => "i32.ge_u",
            Operator::I64Eqz => "i64.eqz",
            Operator::I64Eq => "i64.eq",
            Operator::I64Ne => "i64.ne",
            Operator::I64LtS => "i64.lt_s",
            Operator::I64LtU => "i64.lt_u",
            Operator::I64GtS => "i64.gt_s",
            Operator::I64GtU => "i64.gt_u",
            Operator::I64LeS => "i64.le_s",
            Operator::I64LeU => "i64.le_u",
            Operator::I64GeS => "i64.ge_s",
            Operator::I64GeU => "i64.ge_u",
            Operator::F32Eq => "f32.eq",
            Operator::F32Ne => "f32.ne",
            Operator::F32Lt => "f32.lt",
            Operator::F32Gt => "f32.gt",
            Operator::F32Le => "f32.le",
            Operator::F32Ge => "f32.ge",
            Operator::F64Eq => "f64.eq",
            Operator::F64Ne => "f64.ne",
            Operator::F64Lt => "f64.lt",
            Operator::F64Gt => "f64.gt",
            Operator::F64Le => "f64.le",
            Operator::F64Ge => "f64.ge",
            Operator::I32Clz => "i32.clz",
            Operator::I32Ctz => "i32.ctz",
            Operator::I32Popcnt => "i32.popcnt",
            Operator::I32Add => "i32.add",
            Operator::I32Sub => "i32.sub",
            Operator::I32Mul => "i32.mul",
            Operator::I32DivS => "i32.div_s",
            Operator::I32DivU => "i32.div_u",
            Operator::I32RemS => "i32.rem_s",
            Operator::I32RemU => "i32.rem_u",
            Operator::I32And => "i32.and",
            Operator::I32Or => "i32.or",
            Operator::I32Xor => "i32.xor",
            Operator::I32Shl => "i32.shl",
            Operator::I32ShrS => "i32.shr_s",
            Operator::I32ShrU => "i32.shr_u",
            Operator::I32Rotl => "i32.rotl",
            Operator::I32Rotr => "i32.rotr",
            _ => "unknown",
        }
    }

    /// Get operand description for display
    pub fn operands(&self) -> String {
        match &self.operator {
            Operator::Call { function_index } => format!("func_{}", function_index),
            Operator::LocalGet { local_index }
            | Operator::LocalSet { local_index }
            | Operator::LocalTee { local_index } => {
                format!("${}", local_index)
            }
            Operator::GlobalGet { global_index } | Operator::GlobalSet { global_index } => {
                format!("global_{}", global_index)
            }
            Operator::Br { relative_depth } | Operator::BrIf { relative_depth } => {
                format!("{}", relative_depth)
            }
            Operator::I32Const { value } => format!("{}", value),
            Operator::I64Const { value } => format!("{}", value),
            Operator::F32Const { value } => format!("{}", f32::from_bits(value.bits())),
            Operator::F64Const { value } => format!("{}", f64::from_bits(value.bits())),
            Operator::I32Load { memarg } | Operator::I32Store { memarg } => {
                format!("offset={} align={}", memarg.offset, memarg.align)
            }
            _ => String::new(),
        }
    }

    /// Check if this instruction is a control flow instruction
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self.operator,
            Operator::Br { .. }
                | Operator::BrIf { .. }
                | Operator::BrTable { .. }
                | Operator::Return
                | Operator::Call { .. }
                | Operator::CallIndirect { .. }
                | Operator::If { .. }
                | Operator::Else
                | Operator::End
        )
    }

    /// Check if this instruction is a function call
    pub fn is_call(&self) -> bool {
        matches!(
            self.operator,
            Operator::Call { .. } | Operator::CallIndirect { .. }
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

            // Convert to owned operator for storage
            let owned_op = self.make_owned_operator(op);

            let instruction = Instruction::new(offset, owned_op, function_index, local_index);
            self.instructions.push(instruction);

            local_index += 1;
        }

        Ok(())
    }

    /// Convert borrowed operator to owned for storage
    fn make_owned_operator(&self, op: Operator) -> Operator<'static> {
        // This is a simplified conversion - in practice you'd need to handle all operators
        // For now, we'll use unsafe transmutation as a workaround for the lifetime issue
        unsafe { std::mem::transmute(op) }
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
}
