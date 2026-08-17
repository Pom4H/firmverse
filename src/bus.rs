use std::cell::{Cell, RefCell};
use std::rc::Rc;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

pub const SRAM_BASE: u32 = 0x1FFF_0000;
pub const SRAM_SIZE: usize = 64 * 1024;
pub const HOST_RAM_BASE: u32 = 0x2000_0000;
pub const HOST_RAM_SIZE: usize = 128 * 1024;
pub const XIP_BASE: u32 = 0x1100_0000;
pub const XIP_SIZE: usize = 256 * 1024;
pub const MMIO_BASE: u32 = 0x4000_0000;
pub const MMIO_END: u32 = 0x5001_0000;
pub const ROM_END: u32 = 0x0002_0000;

pub const GPIO_BASE: u32 = 0x4000_8000;
const GPIO_WINDOW: u32 = 0x80;
pub const GPIO_PIN_MASK: u32 = (1 << 23) - 1;
const GPIO_DR: u32 = 0x00;
const GPIO_DDR: u32 = 0x04;
const GPIO_CTL: u32 = 0x08;
const GPIO_EXT: u32 = 0x50;

const UART0_BASE: u32 = 0x4000_4000;
const UART1_BASE: u32 = 0x4000_9000;
const UART_WINDOW: u32 = 0x100;
pub const ADC_CH_BASE: u32 = 0x4005_0400;
pub const ADC_CH_COUNT: usize = 9;
const PWM_BASE: u32 = 0x4000_E000;
pub const PWM_CHANNELS: usize = 6;
const TIM_CURRENT: [u32; 6] = [
    0x4000_1004,
    0x4000_1018,
    0x4000_102C,
    0x4000_1040,
    0x4000_1054,
    0x4000_1068,
];

#[derive(Clone, Copy, Debug, Default)]
pub struct GpioBank {
    pub dr: u32,
    pub ddr: u32,
    pub ctl: u32,
    pub ext: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct UartRegs {
    dll: u8,
    dlm: u8,
    ier: u32,
    fcr: u8,
    lcr: u8,
    mcr: u32,
    scr: u8,
}

pub struct Phy6252Bus {
    pub sram: Vec<u8>,
    pub host_ram: Rc<RefCell<Vec<u8>>>,
    pub xip: Vec<u8>,
    mmio: RefCell<[u32; 1024]>,
    pub gpio: Rc<RefCell<GpioBank>>,
    pub gpio_changed: Rc<RefCell<bool>>,
    pub uart_rx: Rc<RefCell<Vec<u8>>>,
    uart_regs: Rc<RefCell<[UartRegs; 2]>>,
    pub pwm: Rc<RefCell<[u32; PWM_CHANNELS]>>,
    pub pwm_changed: Rc<RefCell<bool>>,
    pub adc_mv: Rc<RefCell<[u16; ADC_CH_COUNT]>>,
    timer_count: Cell<u32>,
}

impl Phy6252Bus {
    pub fn new(sram: Vec<u8>, xip: Vec<u8>) -> Self {
        let mut adc = [0u16; ADC_CH_COUNT];
        adc[3] = 3_300;
        adc[4] = 1_650;
        adc[6] = 2_500;
        adc[7] = 3_300;
        Self {
            sram,
            host_ram: Rc::new(RefCell::new(vec![0u8; HOST_RAM_SIZE])),
            xip,
            mmio: RefCell::new([0xFFFF_FFFF; 1024]),
            gpio: Rc::new(RefCell::new(GpioBank::default())),
            gpio_changed: Rc::new(RefCell::new(false)),
            uart_rx: Rc::new(RefCell::new(Vec::new())),
            uart_regs: Rc::new(RefCell::new([UartRegs::default(); 2])),
            pwm: Rc::new(RefCell::new([0; PWM_CHANNELS])),
            pwm_changed: Rc::new(RefCell::new(false)),
            adc_mv: Rc::new(RefCell::new(adc)),
            timer_count: Cell::new(0xFFFF_0000),
        }
    }

    pub fn vector_table(&self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out.copy_from_slice(&self.sram[0..8]);
        out
    }

    fn gpio_off(addr: u32) -> Option<u32> {
        if addr < GPIO_BASE || addr >= GPIO_BASE + GPIO_WINDOW {
            return None;
        }
        Some(addr - GPIO_BASE)
    }

    fn gpio_read_reg(&self, off: u32) -> u32 {
        let gpio = self.gpio.borrow();
        match off & !3 {
            GPIO_DR => gpio.dr,
            GPIO_DDR => gpio.ddr,
            GPIO_CTL => gpio.ctl,
            GPIO_EXT => (gpio.dr & gpio.ddr) | (gpio.ext & !gpio.ddr),
            _ => 0,
        }
    }

    fn gpio_write_reg(&self, off: u32, value: u32) {
        let aligned = off & !3;
        let masked = value & GPIO_PIN_MASK;
        let mut gpio = self.gpio.borrow_mut();
        match aligned {
            GPIO_DR => {
                if gpio.dr != masked {
                    gpio.dr = masked;
                    *self.gpio_changed.borrow_mut() = true;
                }
            }
            GPIO_DDR => {
                if gpio.ddr != masked {
                    gpio.ddr = masked;
                    *self.gpio_changed.borrow_mut() = true;
                }
            }
            GPIO_CTL => gpio.ctl = masked,
            _ => {}
        }
    }

    fn gpio_write_partial(&self, addr: u32, value: u32, width: u32) {
        let off = addr - GPIO_BASE;
        let shift = (addr & 3) * 8;
        let bits = width * 8;
        let mask = if bits >= 32 {
            0xFFFF_FFFF
        } else {
            ((1u32 << bits) - 1) << shift
        };
        let cur = self.gpio_read_reg(off);
        self.gpio_write_reg(off, (cur & !mask) | ((value << shift) & mask));
    }

    fn mmio_index(addr: u32) -> Option<usize> {
        if addr < MMIO_BASE || addr >= MMIO_END {
            return None;
        }
        Some(((addr - MMIO_BASE) >> 2) as usize % 1024)
    }

    fn adc_read(&self, addr: u32) -> Option<u32> {
        if addr < ADC_CH_BASE {
            return None;
        }
        let off = (addr - ADC_CH_BASE) as usize;
        if off % 4 != 0 {
            return None;
        }
        let ch = off / 4;
        if ch >= ADC_CH_COUNT {
            return None;
        }
        Some(u32::from(self.adc_mv.borrow()[ch]))
    }

    fn pwm_write(&self, addr: u32, value: u32) -> bool {
        if addr < PWM_BASE || addr >= PWM_BASE + 0x80 {
            return false;
        }
        let off = addr - PWM_BASE;
        if off % 16 != 8 {
            return true;
        }
        let ch = (off / 16) as usize;
        if ch >= PWM_CHANNELS {
            return true;
        }
        let mut pwm = self.pwm.borrow_mut();
        if pwm[ch] != value {
            pwm[ch] = value;
            *self.pwm_changed.borrow_mut() = true;
        }
        true
    }

    fn uart_port(addr: u32) -> Option<(usize, u32)> {
        if addr >= UART0_BASE && addr < UART0_BASE + UART_WINDOW {
            Some((0, addr - UART0_BASE))
        } else if addr >= UART1_BASE && addr < UART1_BASE + UART_WINDOW {
            Some((1, addr - UART1_BASE))
        } else {
            None
        }
    }

    fn uart_read(&self, addr: u32) -> Option<u32> {
        let (port, off) = Self::uart_port(addr & !3)?;
        let regs = self.uart_regs.borrow();
        let uart = regs[port];
        let dlab = uart.lcr & 0x80 != 0;
        match off & !3 {
            0x00 => Some(if dlab { u32::from(uart.dll) } else { 0 }),
            0x04 => Some(if dlab { u32::from(uart.dlm) } else { uart.ier }),
            0x08 => Some(0x01), // IIR: no interrupt pending
            0x0C => Some(u32::from(uart.lcr)),
            0x10 => Some(uart.mcr),
            0x14 => Some(0x60), // LSR_THRE | LSR_TEMT
            0x1C => Some(u32::from(uart.scr)),
            0x7C => Some(0x06),     // USR_TFE | USR_TFNF, not busy
            0x80 | 0x84 => Some(0), // TFL/RFL empty
            _ => None,
        }
    }

    fn uart_write(&self, addr: u32, value: u32, width: u32) -> bool {
        let Some((port, off)) = Self::uart_port(addr) else {
            return false;
        };
        let aligned = off & !3;
        if !matches!(aligned, 0x00 | 0x04 | 0x08 | 0x0C | 0x10 | 0x1C) {
            return false;
        }
        let shift = (addr & 3) * 8;
        let low = ((value << shift) & 0xff) as u8;
        let mut regs = self.uart_regs.borrow_mut();
        let uart = &mut regs[port];
        let dlab = uart.lcr & 0x80 != 0;
        match aligned {
            0x00 => {
                if dlab {
                    uart.dll = low;
                } else {
                    self.uart_rx.borrow_mut().push(low);
                }
            }
            0x04 => {
                if dlab {
                    uart.dlm = low;
                } else if width >= 4 && addr & 3 == 0 {
                    uart.ier = value;
                } else {
                    let mask = if width == 1 { 0xff } else { 0xffff };
                    uart.ier = (uart.ier & !mask) | (value & mask);
                }
            }
            0x08 => uart.fcr = low,
            0x0C => uart.lcr = low,
            0x10 => uart.mcr = value,
            0x1C => uart.scr = low,
            _ => unreachable!(),
        }
        true
    }

    fn peripheral_read(&self, addr: u32) -> Option<u32> {
        let aligned = addr & !3;
        if TIM_CURRENT.contains(&aligned) {
            let value = self.timer_count.get();
            self.timer_count.set(value.wrapping_sub(4));
            return Some(value);
        }
        if let Some(value) = self.adc_read(aligned) {
            return Some(value);
        }
        self.uart_read(aligned)
    }
}

impl Bus for Phy6252Bus {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if let Some(value) = read_le32(&self.sram, SRAM_BASE, addr) {
            return Ok(value);
        }
        if let Some(value) = read_le32(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            return Ok(value);
        }
        if let Some(value) = read_le32(&self.xip, XIP_BASE, addr) {
            return Ok(value);
        }
        if addr < ROM_END {
            return Ok(0);
        }
        if let Some(off) = Self::gpio_off(addr) {
            return Ok(self.gpio_read_reg(off));
        }
        if let Some(value) = self.peripheral_read(addr) {
            return Ok(value);
        }
        if let Some(index) = Self::mmio_index(addr) {
            return Ok(self.mmio.borrow()[index]);
        }
        Err(Fault::DAccViol)
    }

    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        if let Some(value) = read_le16(&self.sram, SRAM_BASE, addr) {
            return Ok(value);
        }
        if let Some(value) = read_le16(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            return Ok(value);
        }
        if let Some(value) = read_le16(&self.xip, XIP_BASE, addr) {
            return Ok(value);
        }
        if addr < ROM_END {
            return Ok(0);
        }
        if let Some(off) = Self::gpio_off(addr) {
            return Ok((self.gpio_read_reg(off) >> ((addr & 3) * 8)) as u16);
        }
        if let Some(value) = self.peripheral_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u16);
        }
        if let Some(index) = Self::mmio_index(addr) {
            return Ok(self.mmio.borrow()[index] as u16);
        }
        Err(Fault::DAccViol)
    }

    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        if let Some(offset) = offset_in(&self.sram, SRAM_BASE, addr) {
            return Ok(self.sram[offset]);
        }
        if let Some(offset) = offset_in(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            return Ok(self.host_ram.borrow()[offset]);
        }
        if let Some(offset) = offset_in(&self.xip, XIP_BASE, addr) {
            return Ok(self.xip[offset]);
        }
        if addr < ROM_END {
            return Ok(0);
        }
        if let Some(off) = Self::gpio_off(addr) {
            return Ok((self.gpio_read_reg(off) >> ((addr & 3) * 8)) as u8);
        }
        if let Some(value) = self.peripheral_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u8);
        }
        if let Some(index) = Self::mmio_index(addr) {
            return Ok(self.mmio.borrow()[index] as u8);
        }
        Err(Fault::DAccViol)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        if let Some(offset) = offset_in(&self.sram, SRAM_BASE, addr) {
            self.sram[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            self.host_ram.borrow_mut()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if offset_in(&self.xip, XIP_BASE, addr).is_some() {
            return Ok(());
        }
        if addr < ROM_END {
            return Ok(());
        }
        if Self::gpio_off(addr).is_some() {
            self.gpio_write_partial(addr, value, 4);
            return Ok(());
        }
        if self.pwm_write(addr, value) {
            return Ok(());
        }
        if self.uart_write(addr, value, 4) {
            return Ok(());
        }
        if let Some(index) = Self::mmio_index(addr) {
            self.mmio.borrow_mut()[index] = value;
            return Ok(());
        }
        Err(Fault::DAccViol)
    }

    fn write16(&mut self, addr: u32, value: u16) -> Result<(), Fault> {
        if let Some(offset) = offset_in(&self.sram, SRAM_BASE, addr) {
            self.sram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            self.host_ram.borrow_mut()[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if offset_in(&self.xip, XIP_BASE, addr).is_some() {
            return Ok(());
        }
        if addr < ROM_END {
            return Ok(());
        }
        if Self::gpio_off(addr).is_some() {
            self.gpio_write_partial(addr, u32::from(value), 2);
            return Ok(());
        }
        if self.pwm_write(addr, u32::from(value)) {
            return Ok(());
        }
        if self.uart_write(addr, u32::from(value), 2) {
            return Ok(());
        }
        if let Some(index) = Self::mmio_index(addr) {
            self.mmio.borrow_mut()[index] = u32::from(value);
            return Ok(());
        }
        Err(Fault::DAccViol)
    }

    fn write8(&mut self, addr: u32, value: u8) -> Result<(), Fault> {
        if let Some(offset) = offset_in(&self.sram, SRAM_BASE, addr) {
            self.sram[offset] = value;
            return Ok(());
        }
        if let Some(offset) = offset_in(&self.host_ram.borrow(), HOST_RAM_BASE, addr) {
            self.host_ram.borrow_mut()[offset] = value;
            return Ok(());
        }
        if offset_in(&self.xip, XIP_BASE, addr).is_some() {
            return Ok(());
        }
        if addr < ROM_END {
            return Ok(());
        }
        if Self::gpio_off(addr).is_some() {
            self.gpio_write_partial(addr, u32::from(value), 1);
            return Ok(());
        }
        if self.uart_write(addr, u32::from(value), 1) {
            return Ok(());
        }
        if self.pwm_write(addr, u32::from(value)) {
            return Ok(());
        }
        if let Some(index) = Self::mmio_index(addr) {
            self.mmio.borrow_mut()[index] = u32::from(value);
            return Ok(());
        }
        Err(Fault::DAccViol)
    }

    fn in_range(&self, addr: u32) -> bool {
        offset_in(&self.sram, SRAM_BASE, addr).is_some()
            || offset_in(&self.host_ram.borrow(), HOST_RAM_BASE, addr).is_some()
            || offset_in(&self.xip, XIP_BASE, addr).is_some()
            || (addr >= MMIO_BASE && addr < MMIO_END)
            || addr < ROM_END
    }
}

fn offset_in(mem: &[u8], base: u32, addr: u32) -> Option<usize> {
    let offset = addr.wrapping_sub(base) as usize;
    if offset < mem.len() {
        Some(offset)
    } else {
        None
    }
}

fn read_le16(mem: &[u8], base: u32, addr: u32) -> Option<u16> {
    let offset = offset_in(mem, base, addr)?;
    let bytes = mem.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_le32(mem: &[u8], base: u32, addr: u32) -> Option<u32> {
    let offset = offset_in(mem, base, addr)?;
    let bytes = mem.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uart_dlab_divisor_writes_do_not_leak_into_tx_log() {
        let mut bus = Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0; XIP_SIZE]);
        bus.write8(UART0_BASE + 0x0C, 0x80).unwrap();
        bus.write8(UART0_BASE, 9).unwrap();
        bus.write8(UART0_BASE + 0x04, 0).unwrap();
        assert_eq!(bus.read32(UART0_BASE).unwrap(), 9);
        assert!(bus.uart_rx.borrow().is_empty());

        bus.write8(UART0_BASE + 0x0C, 0x03).unwrap();
        bus.write8(UART0_BASE, b'A').unwrap();
        assert_eq!(&*bus.uart_rx.borrow(), b"A");
    }

    #[test]
    fn uart_exposes_idle_status_and_ier_readback() {
        let mut bus = Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0; XIP_SIZE]);
        bus.write32(UART0_BASE + 0x04, 0x81).unwrap();
        assert_eq!(bus.read32(UART0_BASE + 0x04).unwrap(), 0x81);
        assert_eq!(bus.read32(UART0_BASE + 0x14).unwrap(), 0x60);
        assert_eq!(bus.read32(UART0_BASE + 0x7C).unwrap(), 0x06);
    }
}
