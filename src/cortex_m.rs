//! Generic Cortex-M execution used for portable firmware probes.
//!
//! This runtime intentionally models only linear Flash/RAM, the Cortex system
//! peripherals provided by zmu, semihosting, and a tiny completion mailbox. It
//! does not invent a vendor MCU or silently reuse PHY6252 peripherals.

use crate::board::{require_generic_cortex_m4, BoardKind};
use crate::hex::HexImage;
use crate::soc::{self, SocKind};
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::{Fault, FaultTrapMode};
use zmu_cortex_m::core::register::BaseReg;
use zmu_cortex_m::core::reset::Reset;
use zmu_cortex_m::executor::Executor;
use zmu_cortex_m::memory::map::MemoryMapConfig;
use zmu_cortex_m::memory::ram::RAM;
use zmu_cortex_m::semihosting::{SemihostingCommand, SemihostingResponse, SysExceptionReason};
use zmu_cortex_m::Processor;

pub const FLASH_ORIGIN: u32 = 0x0800_0000;
pub const FLASH_BYTES: usize = 2 * 1024 * 1024;
pub const RAM_ORIGIN: u32 = 0x2000_0000;
pub const RAM_BYTES: usize = 512 * 1024;
pub const STACK_SCAN_BYTES: usize = 256 * 1024;
pub const PROBE_MAILBOX_BASE: u32 = 0x4000_F000;
pub const PROBE_MAILBOX_MAGIC: u32 = 0x4857_5052;
const PROBE_MAILBOX_BYTES: u32 = 8;
const STACK_PATTERN: u8 = 0xA5;
const CORTEX_M4_CPUID: u32 = 0x410F_C241;

pub struct ProbeOpts {
    pub hex: PathBuf,
    pub board: BoardKind,
    pub strict: bool,
    pub max_insns: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProbeSignal {
    completed: bool,
    status: u32,
}

struct ProbeMailbox {
    signal: Rc<RefCell<ProbeSignal>>,
}

impl ProbeMailbox {
    fn new(signal: Rc<RefCell<ProbeSignal>>) -> Self {
        Self { signal }
    }
}

impl Bus for ProbeMailbox {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        match addr {
            PROBE_MAILBOX_BASE => Ok(PROBE_MAILBOX_MAGIC),
            value if value == PROBE_MAILBOX_BASE + 4 => Ok(self.signal.borrow().status),
            _ => Err(Fault::DAccViol),
        }
    }

    fn read16(&self, _addr: u32) -> Result<u16, Fault> {
        Err(Fault::DAccViol)
    }

    fn read8(&self, _addr: u32) -> Result<u8, Fault> {
        Err(Fault::DAccViol)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        let mut signal = self.signal.borrow_mut();
        match addr {
            PROBE_MAILBOX_BASE if value == PROBE_MAILBOX_MAGIC => {
                signal.completed = true;
                signal.status = 0;
                Ok(())
            }
            value_addr if value_addr == PROBE_MAILBOX_BASE + 4 => {
                signal.completed = true;
                signal.status = value;
                Ok(())
            }
            _ => Err(Fault::DAccViol),
        }
    }

    fn write16(&mut self, _addr: u32, _value: u16) -> Result<(), Fault> {
        Err(Fault::DAccViol)
    }

    fn write8(&mut self, _addr: u32, _value: u8) -> Result<(), Fault> {
        Err(Fault::DAccViol)
    }

    fn in_range(&self, addr: u32) -> bool {
        (PROBE_MAILBOX_BASE..PROBE_MAILBOX_BASE + PROBE_MAILBOX_BYTES).contains(&addr)
    }
}

#[derive(Debug, Default)]
struct SemihostState {
    completed: Option<u32>,
    output: Vec<u8>,
}

#[derive(Clone, Debug)]
enum StopReason {
    Mailbox(u32),
    Semihost(u32),
    Fault(String),
    Halt,
    InstructionLimit,
}

impl StopReason {
    const fn status(&self) -> &'static str {
        match self {
            Self::Mailbox(0) | Self::Semihost(0) => "ok",
            Self::Mailbox(_) | Self::Semihost(_) => "failed",
            Self::Fault(_) => "fault",
            Self::Halt => "halt",
            Self::InstructionLimit => "timeout",
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::Mailbox(status) => format!("mailbox-{status}"),
            Self::Semihost(status) => format!("semihost-{status}"),
            Self::Fault(context) => context.replace(' ', "_"),
            Self::Halt => "processor-halted-without-result".into(),
            Self::InstructionLimit => "instruction-limit".into(),
        }
    }

    const fn success(&self) -> bool {
        matches!(self, Self::Mailbox(0) | Self::Semihost(0))
    }
}

pub fn run(opts: ProbeOpts) -> Result<ExitCode, String> {
    let board = require_generic_cortex_m4(opts.board)?;
    let soc_profile = soc::require_implemented(SocKind::GenericCortexM4)?;
    let image = HexImage::load(&opts.hex).map_err(|error| {
        format!(
            "cannot load Cortex-M firmware {}: {error}",
            opts.hex.display()
        )
    })?;

    let mut flash = vec![0xFF; FLASH_BYTES];
    image.fill(FLASH_ORIGIN, &mut flash);
    validate_vectors(&flash)?;

    let signal = Rc::new(RefCell::new(ProbeSignal::default()));
    let semihost = Rc::new(RefCell::new(SemihostState::default()));
    let semihost_capture = Rc::clone(&semihost);

    let mut processor = Processor::new();
    processor.cpuid = CORTEX_M4_CPUID;
    processor.vtor = FLASH_ORIGIN;
    processor.sram = RAM::new_with_fill(RAM_ORIGIN, RAM_BYTES, STACK_PATTERN);
    fill_ram_image(&image, &mut processor)?;
    processor.flash_memory(FLASH_BYTES, &flash);
    processor.memory_map(Some(MemoryMapConfig::new(FLASH_ORIGIN, 0, FLASH_BYTES)));
    processor.device(Some(Box::new(ProbeMailbox::new(Rc::clone(&signal)))));
    processor.semihost(Some(Box::new(move |command| {
        handle_semihost(command, &semihost_capture)
    })));
    processor.fault_trap_mode(FaultTrapMode::hardfault());
    processor
        .reset()
        .map_err(|fault| format!("Cortex-M reset failed: {fault:?}"))?;
    processor.running = true;

    let initial_sp = processor.msp;
    validate_stack_pointer(initial_sp)?;
    let stop = execute(&mut processor, &signal, &semihost, opts.max_insns);
    let (stack_used, stack_saturated) = stack_high_water(&processor, initial_sp)?;

    let guest_output = semihost.borrow().output.clone();
    for line in String::from_utf8_lossy(&guest_output).lines() {
        println!("GUEST {line}");
    }

    println!(
        "PROBE status={} board={} soc={} profile={} instructions={} cycles={} stack_used={} stack_window={} stack_saturated={} initial_sp=0x{initial_sp:08x} pc=0x{:08x} exit_code={} strict={} reason={}",
        stop.status(),
        board.id,
        soc_profile.id,
        soc_profile.cpu.label(),
        processor.instruction_count,
        processor.cycle_count,
        stack_used,
        STACK_SCAN_BYTES,
        stack_saturated,
        processor.get_pc(),
        processor.exit_code,
        opts.strict,
        stop.detail(),
    );

    Ok(ExitCode::from(if stop.success() { 0 } else { 2 }))
}

fn validate_vectors(flash: &[u8]) -> Result<(), String> {
    let vectors = flash
        .get(..8)
        .ok_or_else(|| "Cortex-M image has no vector table".to_string())?;
    let sp = u32::from_le_bytes(vectors[..4].try_into().map_err(|_| "invalid SP vector")?);
    let reset = u32::from_le_bytes(
        vectors[4..8]
            .try_into()
            .map_err(|_| "invalid reset vector")?,
    );
    validate_stack_pointer(sp)?;
    let reset_address = reset & !1;
    let flash_end = FLASH_ORIGIN + u32::try_from(FLASH_BYTES).map_err(|_| "Flash too large")?;
    if reset & 1 == 0 || !(FLASH_ORIGIN..flash_end).contains(&reset_address) {
        return Err(format!(
            "invalid Cortex-M reset vector {reset:#010x}; expected Thumb code in {FLASH_ORIGIN:#010x}..{flash_end:#010x}"
        ));
    }
    Ok(())
}

fn validate_stack_pointer(sp: u32) -> Result<(), String> {
    let ram_end = RAM_ORIGIN + u32::try_from(RAM_BYTES).map_err(|_| "RAM too large")?;
    if sp & 3 != 0 || sp <= RAM_ORIGIN || sp > ram_end {
        return Err(format!(
            "invalid initial MSP {sp:#010x}; expected aligned address in {RAM_ORIGIN:#010x}..={ram_end:#010x}"
        ));
    }
    Ok(())
}

fn fill_ram_image(image: &HexImage, processor: &mut Processor) -> Result<(), String> {
    let ram_end = RAM_ORIGIN + u32::try_from(RAM_BYTES).map_err(|_| "RAM too large")?;
    for (address, value) in &image.bytes {
        if (RAM_ORIGIN..ram_end).contains(address) {
            processor
                .sram
                .write8(*address, *value)
                .map_err(|fault| format!("cannot load RAM byte at {address:#010x}: {fault:?}"))?;
        }
    }
    Ok(())
}

fn execute(
    processor: &mut Processor,
    signal: &Rc<RefCell<ProbeSignal>>,
    semihost: &Rc<RefCell<SemihostState>>,
    max_insns: u64,
) -> StopReason {
    while processor.running && processor.instruction_count < max_insns {
        if processor.sleeping {
            processor.step_sleep();
        } else {
            processor.step();
        }

        if let Some(context) = processor.take_pending_fault_trap() {
            return StopReason::Fault(format!("{context:?}"));
        }
        let mailbox = *signal.borrow();
        if mailbox.completed {
            return StopReason::Mailbox(mailbox.status);
        }
        if let Some(status) = semihost.borrow().completed {
            return StopReason::Semihost(status);
        }
    }

    if let Some(status) = semihost.borrow().completed {
        StopReason::Semihost(status)
    } else if !processor.running {
        StopReason::Halt
    } else {
        StopReason::InstructionLimit
    }
}

fn stack_high_water(processor: &Processor, initial_sp: u32) -> Result<(usize, bool), String> {
    let available = usize::try_from(initial_sp - RAM_ORIGIN).map_err(|_| "stack range")?;
    let window = STACK_SCAN_BYTES.min(available);
    let lower = initial_sp - u32::try_from(window).map_err(|_| "stack window")?;
    let mut first_touched = None;

    for address in lower..initial_sp {
        let value = processor
            .sram
            .read8(address)
            .map_err(|fault| format!("cannot scan stack at {address:#010x}: {fault:?}"))?;
        if value != STACK_PATTERN {
            first_touched = Some(address);
            break;
        }
    }

    let Some(first_touched) = first_touched else {
        return Ok((0, false));
    };
    Ok((
        usize::try_from(initial_sp - first_touched).map_err(|_| "stack usage")?,
        first_touched == lower,
    ))
}

fn handle_semihost(
    command: &SemihostingCommand,
    state: &Rc<RefCell<SemihostState>>,
) -> SemihostingResponse {
    match command {
        SemihostingCommand::SysOpen { .. } => SemihostingResponse::SysOpen { result: Err(-1) },
        SemihostingCommand::SysClose { .. } => SemihostingResponse::SysClose { success: false },
        SemihostingCommand::SysSeek { .. } => SemihostingResponse::SysSeek { success: false },
        SemihostingCommand::SysFlen { .. } => SemihostingResponse::SysFlen { result: Err(-1) },
        SemihostingCommand::SysIstty { .. } => SemihostingResponse::SysIstty { result: Ok(1) },
        SemihostingCommand::SysWriteC { data } => {
            state.borrow_mut().output.push(*data);
            SemihostingResponse::SysWrite { result: Ok(1) }
        }
        SemihostingCommand::SysWrite { data, .. } => {
            state.borrow_mut().output.extend_from_slice(data);
            SemihostingResponse::SysWrite {
                result: Ok(u32::try_from(data.len()).unwrap_or(u32::MAX)),
            }
        }
        SemihostingCommand::SysRead { .. } => SemihostingResponse::SysRead { result: Err(-1) },
        SemihostingCommand::SysException { reason } => {
            let status = if *reason == SysExceptionReason::ADPStoppedApplicationExit {
                0
            } else {
                1
            };
            state.borrow_mut().completed = Some(status);
            SemihostingResponse::SysException {
                success: true,
                stop: true,
            }
        }
        SemihostingCommand::SysExitExtended { reason, subcode } => {
            let status = if *reason == SysExceptionReason::ADPStoppedApplicationExit {
                *subcode
            } else {
                1
            };
            state.borrow_mut().completed = Some(status);
            SemihostingResponse::SysExitExtended {
                success: true,
                stop: true,
                exit_code: Some(status),
            }
        }
        SemihostingCommand::SysClock => SemihostingResponse::SysClock { result: Ok(0) },
        SemihostingCommand::SysErrno => SemihostingResponse::SysErrno { result: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_marks_success() {
        let signal = Rc::new(RefCell::new(ProbeSignal::default()));
        let mut mailbox = ProbeMailbox::new(Rc::clone(&signal));
        mailbox
            .write32(PROBE_MAILBOX_BASE, PROBE_MAILBOX_MAGIC)
            .expect("mailbox write");
        assert_eq!(
            *signal.borrow(),
            ProbeSignal {
                completed: true,
                status: 0
            }
        );
    }

    #[test]
    fn vectors_require_real_flash_and_ram_addresses() {
        let mut flash = vec![0xFF; 32];
        flash[..4].copy_from_slice(&(RAM_ORIGIN + RAM_BYTES as u32).to_le_bytes());
        flash[4..8].copy_from_slice(&(FLASH_ORIGIN + 9).to_le_bytes());
        assert!(validate_vectors(&flash).is_ok());
        flash[4..8].copy_from_slice(&(FLASH_ORIGIN + 8).to_le_bytes());
        assert!(validate_vectors(&flash).is_err());
    }
}
