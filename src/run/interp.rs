use super::disp::{write_to_display, DisplayBuffer};
use super::input::Key;
use super::prog::{Program, ProgramKind, PROGRAM_MEMORY_SIZE, PROGRAM_STARTING_ADDRESS};

use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

use std::fmt::Display;

pub const VFLAG: usize = 15;

const FONT_STARTING_ADDRESS: u16 = 0x50; // store font in memory from 0x50 to 0x9F inclusive
const FONT_CHAR_DATA_SIZE: u8 = 5;
const FONT: [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80, // F
];

// Takes a 16 bit number (instruction size) and decomposes it into its parts
#[derive(Clone, Copy, Debug)]
pub struct InstructionParameters {
    pub bits: u16,
    pub op: u8,
    pub x: u8,
    pub y: u8,
    pub n: u8,
    pub nn: u8,
    pub nnn: u16,
}

impl From<u16> for InstructionParameters {
    fn from(bits: u16) -> Self {
        InstructionParameters {
            bits,
            op: ((bits & 0xF000) >> 4 * 3) as u8,
            x: ((bits & 0x0F00) >> 4 * 2) as u8,
            y: ((bits & 0x00F0) >> 4 * 1) as u8,
            n: ((bits & 0x000F) >> 4 * 0) as u8,
            nn: ((bits & 0x00FF) >> 4 * 0) as u8,
            nnn: ((bits & 0x0FFF) >> 4 * 0) as u16,
        }
    }
}

impl From<[u8; 2]> for InstructionParameters {
    fn from(bytes: [u8; 2]) -> Self {
        InstructionParameters::from((bytes[0] as u16) << 8 | bytes[1] as u16)
    }
}

impl Display for InstructionParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:#06X} (op = {:#X?}, x = {:?}, y = {:?}, n = {:?}, nn = {:?}, nnn = {:?})",
            self.bits, self.op, self.x, self.y, self.n, self.nn, self.nnn
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    ClearScreen,
    Jump(u16),
    JumpWithOffset(u16, u8),
    CallSubroutine(u16),
    SubroutineReturn,
    SkipIfEqualsConstant(u8, u8),
    SkipIfNotEqualsConstant(u8, u8),
    SkipIfEquals(u8, u8),
    SkipIfNotEquals(u8, u8),
    SkipIfKeyDown(u8),
    SkipIfKeyNotDown(u8),
    GetKey(u8),
    SetConstant(u8, u8),
    AddConstant(u8, u8),
    Set(u8, u8),
    Or(u8, u8),
    And(u8, u8),
    Xor(u8, u8),
    Add(u8, u8),
    Sub(u8, u8, bool),
    Shift(u8, u8, bool),
    GetDelayTimer(u8),
    SetDelayTimer(u8),
    SetSoundTimer(u8),
    SetIndex(u16),
    SetIndexToHexChar(u8),
    AddToIndex(u8),
    Load(u8),
    Store(u8),
    StoreDecimal(u8),
    GenerateRandom(u8, u8),
    Display(u8, u8, u8),
}

impl TryFrom<InstructionParameters> for Instruction {
    type Error = String;
    fn try_from(params: InstructionParameters) -> Result<Self, Self::Error> {
        let (op, x, y, n, nn, nnn) = (
            params.op, params.x, params.y, params.n, params.nn, params.nnn,
        );

        match op {
            0x0 => match nnn {
                0x0E0 => Ok(Self::ClearScreen),
                0x0EE => Ok(Self::SubroutineReturn),
                _ => Err(format!("unable to decode instruction {}", params)),
            },
            0x1 => Ok(Self::Jump(nnn)),
            0x2 => Ok(Self::CallSubroutine(nnn)),
            0x3 => Ok(Self::SkipIfEqualsConstant(x, nn)),
            0x4 => Ok(Self::SkipIfNotEqualsConstant(x, nn)),
            0x5 => Ok(Self::SkipIfEquals(x, y)),
            0x6 => Ok(Self::SetConstant(x, nn)),
            0x7 => Ok(Self::AddConstant(x, nn)),
            0x8 => match n {
                0x0 => Ok(Self::Set(x, y)),
                0x1 => Ok(Self::Or(x, y)),
                0x2 => Ok(Self::And(x, y)),
                0x3 => Ok(Self::Xor(x, y)),
                0x4 => Ok(Self::Add(x, y)),
                0x5 => Ok(Self::Sub(x, y, true)),
                0x6 => Ok(Self::Shift(x, y, true)),
                0x7 => Ok(Self::Sub(x, y, false)),
                0xE => Ok(Self::Shift(x, y, false)),
                _ => Err(format!("unable to decode instruction {}", params)),
            },
            0x9 => match n {
                0x0 => Ok(Self::SkipIfNotEquals(x, y)),
                _ => Err(format!("unable to decode instruction {}", params)),
            },
            0xA => Ok(Self::SetIndex(nnn)),
            0xB => Ok(Self::JumpWithOffset(nnn, x)),
            0xC => Ok(Self::GenerateRandom(x, nn)),
            0xD => Ok(Self::Display(x, y, n)),
            0xE => match nn {
                0x9E => Ok(Self::SkipIfKeyDown(x)),
                0xA1 => Ok(Self::SkipIfKeyNotDown(x)),
                _ => Err(format!("unable to decode instruction {}", params)),
            },
            0xF => match nn {
                0x07 => Ok(Self::GetDelayTimer(x)),
                0x15 => Ok(Self::SetDelayTimer(x)),
                0x18 => Ok(Self::SetSoundTimer(x)),
                0x1E => Ok(Self::AddToIndex(x)),
                0x0A => Ok(Self::GetKey(x)),
                0x29 => Ok(Self::SetIndexToHexChar(x)),
                0x33 => Ok(Self::StoreDecimal(x)),
                0x55 => Ok(Self::Store(x)),
                0x65 => Ok(Self::Load(x)),
                _ => Err(format!("unable to decode instruction {}", params)),
            },
            _ => Err(format!("unable to decode instruction {}", params)),
        }
    }
}

// State the interpreter pulls from IO is stored here
#[derive(Debug, Default)]
pub struct InterpreterInput {
    pub delay_timer: u8,

    pub down_keys: u16,
    pub just_pressed_key: Option<u8>,
    pub just_released_key: Option<u8>,
}

// Response body so IO know how to proceed
#[derive(Debug)]
pub struct InterpreterOutput {
    pub display: DisplayBuffer,
    pub awaiting_input: bool,

    pub request: Option<InterpreterRequest>,
}

// Interpreter IO Request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpreterRequest {
    Display,
    SetDelayTimer(u8),
    SetSoundTimer(u8),
}

pub type InterpreterMemory = [u8; PROGRAM_MEMORY_SIZE as usize];

#[derive(Debug, Eq, PartialEq)]
pub enum PartialInterpreterStatePayload {
    Rng(StdRng),
    Display(DisplayBuffer),
}

#[derive(Debug, PartialEq, Eq)]
pub struct InterpreterHistoryFragment {
    pub instruction: Option<Instruction>,
    pub pc: u16,
    pub return_address: u16,
    pub index: u16,
    pub index_memory: [u8; 16],
    pub registers: [u8; 16],
    pub payload: Option<Box<PartialInterpreterStatePayload>>,
}

impl From<&Interpreter> for InterpreterHistoryFragment {
    fn from(interp: &Interpreter) -> Self {
        let mut index_memory = [0; 16];
        let index = interp.index as usize;
        if index < interp.memory.len() {
            let n = (index + 16).min(interp.memory.len()) - index;
            index_memory[..n].copy_from_slice(&interp.memory[index..index + n]);
        }

        let instruction = interp.fetch().and_then(Instruction::try_from).ok();

        InterpreterHistoryFragment {
            payload: match instruction.as_ref() {
                Some(&Instruction::GenerateRandom(_, _)) => Some(Box::new(
                    PartialInterpreterStatePayload::Rng(interp.rng.clone()),
                )),
                Some(&Instruction::ClearScreen) => Some(Box::new(
                    PartialInterpreterStatePayload::Display(interp.output.display.clone()),
                )),
                _ => None,
            },

            pc: interp.pc,
            instruction,
            return_address: interp.stack.last().cloned().unwrap_or_default(),
            index: interp.index,
            index_memory,
            registers: interp.registers,
        }
    }
}

impl InterpreterHistoryFragment {
    pub(super) fn are_get_key_forms(&self, rhs: &Self) -> bool {
        if let Some(&Instruction::GetKey(_)) = self.instruction.as_ref() {
            self == rhs
        } else {
            false
        }
    }

    pub(super) fn does_modify_display(&self) -> bool {
        match self.instruction.as_ref() {
            Some(&Instruction::ClearScreen | &Instruction::Display(_, _, _)) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct Interpreter {
    pub memory: InterpreterMemory,
    pub pc: u16,
    pub index: u16,
    pub stack: Vec<u16>,
    pub registers: [u8; 16],
    pub input: InterpreterInput,
    pub output: InterpreterOutput,
    pub program: Program,
    pub rng: StdRng,
}

impl<'a> From<Program> for Interpreter {
    fn from(program: Program) -> Self {
        Interpreter {
            memory: Self::alloc(&program),
            program,
            pc: PROGRAM_STARTING_ADDRESS,
            index: 0,
            stack: Vec::with_capacity(16),
            registers: [0; 16],
            rng: StdRng::from_entropy(),
            input: Default::default(),
            output: InterpreterOutput {
                display: Default::default(),
                awaiting_input: false,
                request: None,
            },
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Interpreter::from(Program::default())
    }
}

impl Interpreter {
    pub fn instruction_parameters(
        binary: &[u8],
    ) -> impl Iterator<Item = InstructionParameters> + '_ {
        binary
            .windows(2)
            .map(|slice| InstructionParameters::from([slice[0], slice[1]]))
    }

    pub fn alloc(program: &Program) -> InterpreterMemory {
        let mut memory = [0; PROGRAM_MEMORY_SIZE as usize];

        memory[FONT_STARTING_ADDRESS as usize..FONT_STARTING_ADDRESS as usize + FONT.len()]
            .copy_from_slice(&FONT);

        memory[PROGRAM_STARTING_ADDRESS as usize
            ..PROGRAM_STARTING_ADDRESS as usize + program.data.len()]
            .copy_from_slice(&program.data);

        memory
    }

    pub fn checked_addr_add(&self, addr: u16, amt: u16) -> Option<u16> {
        let (result_addr, result_overflow) = addr.overflowing_add(amt);
        if (addr as usize) < self.memory.len()
            && (result_addr as usize) < self.memory.len()
            && !result_overflow
        {
            Some(result_addr)
        } else {
            None
        }
    }

    pub fn undo(&mut self, prior_state: &InterpreterHistoryFragment) {
        self.pc = prior_state.pc;
        self.index = prior_state.index;
        self.registers = prior_state.registers;

        let index = self.index as usize;
        let n = (index + 16).min(self.memory.len()) - index;

        self.memory[index..index + n].copy_from_slice(&prior_state.index_memory);
        let Some(inst) = prior_state.instruction.as_ref() else {
            unreachable!("Cannot undo to a state without an instruction")
        };
        log::trace!("Undoing instruction: {:?}", inst);

        match inst {
            Instruction::CallSubroutine(_) => {
                self.stack.pop();
            }
            Instruction::SubroutineReturn => {
                self.stack.push(prior_state.return_address);
            }
            Instruction::Display(vx, vy, height) => {
                self.exec_display_instruction(*vx, *vy, *height);
                self.registers[VFLAG] = prior_state.registers[VFLAG];
            }
            Instruction::ClearScreen => {
                let Some(PartialInterpreterStatePayload::Display(display)) = prior_state.payload.as_deref() else {
                    unreachable!("clear screen instruction should have display payload");
                };

                self.output.display = *display;
            }
            Instruction::GenerateRandom(_, _) => {
                let Some(PartialInterpreterStatePayload::Rng(rng)) = prior_state.payload.as_deref() else {
                    unreachable!("generate random instruction should have rng payload");
                };

                self.rng = rng.clone();
            }
            _ => (),
        }
    }

    pub fn input_mut(&mut self) -> &mut InterpreterInput {
        &mut self.input
    }

    // interpret the next instruction
    pub fn step(&mut self) -> Result<&InterpreterOutput, String> {
        // clear output request
        self.output.request = None;

        // fetch + decode
        match Instruction::try_from(self.fetch()?) {
            Ok(inst) => {
                log::trace!("Instruction {:#05X?} {:?} ", self.pc, inst);
                self.pc += 2;

                // execute instruction
                if let Err(e) = self.exec(inst) {
                    self.pc -= 2; // revert program counter
                    Err(e)
                } else {
                    Ok(&self.output)
                }
            }
            Err(e) => Err(format!("Decode at {:#05X?} failed: {}", self.pc, e)),
        }
    }

    pub fn pick_key<'a, 'b, T: TryInto<Key>>(
        &'a self,
        key_down: &'b Option<T>,
        key_up: &'b Option<T>,
    ) -> &'b Option<T> {
        match self.program.kind {
            ProgramKind::COSMACVIP => key_up,
            _ => key_down,
        }
    }

    pub fn fetch(&self) -> Result<InstructionParameters, String> {
        if (self.pc as usize) < self.memory.len() - 1 {
            Ok(InstructionParameters::from([
                self.memory[self.pc as usize],
                self.memory[self.pc as usize + 1],
            ]))
        } else {
            Err(format!(
                "Fetch failed: Program counter address is out of bounds ({:#05X?})",
                self.pc
            ))
        }
    }

    fn exec(&mut self, inst: Instruction) -> Result<(), String> {
        match inst {
            Instruction::ClearScreen => {
                self.output.display.fill(0);
                self.output.request = Some(InterpreterRequest::Display);
            }

            Instruction::Jump(pc) => self.pc = pc,

            Instruction::JumpWithOffset(address, vx) => {
                let offset = if self.program.kind == ProgramKind::CHIP48 {
                    self.registers[vx as usize] as u16
                } else {
                    self.registers[0] as u16
                };

                if (offset as usize) < self.memory.len().saturating_sub(address as usize) {
                    self.pc = address + offset;
                } else {
                    return Err(format!(
                        "Jump with offset failed: adresss {:#05X?} with offset {:#04X?} ({}) is out of bounds",
                        address, offset, offset
                    ));
                }
            }

            Instruction::CallSubroutine(pc) => {
                self.stack.push(self.pc);
                self.pc = pc;
            }

            Instruction::SubroutineReturn => {
                self.pc = self
                    .stack
                    .pop()
                    .expect("Could not return from subroutine because stack is empty")
            }

            Instruction::SkipIfEqualsConstant(vx, value) => {
                if self.registers[vx as usize] == value {
                    self.pc += 2
                }
            }

            Instruction::SkipIfNotEqualsConstant(vx, value) => {
                if self.registers[vx as usize] != value {
                    self.pc += 2
                }
            }

            Instruction::SkipIfEquals(vx, vy) => {
                if self.registers[vx as usize] == self.registers[vy as usize] {
                    self.pc += 2
                }
            }

            Instruction::SkipIfNotEquals(vx, vy) => {
                if self.registers[vx as usize] != self.registers[vy as usize] {
                    self.pc += 2
                }
            }

            Instruction::SkipIfKeyDown(vx) => {
                if self.input.down_keys >> self.registers[vx as usize] & 1 == 1 {
                    self.pc += 2
                }
            }

            Instruction::SkipIfKeyNotDown(vx) => {
                if self.input.down_keys >> self.registers[vx as usize] & 1 == 0 {
                    self.pc += 2
                }
            }

            Instruction::GetKey(vx) => {
                if let Some(key_code) =
                    self.pick_key(&self.input.just_pressed_key, &self.input.just_released_key)
                {
                    self.registers[vx as usize] = *key_code;
                } else {
                    self.pc -= 2;
                }
            }

            Instruction::SetConstant(vx, value) => self.registers[vx as usize] = value,

            Instruction::AddConstant(vx, change) => {
                self.registers[vx as usize] = self.registers[vx as usize].overflowing_add(change).0
            }

            Instruction::Set(vx, vy) => self.registers[vx as usize] = self.registers[vy as usize],

            Instruction::Or(vx, vy) => self.registers[vx as usize] |= self.registers[vy as usize],

            Instruction::And(vx, vy) => self.registers[vx as usize] &= self.registers[vy as usize],

            Instruction::Xor(vx, vy) => self.registers[vx as usize] ^= self.registers[vy as usize],

            Instruction::Add(vx, vy) => {
                let (value, overflowed) =
                    self.registers[vx as usize].overflowing_add(self.registers[vy as usize]);
                self.registers[vx as usize] = value;
                self.registers[VFLAG] = overflowed as u8;
            }

            Instruction::Sub(vx, vy, vx_minus_vy) => {
                let (value, overflowed) = if vx_minus_vy {
                    self.registers[vx as usize].overflowing_sub(self.registers[vy as usize])
                } else {
                    self.registers[vy as usize].overflowing_sub(self.registers[vx as usize])
                };

                self.registers[vx as usize] = value;
                self.registers[VFLAG] = !overflowed as u8; // vf is 0 on overflow instead of 1 like add
            }

            Instruction::Shift(vx, vy, right) => {
                let bits = match self.program.kind {
                    ProgramKind::COSMACVIP => self.registers[vy as usize],
                    _ => self.registers[vx as usize],
                };

                if right {
                    self.registers[VFLAG] = bits & 1;
                    self.registers[vx as usize] = bits >> 1;
                } else {
                    self.registers[VFLAG] = bits.reverse_bits() & 1;
                    self.registers[vx as usize] = bits << 1;
                }
            }

            Instruction::GetDelayTimer(vx) => self.registers[vx as usize] = self.input.delay_timer,

            Instruction::SetDelayTimer(vx) => {
                self.output.request = Some(InterpreterRequest::SetDelayTimer(
                    self.registers[vx as usize],
                ))
            }

            Instruction::SetSoundTimer(vx) => {
                self.output.request = Some(InterpreterRequest::SetSoundTimer(
                    self.registers[vx as usize],
                ))
            }

            Instruction::SetIndex(index) => self.index = index,

            Instruction::SetIndexToHexChar(vx) => {
                self.index = FONT_STARTING_ADDRESS
                    + (FONT_CHAR_DATA_SIZE as u16 * self.registers[vx as usize] as u16)
            }

            // TODO: maybe make optional behavior (register vf set on overflow) configurable
            Instruction::AddToIndex(vx) => {
                self.index = self
                    .index
                    .overflowing_add(self.registers[vx as usize] as u16)
                    .0;
            }

            Instruction::Load(vx) => {
                if let Some(addr) = self.checked_addr_add(self.index, vx as u16) {
                    self.registers[..=vx as usize]
                        .copy_from_slice(&self.memory[self.index as usize..=addr as usize]);
                    if self.program.kind == ProgramKind::COSMACVIP {
                        self.index = addr.overflowing_add(1).0;
                    }
                } else {
                    return Err(format!(
                        "Failed to load bytes from memory: out of bounds read ({} byte{} from i = {:#05X?})", 
                        vx + 1, 
                        if vx > 0 { "s" } else { "" }, 
                        self.index
                    ));
                }
            }

            Instruction::Store(vx) => {
                if let Some(addr) = self.checked_addr_add(self.index, vx as u16) {
                    self.memory[self.index as usize..=addr as usize]
                        .copy_from_slice(&self.registers[..=vx as usize]);
                    if self.program.kind == ProgramKind::COSMACVIP {
                        self.index = addr.overflowing_add(1).0;
                    }
                } else {
                    return Err(format!(
                        "Failed to write bytes to memory: out of bounds write ({} byte{} from i = {:#05X?})", 
                        vx + 1, 
                        if vx > 0 { "s" } else { "" }, 
                        self.index
                    ));
                }
            }

            Instruction::StoreDecimal(vx) => {
                if let Some(addr) = self.checked_addr_add(self.index, 2) {
                    let number = self.registers[vx as usize];
                    for (i, val) in self.memory[self.index as usize..=addr as usize]
                        .iter_mut()
                        .rev()
                        .enumerate()
                    {
                        *val = number / 10u8.pow(i as u32) % 10;
                    }
                } else {
                    return Err(format!("Failed to write decimal to memory: out of bounds write (3 bytes from i = {:#05X?})", self.index));
                }
            }

            Instruction::GenerateRandom(vx, bound) => {
                self.registers[vx as usize] = (self.rng.next_u32() & bound as u32) as u8;
            }

            Instruction::Display(vx, vy, height) => {
                if self
                    .checked_addr_add(self.index, height.saturating_sub(1) as u16)
                    .is_none()
                {
                    return Err(format!(
                        "Failed to display: sprite out of bounds read ({} byte{} from i = {:#05X?})", 
                        height, 
                        if height > 1 { "s" } else { "" }, 
                        self.index
                    ));
                }

                self.exec_display_instruction(vx, vy, height);

                self.output.request = Some(InterpreterRequest::Display);
            }
        }
        Ok(())
    }

    fn exec_display_instruction(&mut self, vx: u8, vy: u8, height: u8) {
        self.registers[VFLAG] = write_to_display(
            &mut self.output.display,
            &self.memory[self.index as usize..],
            self.registers[vx as usize],
            self.registers[vy as usize],
            height,
        ) as u8;
    }
}