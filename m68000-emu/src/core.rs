mod load;

use crate::traits::{BusInterface, GetBit, SignBit};

#[derive(Debug, Clone, Copy)]
struct ConditionCodes {
    carry: bool,
    overflow: bool,
    zero: bool,
    negative: bool,
    extend: bool,
}

impl From<u8> for ConditionCodes {
    fn from(value: u8) -> Self {
        Self {
            carry: value.bit(0),
            overflow: value.bit(1),
            zero: value.bit(2),
            negative: value.bit(3),
            extend: value.bit(4),
        }
    }
}

impl From<ConditionCodes> for u8 {
    fn from(value: ConditionCodes) -> Self {
        (u8::from(value.extend) << 4)
            | (u8::from(value.negative) << 3)
            | (u8::from(value.zero) << 2)
            | (u8::from(value.overflow) << 1)
            | u8::from(value.carry)
    }
}

#[derive(Debug, Clone)]
struct Registers {
    data: [u32; 8],
    address: [u32; 7],
    usp: u32,
    ssp: u32,
    pc: u32,
    ccr: ConditionCodes,
    interrupt_priority_mask: u8,
    supervisor_mode: bool,
    trace_enabled: bool,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            data: [0; 8],
            address: [0; 7],
            usp: 0,
            ssp: 0,
            pc: 0,
            ccr: 0.into(),
            interrupt_priority_mask: 0,
            supervisor_mode: true,
            trace_enabled: false,
        }
    }

    fn status_register(&self) -> u16 {
        let lsb: u8 = self.ccr.into();
        let msb = self.interrupt_priority_mask
            | (u8::from(self.supervisor_mode) << 5)
            | (u8::from(self.trace_enabled) << 7);

        u16::from_be_bytes([msb, lsb])
    }

    fn set_status_register(&mut self, value: u16) {
        let [msb, lsb] = value.to_be_bytes();

        self.interrupt_priority_mask = msb & 0x07;
        self.supervisor_mode = msb.bit(5);
        self.trace_enabled = msb.bit(7);

        self.ccr = lsb.into();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DataRegister(u8);

impl DataRegister {
    fn read_from(self, registers: &Registers) -> u32 {
        registers.data[self.0 as usize]
    }

    fn write_byte_to(self, registers: &mut Registers, value: u8) {
        let existing_value = registers.data[self.0 as usize];
        registers.data[self.0 as usize] = (existing_value & 0xFFFF_FF00) | u32::from(value);
    }

    fn write_word_to(self, registers: &mut Registers, value: u16) {
        let existing_value = registers.data[self.0 as usize];
        registers.data[self.0 as usize] = (existing_value & 0xFFFF_0000) | u32::from(value);
    }

    fn write_long_word_to(self, registers: &mut Registers, value: u32) {
        registers.data[self.0 as usize] = value;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressRegister(u8);

impl AddressRegister {
    fn is_stack_pointer(self) -> bool {
        self.0 == 7
    }

    fn read_from(self, registers: &Registers) -> u32 {
        match (self.0, registers.supervisor_mode) {
            (7, false) => registers.usp,
            (7, true) => registers.ssp,
            (register, _) => registers.address[register as usize],
        }
    }

    #[allow(clippy::unused_self)]
    fn write_byte_to(self, _registers: &mut Registers, _value: u8) {
        panic!("Writing a byte to an address register is not supported");
    }

    fn write_word_to(self, registers: &mut Registers, value: u16) {
        // Address register writes are always sign extended to 32 bits
        self.write_long_word_to(registers, value as i16 as u32);
    }

    fn write_long_word_to(self, registers: &mut Registers, value: u32) {
        match (self.0, registers.supervisor_mode) {
            (7, false) => {
                registers.usp = value;
            }
            (7, true) => {
                registers.ssp = value;
            }
            (register, _) => {
                registers.address[register as usize] = value;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpSize {
    Byte,
    Word,
    LongWord,
}

impl OpSize {
    #[cfg(test)]
    const ALL: [Self; 3] = [Self::Byte, Self::Word, Self::LongWord];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizedValue {
    Byte(u8),
    Word(u16),
    LongWord(u32),
}

impl SizedValue {
    fn is_zero(self) -> bool {
        match self {
            Self::Byte(value) => value == 0,
            Self::Word(value) => value == 0,
            Self::LongWord(value) => value == 0,
        }
    }
}

impl SignBit for SizedValue {
    fn sign_bit(self) -> bool {
        match self {
            Self::Byte(value) => value.sign_bit(),
            Self::Word(value) => value.sign_bit(),
            Self::LongWord(value) => value.sign_bit(),
        }
    }
}

trait IncrementStep: Copy {
    fn increment_step_for(register: AddressRegister) -> u32;
}

impl IncrementStep for u8 {
    fn increment_step_for(register: AddressRegister) -> u32 {
        if register.is_stack_pointer() {
            2
        } else {
            1
        }
    }
}

impl IncrementStep for u16 {
    fn increment_step_for(_register: AddressRegister) -> u32 {
        2
    }
}

impl IncrementStep for u32 {
    fn increment_step_for(_register: AddressRegister) -> u32 {
        4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexRegister {
    Data(DataRegister),
    Address(AddressRegister),
}

impl IndexRegister {
    fn read_from(self, registers: &Registers, size: IndexSize) -> u32 {
        let raw_value = match self {
            Self::Data(register) => register.read_from(registers),
            Self::Address(register) => register.read_from(registers),
        };

        match size {
            IndexSize::SignExtendedWord => raw_value as i16 as u32,
            IndexSize::LongWord => raw_value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexSize {
    SignExtendedWord,
    LongWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressingMode {
    DataDirect(DataRegister),
    AddressDirect(AddressRegister),
    AddressIndirect(AddressRegister),
    AddressIndirectPostincrement(AddressRegister),
    AddressIndirectPredecrement(AddressRegister),
    AddressIndirectDisplacement(AddressRegister, i16),
    AddressIndirectIndexed(AddressRegister, IndexRegister, IndexSize, i8),
    PcRelativeDisplacement(u32, i16),
    PcRelativeIndexed(u32, IndexRegister, IndexSize, i8),
    AbsoluteShort(i16),
    AbsoluteLong(u32),
    Immediate(u32),
}

macro_rules! impl_addressing_read_method {
    ($method_name:ident, $t:ty, $bus_read_method:ident) => {
        fn $method_name<B: BusInterface>(self, registers: &mut Registers, bus: &mut B) -> $t {
            match self {
                Self::DataDirect(register) => register.read_from(registers) as $t,
                Self::AddressDirect(register) => register.read_from(registers) as $t,
                Self::AddressIndirect(register) => {
                    let address = register.read_from(registers);
                    bus.$bus_read_method(address)
                }
                Self::AddressIndirectPostincrement(register) => {
                    let increment_step = <$t>::increment_step_for(register);

                    let address = register.read_from(registers);
                    register.write_long_word_to(registers, address.wrapping_add(increment_step));
                    bus.$bus_read_method(address)
                }
                Self::AddressIndirectPredecrement(register) => {
                    let increment_step = <$t>::increment_step_for(register);

                    let address = register.read_from(registers).wrapping_sub(increment_step);
                    register.write_long_word_to(registers, address);
                    bus.$bus_read_method(address)
                }
                Self::AddressIndirectDisplacement(register, displacement) => {
                    let address = register
                        .read_from(registers)
                        .wrapping_add(displacement as u32);
                    bus.$bus_read_method(address)
                }
                Self::AddressIndirectIndexed(
                    a_register,
                    index_register,
                    index_size,
                    displacement,
                ) => {
                    let index = index_register.read_from(registers, index_size);
                    let address = a_register
                        .read_from(registers)
                        .wrapping_add(index)
                        .wrapping_add(displacement as u32);
                    bus.$bus_read_method(address)
                }
                Self::PcRelativeDisplacement(pc, displacement) => {
                    let address = pc.wrapping_add(displacement as u32);
                    bus.$bus_read_method(address)
                }
                Self::PcRelativeIndexed(pc, index_register, index_size, displacement) => {
                    let index = index_register.read_from(registers, index_size);
                    let address = pc.wrapping_add(index).wrapping_add(displacement as u32);
                    bus.$bus_read_method(address)
                }
                Self::AbsoluteShort(address) => bus.$bus_read_method(address as u32),
                Self::AbsoluteLong(address) => bus.$bus_read_method(address),
                Self::Immediate(data) => data as $t,
            }
        }
    };
}

macro_rules! impl_addressing_write_method {
    ($method_name:ident, $t:ty, $bus_write_method:ident, $register_write_method:ident) => {
        fn $method_name<B: BusInterface>(self, registers: &mut Registers, bus: &mut B, value: $t) {
            match self {
                Self::DataDirect(register) => {
                    register.$register_write_method(registers, value);
                }
                Self::AddressDirect(register) => {
                    register.$register_write_method(registers, value);
                }
                Self::AddressIndirect(register) => {
                    let address = register.read_from(registers);
                    bus.$bus_write_method(address, value);
                }
                Self::AddressIndirectPostincrement(register) => {
                    let increment_step = <$t>::increment_step_for(register);

                    let address = register.read_from(registers);
                    register.write_long_word_to(registers, address.wrapping_add(increment_step));
                    bus.$bus_write_method(address, value);
                }
                Self::AddressIndirectPredecrement(register) => {
                    let increment_step = <$t>::increment_step_for(register);

                    let address = register.read_from(registers).wrapping_sub(increment_step);
                    register.write_long_word_to(registers, address);
                    bus.$bus_write_method(address, value);
                }
                Self::AddressIndirectDisplacement(register, displacement) => {
                    let address = register
                        .read_from(registers)
                        .wrapping_add(displacement as u32);
                    bus.$bus_write_method(address, value);
                }
                Self::AddressIndirectIndexed(
                    a_register,
                    index_register,
                    index_size,
                    displacement,
                ) => {
                    let index = index_register.read_from(registers, index_size);
                    let address = a_register
                        .read_from(registers)
                        .wrapping_add(index)
                        .wrapping_add(displacement as u32);
                    bus.$bus_write_method(address, value);
                }
                Self::AbsoluteShort(address) => {
                    bus.$bus_write_method(address as u32, value);
                }
                Self::AbsoluteLong(address) => {
                    bus.$bus_write_method(address, value);
                }
                Self::PcRelativeDisplacement(..)
                | Self::PcRelativeIndexed(..)
                | Self::Immediate(..) => {
                    panic!("writes not supported with addressing mode {self:?}")
                }
            }
        }
    };
}

impl AddressingMode {
    impl_addressing_read_method!(read_byte_from, u8, read_memory);
    impl_addressing_read_method!(read_word_from, u16, read_word);
    impl_addressing_read_method!(read_long_word_from, u32, read_long_word);

    fn read_from<B: BusInterface>(
        self,
        registers: &mut Registers,
        bus: &mut B,
        size: OpSize,
    ) -> SizedValue {
        match size {
            OpSize::Byte => SizedValue::Byte(self.read_byte_from(registers, bus)),
            OpSize::Word => SizedValue::Word(self.read_word_from(registers, bus)),
            OpSize::LongWord => SizedValue::LongWord(self.read_long_word_from(registers, bus)),
        }
    }

    impl_addressing_write_method!(write_byte_to, u8, write_memory, write_byte_to);
    impl_addressing_write_method!(write_word_to, u16, write_word, write_word_to);
    impl_addressing_write_method!(write_long_word_to, u32, write_long_word, write_long_word_to);

    fn write_to<B: BusInterface>(self, registers: &mut Registers, bus: &mut B, value: SizedValue) {
        match value {
            SizedValue::Byte(value) => {
                self.write_byte_to(registers, bus, value);
            }
            SizedValue::Word(value) => {
                self.write_word_to(registers, bus, value);
            }
            SizedValue::LongWord(value) => {
                self.write_long_word_to(registers, bus, value);
            }
        }
    }

    fn is_address_direct(self) -> bool {
        matches!(self, Self::AddressDirect(..))
    }

    fn is_writable(self) -> bool {
        !matches!(
            self,
            Self::PcRelativeDisplacement(..) | Self::PcRelativeIndexed(..) | Self::Immediate(..)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    Move {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
    Illegal,
}

struct InstructionExecutor<'registers, 'bus, B> {
    registers: &'registers mut Registers,
    bus: &'bus mut B,
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn new(registers: &'registers mut Registers, bus: &'bus mut B) -> Self {
        Self { registers, bus }
    }

    fn fetch_operand(&mut self) -> u16 {
        let operand = self.bus.read_word(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(2);
        operand
    }

    fn fetch_addressing_mode(
        &mut self,
        mode: u8,
        register: u8,
        size: OpSize,
    ) -> Option<AddressingMode> {
        match (mode & 0x07, register & 0x07) {
            (0x00, register) => Some(AddressingMode::DataDirect(DataRegister(register))),
            (0x01, register) => Some(AddressingMode::AddressDirect(AddressRegister(register))),
            (0x02, register) => Some(AddressingMode::AddressIndirect(AddressRegister(register))),
            (0x03, register) => Some(AddressingMode::AddressIndirectPostincrement(
                AddressRegister(register),
            )),
            (0x04, register) => Some(AddressingMode::AddressIndirectPredecrement(
                AddressRegister(register),
            )),
            (0x05, register) => {
                let extension = self.fetch_operand();
                log::trace!("Extension word: {extension:04X}");

                let displacement = extension as i16;
                Some(AddressingMode::AddressIndirectDisplacement(
                    AddressRegister(register),
                    displacement,
                ))
            }
            (0x06, register) => {
                let extension = self.fetch_operand();
                log::trace!("Extension word: {extension:04X}");

                let (index_register, index_size) = parse_index(extension);
                let displacement = extension as i8;

                Some(AddressingMode::AddressIndirectIndexed(
                    AddressRegister(register),
                    index_register,
                    index_size,
                    displacement,
                ))
            }
            (0x07, 0x00) => {
                let extension = self.fetch_operand();
                log::trace!("Extension word: {extension:04X}");

                Some(AddressingMode::AbsoluteShort(extension as i16))
            }
            (0x07, 0x01) => {
                let extension_0 = self.fetch_operand();
                let extension_1 = self.fetch_operand();

                log::trace!("Extension words: {extension_0:04X} {extension_1:04X}");

                let address = (u32::from(extension_0) << 16) | u32::from(extension_1);
                Some(AddressingMode::AbsoluteLong(address))
            }
            (0x07, 0x02) => {
                let pc = self.registers.pc;
                let extension = self.fetch_operand();
                log::trace!("Extension word: {extension:04X}");

                let displacement = extension as i16;
                Some(AddressingMode::PcRelativeDisplacement(pc, displacement))
            }
            (0x07, 0x03) => {
                let pc = self.registers.pc;
                let extension = self.fetch_operand();
                log::trace!("Extension word: {extension:04X}");

                let (index_register, index_size) = parse_index(extension);
                let displacement = extension as i8;

                Some(AddressingMode::PcRelativeIndexed(
                    pc,
                    index_register,
                    index_size,
                    displacement,
                ))
            }
            (0x07, 0x04) => {
                let extension_0 = self.fetch_operand();
                log::trace!("Extension word: {extension_0:04X}");

                let immediate_value = match size {
                    OpSize::Byte => (extension_0 as u8).into(),
                    OpSize::Word => extension_0.into(),
                    OpSize::LongWord => {
                        let extension_1 = self.fetch_operand();
                        log::trace!("Second extension word: {extension_1:04X}");

                        (u32::from(extension_0) << 16) | u32::from(extension_1)
                    }
                };

                Some(AddressingMode::Immediate(immediate_value))
            }
            (0x07, 0x05 | 0x06 | 0x07) => {
                // Reserved/illegal addressing mode
                None
            }
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }

    fn fetch_addressing_mode_from_opcode(
        &mut self,
        opcode: u16,
        size: OpSize,
    ) -> Option<AddressingMode> {
        let mode = ((opcode >> 3) & 0x07) as u8;
        let register = (opcode & 0x07) as u8;
        self.fetch_addressing_mode(mode, register, size)
    }

    fn decode_instruction(&mut self) -> Instruction {
        let opcode = self.fetch_operand();
        log::trace!("opcode is {opcode:016b}");
        match opcode & 0xF000 {
            0x1000 | 0x2000 | 0x3000 => {
                // MOVE / MOVEA
                let size = match opcode & 0xF000 {
                    0x1000 => OpSize::Byte,
                    0x3000 => OpSize::Word,
                    0x2000 => OpSize::LongWord,
                    _ => unreachable!("nested match expressions"),
                };

                let Some(source) = self.fetch_addressing_mode_from_opcode(opcode, size)
                else {
                    return Instruction::Illegal;
                };

                let dest_mode = ((opcode >> 6) & 0x07) as u8;
                let dest_register = ((opcode >> 9) & 0x07) as u8;
                let Some(dest) = self.fetch_addressing_mode(dest_mode, dest_register, size)
                else {
                    return Instruction::Illegal;
                };

                if !dest.is_writable() || (dest.is_address_direct() && size == OpSize::Byte) {
                    return Instruction::Illegal;
                }

                Instruction::Move { size, source, dest }
            }
            _ => Instruction::Illegal,
        }
    }

    fn execute_instruction(&mut self, instruction: Instruction) {
        match instruction {
            Instruction::Move { size, source, dest } => self.mov(size, source, dest),
            Instruction::Illegal => panic!("illegal or unimplemented instruction"),
        }
    }

    fn execute(mut self) {
        let instruction = self.decode_instruction();
        log::trace!(
            "Decoded instruction {instruction:?}, PC is now {:08X}",
            self.registers.pc
        );
        self.execute_instruction(instruction);
    }
}

fn parse_index(extension: u16) -> (IndexRegister, IndexSize) {
    let index_register_number = ((extension >> 12) & 0x07) as u8;
    let index_register = if extension.bit(15) {
        IndexRegister::Address(AddressRegister(index_register_number))
    } else {
        IndexRegister::Data(DataRegister(index_register_number))
    };

    let index_size = if extension.bit(11) {
        IndexSize::LongWord
    } else {
        IndexSize::SignExtendedWord
    };
    (index_register, index_size)
}

pub struct M68000 {
    registers: Registers,
}

impl M68000 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: Registers::new(),
        }
    }

    #[must_use]
    pub fn data_registers(&self) -> [u32; 8] {
        self.registers.data
    }

    pub fn set_data_registers(&mut self, registers: [u32; 8]) {
        self.registers.data = registers;
    }

    #[must_use]
    pub fn address_registers(&self) -> [u32; 7] {
        self.registers.address
    }

    #[must_use]
    pub fn user_stack_pointer(&self) -> u32 {
        self.registers.usp
    }

    #[must_use]
    pub fn supervisor_stack_pointer(&self) -> u32 {
        self.registers.ssp
    }

    pub fn set_address_registers(&mut self, registers: [u32; 7], usp: u32, ssp: u32) {
        self.registers.address = registers;
        self.registers.usp = usp;
        self.registers.ssp = ssp;
    }

    #[must_use]
    pub fn status_register(&self) -> u16 {
        self.registers.status_register()
    }

    pub fn set_status_register(&mut self, status_register: u16) {
        self.registers.set_status_register(status_register);
    }

    #[must_use]
    pub fn pc(&self) -> u32 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, pc: u32) {
        self.registers.pc = pc;
    }

    pub fn execute_instruction<B: BusInterface>(&mut self, bus: &mut B) {
        InstructionExecutor::new(&mut self.registers, bus).execute();
    }
}

impl Default for M68000 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::InMemoryBus;

    #[test]
    fn decode_mov() {
        // MOVE.w A3, D7
        let opcode = 0b0011_111_000_001_011;

        let mut registers = Registers::new();
        let mut bus = InMemoryBus::new();

        registers.pc = 0x1234;
        bus.write_word(registers.pc, opcode);

        let instruction = InstructionExecutor::new(&mut registers, &mut bus).decode_instruction();
        assert_eq!(
            instruction,
            Instruction::Move {
                size: OpSize::Word,
                source: AddressingMode::AddressDirect(AddressRegister(3)),
                dest: AddressingMode::DataDirect(DataRegister(7)),
            }
        );
        assert_eq!(registers.pc, 0x1234 + 2);

        // MOVE.b #$12, ($3456, A4)
        let opcode = 0b0001_100_101_111_100;
        registers.pc = 0x1234;
        bus.write_word(registers.pc, opcode);
        bus.write_word(registers.pc.wrapping_add(2), 0xFF12);
        bus.write_word(registers.pc.wrapping_add(4), 0x3456);

        let instruction = InstructionExecutor::new(&mut registers, &mut bus).decode_instruction();
        assert_eq!(
            instruction,
            Instruction::Move {
                size: OpSize::Byte,
                source: AddressingMode::Immediate(0x12),
                dest: AddressingMode::AddressIndirectDisplacement(AddressRegister(4), 0x3456),
            }
        );
        assert_eq!(registers.pc, 0x1234 + 6);
    }
}
