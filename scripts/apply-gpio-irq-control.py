from pathlib import Path

p = Path('src/bus.rs')
s = p.read_text()
s = s.replace(
'''pub struct GpioBank {
    pub dr: u32,
    pub ddr: u32,
    pub ctl: u32,
    pub ext: u32,
}
''',
'''pub struct GpioBank {
    pub dr: u32,
    pub ddr: u32,
    pub ctl: u32,
    pub inten: u32,
    pub intmask: u32,
    pub ext: u32,
}
''', 1)
s = s.replace(
'''            GPIO_CTL => gpio.ctl,
            GPIO_EXT => (gpio.dr & gpio.ddr) | (gpio.ext & !gpio.ddr),
''',
'''            GPIO_CTL => gpio.ctl,
            0x30 => gpio.inten,
            0x34 => gpio.intmask,
            GPIO_EXT => (gpio.dr & gpio.ddr) | (gpio.ext & !gpio.ddr),
''', 1)
s = s.replace(
'''            GPIO_CTL => gpio.ctl = masked,
            _ => {}
''',
'''            GPIO_CTL => gpio.ctl = masked,
            0x30 => gpio.inten = masked,
            0x34 => gpio.intmask = masked,
            _ => {}
''', 1)
# Add a focused test alongside existing bus tests.
needle = '''    #[test]
    fn uart_dlab_divisor_writes_do_not_leak_into_tx_log() {
'''
test = '''    #[test]
    fn gpio_irq_enable_and_mask_are_stateful() {
        let mut bus = Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0; XIP_SIZE]);
        bus.write32(GPIO_BASE + 0x30, 0x15).unwrap();
        bus.write32(GPIO_BASE + 0x34, 0x0a).unwrap();
        assert_eq!(bus.read32(GPIO_BASE + 0x30).unwrap(), 0x15);
        assert_eq!(bus.read32(GPIO_BASE + 0x34).unwrap(), 0x0a);
    }

'''
if 'gpio_irq_enable_and_mask_are_stateful' not in s:
    if needle not in s:
        raise SystemExit('bus test marker not found')
    s = s.replace(needle, test + needle, 1)
p.write_text(s)

p = Path('src/discovery.rs')
s = p.read_text()
s = s.replace(
'''        match aligned.wrapping_sub(GPIO_BASE) {
            0x00 | 0x04 | 0x08 => true,
            0x50 => !write,
''',
'''        match aligned.wrapping_sub(GPIO_BASE) {
            0x00 | 0x04 | 0x08 | 0x30 | 0x34 => true,
            0x50 => !write,
''', 1)
p.write_text(s)
