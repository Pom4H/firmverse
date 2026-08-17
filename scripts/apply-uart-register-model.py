from pathlib import Path

p = Path('src/bus.rs')
s = p.read_text()

s = s.replace(
'''#[derive(Clone, Copy, Debug, Default)]
pub struct GpioBank {
    pub dr: u32,
    pub ddr: u32,
    pub ctl: u32,
    pub ext: u32,
}
''',
'''#[derive(Clone, Copy, Debug, Default)]
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
''', 1)

s = s.replace(
'''    pub uart_rx: Rc<RefCell<Vec<u8>>>,
    pub pwm: Rc<RefCell<[u32; PWM_CHANNELS]>>,
''',
'''    pub uart_rx: Rc<RefCell<Vec<u8>>>,
    uart_regs: Rc<RefCell<[UartRegs; 2]>>,
    pub pwm: Rc<RefCell<[u32; PWM_CHANNELS]>>,
''', 1)

s = s.replace(
'''            uart_rx: Rc::new(RefCell::new(Vec::new())),
            pwm: Rc::new(RefCell::new([0; PWM_CHANNELS])),
''',
'''            uart_rx: Rc::new(RefCell::new(Vec::new())),
            uart_regs: Rc::new(RefCell::new([UartRegs::default(); 2])),
            pwm: Rc::new(RefCell::new([0; PWM_CHANNELS])),
''', 1)

old_uart = '''    fn uart_write(&self, addr: u32, value: u8) -> bool {
        let base = if addr >= UART0_BASE && addr < UART0_BASE + UART_WINDOW {
            UART0_BASE
        } else if addr >= UART1_BASE && addr < UART1_BASE + UART_WINDOW {
            UART1_BASE
        } else {
            return false;
        };
        if addr - base == 0 {
            self.uart_rx.borrow_mut().push(value);
        }
        true
    }
'''
new_uart = '''    fn uart_port(addr: u32) -> Option<(usize, u32)> {
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
            0x7C => Some(0x06), // USR_TFE | USR_TFNF, not busy
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
'''
if old_uart not in s:
    raise SystemExit('old uart_write not found')
s = s.replace(old_uart, new_uart, 1)

s = s.replace('''        uart_status(aligned)
    }
}

fn uart_status(addr: u32) -> Option<u32> {
    let off = if addr >= UART0_BASE && addr < UART0_BASE + UART_WINDOW {
        addr - UART0_BASE
    } else if addr >= UART1_BASE && addr < UART1_BASE + UART_WINDOW {
        addr - UART1_BASE
    } else {
        return None;
    };
    match off {
        0x08 => Some(0x01),
        0x14 => Some(0x60),
        0x7C => Some(0x06),
        0x80 | 0x84 => Some(0),
        _ => None,
    }
}
''', '''        self.uart_read(aligned)
    }
}
''', 1)

s = s.replace('self.uart_write(addr, value as u8)', 'self.uart_write(addr, value, 4)')
s = s.replace('self.uart_write(addr, value as u8)', 'self.uart_write(addr, u32::from(value), 2)')
# The first replacement also hits write16 if still textual value-as-u8; repair by context.
s = s.replace('''if self.uart_write(addr, value, 4) {
            return Ok(());
        }
        if let Some(index) = Self::mmio_index(addr) {
            self.mmio.borrow_mut()[index] = u32::from(value);''', '''if self.uart_write(addr, u32::from(value), 2) {
            return Ok(());
        }
        if let Some(index) = Self::mmio_index(addr) {
            self.mmio.borrow_mut()[index] = u32::from(value);''')
s = s.replace('self.uart_write(addr, value)', 'self.uart_write(addr, u32::from(value), 1)')
# Restore write32 call if generic replacement touched it.
s = s.replace('self.uart_write(addr, u32::from(value), 1, 4)', 'self.uart_write(addr, value, 4)')

if '#[cfg(test)]\nmod tests {' not in s:
    s += '''

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
'''

p.write_text(s)

p = Path('src/discovery.rs')
s = p.read_text()
s = s.replace(
'''                0x08 | 0x14 | 0x7C | 0x80 | 0x84
''',
'''                0x00 | 0x04 | 0x08 | 0x0C | 0x10 | 0x14 | 0x1C | 0x7C | 0x80 | 0x84
''', 1)
s = s.replace(
'''    fn uart_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        aligned == UART0_BASE || aligned == UART1_BASE
    }
''',
'''    fn uart_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        [UART0_BASE, UART1_BASE].iter().any(|base| {
            matches!(aligned.wrapping_sub(*base), 0x00 | 0x04 | 0x08 | 0x0C | 0x10 | 0x1C)
        })
    }
''', 1)
p.write_text(s)
