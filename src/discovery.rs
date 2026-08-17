use crate::aes::aes128_encrypt_block;
use crate::bus::{Phy6252Bus, ADC_CH_BASE, MMIO_BASE, MMIO_END, PWM_CHANNELS, ROM_END, XIP_BASE};
use crate::silicon_regs;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

const GPIO_BASE: u32 = 0x4000_8000;
const UART0_BASE: u32 = 0x4000_4000;
const UART1_BASE: u32 = 0x4000_9000;
const PWM_BASE: u32 = 0x4000_E000;
const SPIF_BASE: u32 = 0x4000_C800;
const WAKEUP_MASK_31_0: u32 = 0x4000_F0A0;
const WAKEUP_MASK_34_32: u32 = 0x4000_F0A4;
const VECTOR_MIRROR_BYTES: u32 = 0xC0;
const THUMB_BX_LR: u16 = 0x4770;

const PCR_SW_RESET0: u32 = 0x4000_0000;
const PCR_SW_RESET1: u32 = 0x4000_0004;
const PCR_SW_CLK: u32 = 0x4000_0008;
const PCR_SW_RESET2: u32 = 0x4000_000C;
const PCR_SW_RESET3: u32 = 0x4000_0010;
const PCR_SW_CLK1: u32 = 0x4000_0014;
const PCR_APB_CLK: u32 = 0x4000_0018;
const PCR_APB_CLK_UPDATE: u32 = 0x4000_001C;
const PCR_CACHE_CLOCK_GATE: u32 = 0x4000_0020;
const PCR_CACHE_RST: u32 = 0x4000_0024;
const PCR_CACHE_BYPASS: u32 = 0x4000_0028;

const AON_PWROFF: u32 = 0x4000_F000;
const AON_PWRSLP: u32 = 0x4000_F004;
const AON_IOCTL0: u32 = 0x4000_F008;
const AON_IOCTL1: u32 = 0x4000_F00C;
const AON_IOCTL2: u32 = 0x4000_F010;
const AON_PMCTL0: u32 = 0x4000_F014;
const AON_PMCTL1: u32 = 0x4000_F018;
const AON_PMCTL2_0: u32 = 0x4000_F01C;
const AON_PMCTL2_1: u32 = 0x4000_F020;
const AON_XTAL_16M_CTRL: u32 = 0x4000_F0BC;
const AON_SLEEP_R1: u32 = 0x4000_F0C4;
const PCRM_EFUSE_CFG: u32 = 0x4000_F054;
const PCRM_EFUSE_PROG0: u32 = 0x4000_F140;
const PCRM_EFUSE_PROG1: u32 = 0x4000_F144;

const SECURE_KEY_TAIL: u32 = 0x1100_2908;
const SECURE_PLAINTEXT: u32 = 0x1100_2910;
const SECURE_EXPECTED: u32 = 0x1100_2920;
const FINIDV_STATUS: u32 = 0x1FFF_6128;
const FINIDV_SECONDARY: u32 = 0x1FFF_86A0;

const EMU_SLEEP_ALLOWED: u32 = 0x5000_FF00;
const EMU_SLEEP_MODE: u32 = 0x5000_FF04;
const EMU_AES_KEY_PTR: u32 = 0x5000_FF10;
const EMU_AES_PLAINTEXT_PTR: u32 = 0x5000_FF14;
const EMU_AES_CIPHERTEXT_PTR: u32 = 0x5000_FF18;
const EMU_AES_TRIGGER: u32 = 0x5000_FF1C;
const EMU_FINIDV_TRIGGER: u32 = 0x5000_FF20;
const EMU_FINIDV_RESULT: u32 = 0x5000_FF24;
const EMU_HEAP_BASE: u32 = 0x5000_FF30;
const EMU_HEAP_SIZE: u32 = 0x5000_FF34;

const TIM_CURRENT: [u32; 6] = [
    0x4000_1004,
    0x4000_1018,
    0x4000_102C,
    0x4000_1040,
    0x4000_1054,
    0x4000_1068,
];

struct StubReg {
    addr: u32,
    name: &'static str,
    reset: u32,
}

const KNOWN_STUB_REGS: &[StubReg] = &[
    StubReg {
        addr: PCR_SW_RESET0,
        name: "PCR.SW_RESET0",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_SW_RESET1,
        name: "PCR.SW_RESET1",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_SW_CLK,
        name: "PCR.SW_CLK",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_SW_RESET2,
        name: "PCR.SW_RESET2",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_SW_RESET3,
        name: "PCR.SW_RESET3",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_SW_CLK1,
        name: "PCR.SW_CLK1",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_APB_CLK,
        name: "PCR.APB_CLK",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: PCR_APB_CLK_UPDATE,
        name: "PCR.APB_CLK_UPDATE",
        reset: 0,
    },
    StubReg {
        addr: PCR_CACHE_CLOCK_GATE,
        name: "PCR.CACHE_CLOCK_GATE",
        reset: 0,
    },
    StubReg {
        addr: PCR_CACHE_RST,
        name: "PCR.CACHE_RST",
        reset: 0,
    },
    StubReg {
        addr: PCR_CACHE_BYPASS,
        name: "PCR.CACHE_BYPASS",
        reset: 0,
    },
    StubReg {
        addr: 0x4000_2000,
        name: "WDT.CR",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: 0x4000_2004,
        name: "WDT.TORR",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: 0x4000_200C,
        name: "WDT.CRR",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: 0x4000_2014,
        name: "WDT.EOI",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: 0x4000_5000,
        name: "I2C0.IC_CON",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: 0x4000_6000,
        name: "SPI0",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: AON_PWROFF,
        name: "AON.PWROFF",
        reset: 0,
    },
    StubReg {
        addr: AON_PWRSLP,
        name: "AON.PWRSLP",
        reset: 0,
    },
    StubReg {
        addr: AON_IOCTL0,
        name: "AON.IOCTL[0]",
        reset: 0,
    },
    StubReg {
        addr: AON_IOCTL1,
        name: "AON.IOCTL[1]",
        reset: 0,
    },
    StubReg {
        addr: AON_IOCTL2,
        name: "AON.IOCTL[2]",
        reset: 0,
    },
    StubReg {
        addr: AON_PMCTL0,
        name: "AON.PMCTL0",
        reset: 0,
    },
    StubReg {
        addr: AON_PMCTL1,
        name: "AON.PMCTL1",
        reset: 0,
    },
    StubReg {
        addr: AON_PMCTL2_0,
        name: "AON.PMCTL2_0",
        reset: 0,
    },
    StubReg {
        addr: AON_PMCTL2_1,
        name: "AON.PMCTL2_1",
        reset: 0,
    },
    StubReg {
        addr: 0x4000_F03C,
        name: "PCRM.CLKSEL",
        reset: 0xFFFF_FFFF,
    },
    StubReg {
        addr: AON_XTAL_16M_CTRL,
        name: "AON.XTAL_16M_CTRL",
        reset: 0,
    },
    StubReg {
        addr: AON_SLEEP_R1,
        name: "AON.SLEEP_R[1]",
        reset: 0,
    },
    StubReg {
        addr: PCRM_EFUSE_CFG,
        name: "PCRM.efuse_cfg",
        reset: 0,
    },
    StubReg {
        addr: PCRM_EFUSE_PROG0,
        name: "PCRM.EFUSE_PROG[0]",
        reset: 0,
    },
    StubReg {
        addr: PCRM_EFUSE_PROG1,
        name: "PCRM.EFUSE_PROG[1]",
        reset: 0,
    },
];

struct RomShim {
    entry: u32,
    name: &'static str,
    behavior: &'static str,
    code: &'static [u8],
}

const AEABI_MEMCLR4_CODE: &[u8] = &[
    0x00, 0x22, 0x00, 0x29, 0x03, 0xD0, 0x02, 0x70, 0x01, 0x30, 0x01, 0x39, 0xF9, 0xD1, 0x70, 0x47,
];
const DRV_IRQ_INIT_CODE: &[u8] = &[0x70, 0x47, 0x70, 0x47];
const EFUSE_READ_CODE: &[u8] = &[
    0x00, 0x22, 0x0A, 0x60, 0x4A, 0x60, 0x00, 0x20, 0x70, 0x47, 0x70, 0x47,
];
const AES128_ENCRYPT0_CODE: &[u8] = &[
    0x03, 0x4B, 0x18, 0x60, 0x59, 0x60, 0x9A, 0x60, 0x01, 0x20, 0xD8, 0x60, 0x70, 0x47, 0x00, 0xBF,
    0x10, 0xFF, 0x00, 0x50,
];
const OSAL_MEMCMP_CODE: &[u8] = &[
    0x10, 0xB4, 0x00, 0x2A, 0x07, 0xD0, 0x03, 0x78, 0x0C, 0x78, 0xA3, 0x42, 0x06, 0xD1, 0x01, 0x30,
    0x01, 0x31, 0x01, 0x3A, 0xF7, 0xD1, 0x01, 0x20, 0x10, 0xBC, 0x70, 0x47, 0x00, 0x20, 0x10, 0xBC,
    0x70, 0x47,
];
const FINIDV_CODE: &[u8] = &[
    0x02, 0x4B, 0x01, 0x20, 0x18, 0x60, 0x58, 0x68, 0x70, 0x47, 0x00, 0xBF, 0x20, 0xFF, 0x00, 0x50,
];
const OSAL_MEM_SET_HEAP_CODE: &[u8] = &[
    0x02, 0x4A, 0x10, 0x60, 0x51, 0x60, 0x70, 0x47, 0x00, 0xBF, 0xC0, 0x46, 0x30, 0xFF, 0x00, 0x50,
];
const ENABLE_SLEEP_CODE: &[u8] = &[
    0x01, 0x20, 0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xFF, 0x00, 0x50,
];
const DISABLE_SLEEP_CODE: &[u8] = &[
    0x00, 0x20, 0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xFF, 0x00, 0x50,
];
const SET_SLEEP_MODE_CODE: &[u8] = &[
    0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xBF, 0x04, 0xFF, 0x00, 0x50,
];

const ROM_SHIMS: &[RomShim] = &[
    RomShim {
        entry: 0x0001_6DC4,
        name: "spif_config",
        behavior: "noop-return (host XIP backend already configured)",
        code: DRV_IRQ_INIT_CODE,
    },
    RomShim {
        entry: 0x0000_8AA8,
        name: "clk_init ROM helper 0x8AA9",
        behavior: "identity-r0 (observed RC32M->XTAL16M boot path)",
        code: DRV_IRQ_INIT_CODE,
    },
    RomShim {
        entry: 0x0000_8C00,
        name: "clk_init ROM helper 0x8C01",
        behavior: "identity-r0 (observed RC32M->XTAL16M boot path)",
        code: DRV_IRQ_INIT_CODE,
    },
    RomShim {
        entry: 0x0000_0EB2,
        name: "__aeabi_memclr4",
        behavior: "cortex-m0-byte-clear",
        code: AEABI_MEMCLR4_CODE,
    },
    RomShim {
        entry: 0x0000_3FDC,
        name: "LL_ENC_AES128_Encrypt0",
        behavior: "host-aes128-key-plaintext-ciphertext",
        code: AES128_ENCRYPT0_CODE,
    },
    RomShim {
        entry: 0x0000_A2E0,
        name: "finidv",
        behavior: "secure-id-efuse-aes-compare",
        code: FINIDV_CODE,
    },
    RomShim {
        entry: 0x0000_A9C8,
        name: "drv_irq_init",
        behavior: "noop-return",
        code: DRV_IRQ_INIT_CODE,
    },
    RomShim {
        entry: 0x0000_ACE0,
        name: "efuse_read",
        behavior: "blank-8-byte-read-success",
        code: EFUSE_READ_CODE,
    },
    RomShim {
        entry: 0x0000_A920,
        name: "disableSleep",
        behavior: "sleep-allowed=false",
        code: DISABLE_SLEEP_CODE,
    },
    RomShim {
        entry: 0x0000_AEAC,
        name: "enableSleep",
        behavior: "sleep-allowed=true",
        code: ENABLE_SLEEP_CODE,
    },
    RomShim {
        entry: 0x0001_4CB4,
        name: "osal_mem_set_heap",
        behavior: "capture-heap-base-size",
        code: OSAL_MEM_SET_HEAP_CODE,
    },
    RomShim {
        entry: 0x0001_4CCC,
        name: "osal_memcmp",
        behavior: "cortex-m0-byte-compare",
        code: OSAL_MEMCMP_CODE,
    },
    RomShim {
        entry: 0x0001_6B44,
        name: "setSleepMode",
        behavior: "sleep-mode=r0",
        code: SET_SLEEP_MODE_CODE,
    },
];

pub struct DiscoveryBus {
    inner: Phy6252Bus,
    strict: bool,
    sparse_mmio: RefCell<HashMap<u32, u32>>,
    seen_unknown: RefCell<HashSet<u32>>,
    seen_rom: RefCell<HashSet<u32>>,
    seen_shims: RefCell<HashSet<u32>>,
    sleep_allowed: Cell<bool>,
    sleep_mode: Cell<u32>,
    aes_key_ptr: Cell<u32>,
    aes_plaintext_ptr: Cell<u32>,
    aes_ciphertext_ptr: Cell<u32>,
    finidv_result: Cell<u32>,
    heap_base: Cell<u32>,
    heap_size: Cell<u32>,
}

impl DiscoveryBus {
    pub fn new(mut inner: Phy6252Bus, strict: bool) -> Self {
        Self::seed_development_secure_profile(&mut inner);
        Self {
            inner,
            strict,
            sparse_mmio: RefCell::new(HashMap::new()),
            seen_unknown: RefCell::new(HashSet::new()),
            seen_rom: RefCell::new(HashSet::new()),
            seen_shims: RefCell::new(HashSet::new()),
            sleep_allowed: Cell::new(false),
            sleep_mode: Cell::new(0),
            aes_key_ptr: Cell::new(0),
            aes_plaintext_ptr: Cell::new(0),
            aes_ciphertext_ptr: Cell::new(0),
            finidv_result: Cell::new(0),
            heap_base: Cell::new(0),
            heap_size: Cell::new(0),
        }
    }

    fn seed_development_secure_profile(inner: &mut Phy6252Bus) {
        let start = (SECURE_KEY_TAIL - XIP_BASE) as usize;
        let end = (SECURE_EXPECTED + 16 - XIP_BASE) as usize;
        if end > inner.xip.len() || !inner.xip[start..end].iter().all(|byte| *byte == 0) {
            return;
        }
        let expected = aes128_encrypt_block([0; 16], [0; 16]);
        let expected_off = (SECURE_EXPECTED - XIP_BASE) as usize;
        inner.xip[expected_off..expected_off + 16].copy_from_slice(&expected);
        eprintln!("SEC factory_profile=development deterministic-aes128");
    }

    pub fn sleep_allowed(&self) -> bool {
        self.sleep_allowed.get()
    }
    pub fn sleep_mode(&self) -> u32 {
        self.sleep_mode.get()
    }
    fn is_mmio(addr: u32) -> bool {
        (MMIO_BASE..MMIO_END).contains(&addr)
    }
    fn is_unmodeled_rom(addr: u32) -> bool {
        (VECTOR_MIRROR_BYTES..ROM_END).contains(&addr)
    }

    fn rom_shim_for_addr(addr: u32) -> Option<(&'static RomShim, usize)> {
        ROM_SHIMS.iter().find_map(|shim| {
            let offset = addr.checked_sub(shim.entry)? as usize;
            (offset < shim.code.len()).then_some((shim, offset))
        })
    }

    fn rom_shim_byte(&self, addr: u32) -> Option<u8> {
        let (shim, offset) = Self::rom_shim_for_addr(addr)?;
        if self.seen_shims.borrow_mut().insert(shim.entry) {
            eprintln!(
                "ROM shim {} entry={:#010x} behavior={}",
                shim.name, shim.entry, shim.behavior
            );
        }
        Some(shim.code[offset])
    }

    fn rom_shim_read(&self, addr: u32, width: usize) -> Option<u32> {
        let mut value = 0u32;
        for i in 0..width {
            value |= u32::from(self.rom_shim_byte(addr + i as u32)?) << (i * 8);
        }
        Some(value)
    }

    fn gpio_known(addr: u32, write: bool) -> bool {
        let aligned = addr & !3;
        match aligned.wrapping_sub(GPIO_BASE) {
            0x00 | 0x04 | 0x08 | 0x30 | 0x34 => true,
            0x50 => !write,
            _ => false,
        }
    }

    fn uart_read_known(addr: u32) -> bool {
        let aligned = addr & !3;
        [UART0_BASE, UART1_BASE].iter().any(|base| {
            matches!(
                aligned.wrapping_sub(*base),
                0x00 | 0x04 | 0x08 | 0x0C | 0x10 | 0x14 | 0x1C | 0x7C | 0x80 | 0x84
            )
        })
    }

    fn uart_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        [UART0_BASE, UART1_BASE].iter().any(|base| {
            matches!(
                aligned.wrapping_sub(*base),
                0x00 | 0x04 | 0x08 | 0x0C | 0x10 | 0x1C
            )
        })
    }

    fn adc_read_known(addr: u32) -> bool {
        let aligned = addr & !3;
        aligned >= ADC_CH_BASE && aligned < ADC_CH_BASE + 9 * 4
    }

    fn pwm_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        (0..PWM_CHANNELS as u32).any(|ch| aligned == PWM_BASE + ch * 16 + 8)
    }

    fn spif_bootstrap_write_name(addr: u32) -> Option<&'static str> {
        match (addr & !3).wrapping_sub(SPIF_BASE) {
            0x38 => Some("SPIF.WR_COMPLETION_CTRL"),
            0x50 => Some("SPIF.LOW_WR_PROTECTION"),
            0x54 => Some("SPIF.UP_WR_PROTECTION"),
            0x58 => Some("SPIF.WR_PROTECTION"),
            _ => None,
        }
    }

    fn wakeup_bootstrap_write_name(addr: u32) -> Option<&'static str> {
        match addr & !3 {
            WAKEUP_MASK_31_0 => Some("WAKEUP.io_wu_mask_31_0"),
            WAKEUP_MASK_34_32 => Some("WAKEUP.io_wu_mask_34_32"),
            _ => None,
        }
    }

    fn timer_read_known(addr: u32) -> bool {
        TIM_CURRENT.contains(&(addr & !3))
    }

    fn functional_read(addr: u32) -> bool {
        Self::gpio_known(addr, false)
            || Self::uart_read_known(addr)
            || Self::adc_read_known(addr)
            || Self::timer_read_known(addr)
    }

    fn functional_write(addr: u32) -> bool {
        Self::gpio_known(addr, true) || Self::uart_write_known(addr) || Self::pwm_write_known(addr)
    }

    fn storage_reset(addr: u32) -> Option<u32> {
        let aligned = addr & !3;
        KNOWN_STUB_REGS
            .iter()
            .find(|reg| reg.addr == aligned)
            .map(|reg| reg.reset)
            .or_else(|| silicon_regs::storage_reg(aligned).map(|reg| reg.reset))
    }

    fn sparse_read(&self, addr: u32) -> u32 {
        let aligned = addr & !3;
        self.sparse_mmio
            .borrow()
            .get(&aligned)
            .copied()
            .unwrap_or_else(|| Self::storage_reset(aligned).unwrap_or(0xFFFF_FFFF))
    }

    fn sparse_write(&self, addr: u32, value: u32, width: u32) {
        let aligned = addr & !3;
        let shift = (addr & 3) * 8;
        let bits = width * 8;
        let mask = if bits >= 32 {
            0xFFFF_FFFF
        } else {
            ((1u32 << bits) - 1) << shift
        };
        let mut mmio = self.sparse_mmio.borrow_mut();
        let current = mmio
            .get(&aligned)
            .copied()
            .unwrap_or_else(|| Self::storage_reset(aligned).unwrap_or(0xFFFF_FFFF));
        mmio.insert(aligned, (current & !mask) | ((value << shift) & mask));
    }

    fn guest_read_block(&self, addr: u32) -> Result<[u8; 16], Fault> {
        let mut out = [0u8; 16];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = self.inner.read8(addr.wrapping_add(i as u32))?;
        }
        Ok(out)
    }

    fn guest_write_block(&mut self, addr: u32, data: &[u8; 16]) -> Result<(), Fault> {
        for (i, byte) in data.iter().copied().enumerate() {
            self.inner.write8(addr.wrapping_add(i as u32), byte)?;
        }
        Ok(())
    }

    fn run_guest_aes128(&mut self) -> Result<(), Fault> {
        let key_ptr = self.aes_key_ptr.get();
        let plaintext_ptr = self.aes_plaintext_ptr.get();
        let ciphertext_ptr = self.aes_ciphertext_ptr.get();
        let key = self.guest_read_block(key_ptr)?;
        let plaintext = self.guest_read_block(plaintext_ptr)?;
        let ciphertext = aes128_encrypt_block(key, plaintext);
        self.guest_write_block(ciphertext_ptr, &ciphertext)?;
        eprintln!("ROM AES128 key={key_ptr:#010x} plaintext={plaintext_ptr:#010x} ciphertext={ciphertext_ptr:#010x}");
        Ok(())
    }

    fn run_finidv(&mut self) -> Result<u32, Fault> {
        if self.inner.read8(FINIDV_STATUS)? == 1 {
            eprintln!("SEC finidv=pass cached=true");
            return Ok(1);
        }
        let mut key = [0u8; 16];
        for i in 0..8 {
            key[8 + i] = self.inner.read8(SECURE_KEY_TAIL + i as u32)?;
        }
        let plaintext = self.guest_read_block(SECURE_PLAINTEXT)?;
        let expected = self.guest_read_block(SECURE_EXPECTED)?;
        let actual = aes128_encrypt_block(key, plaintext);
        if actual != expected {
            self.inner.write8(FINIDV_STATUS, 0xFF)?;
            eprintln!("SEC finidv=fail reason=aes-mismatch");
            return Ok(0);
        }
        self.inner.write8(FINIDV_STATUS, 1)?;
        let secondary = aes128_encrypt_block(key, expected);
        self.guest_write_block(FINIDV_SECONDARY, &secondary)?;
        eprintln!("SEC finidv=pass cached=false");
        Ok(1)
    }

    fn emu_control_read(&self, addr: u32) -> Option<u32> {
        match addr & !3 {
            EMU_SLEEP_ALLOWED => Some(u32::from(self.sleep_allowed.get())),
            EMU_SLEEP_MODE => Some(self.sleep_mode.get()),
            EMU_AES_KEY_PTR => Some(self.aes_key_ptr.get()),
            EMU_AES_PLAINTEXT_PTR => Some(self.aes_plaintext_ptr.get()),
            EMU_AES_CIPHERTEXT_PTR => Some(self.aes_ciphertext_ptr.get()),
            EMU_AES_TRIGGER => Some(0),
            EMU_FINIDV_TRIGGER => Some(0),
            EMU_FINIDV_RESULT => Some(self.finidv_result.get()),
            EMU_HEAP_BASE => Some(self.heap_base.get()),
            EMU_HEAP_SIZE => Some(self.heap_size.get()),
            _ => None,
        }
    }

    fn emu_control_write(&mut self, addr: u32, value: u32) -> Result<bool, Fault> {
        match addr & !3 {
            EMU_SLEEP_ALLOWED => {
                let new_value = value != 0;
                if self.sleep_allowed.replace(new_value) != new_value {
                    eprintln!("PWR sleep_allowed={new_value}");
                }
                Ok(true)
            }
            EMU_SLEEP_MODE => {
                let old = self.sleep_mode.replace(value);
                if old != value {
                    let name = match value {
                        0 => "MCU_SLEEP_MODE",
                        1 => "SYSTEM_SLEEP_MODE",
                        2 => "SYSTEM_OFF_MODE",
                        _ => "UNKNOWN",
                    };
                    eprintln!("PWR sleep_mode={value} ({name})");
                }
                Ok(true)
            }
            EMU_AES_KEY_PTR => {
                self.aes_key_ptr.set(value);
                Ok(true)
            }
            EMU_AES_PLAINTEXT_PTR => {
                self.aes_plaintext_ptr.set(value);
                Ok(true)
            }
            EMU_AES_CIPHERTEXT_PTR => {
                self.aes_ciphertext_ptr.set(value);
                Ok(true)
            }
            EMU_AES_TRIGGER => {
                if value != 0 {
                    self.run_guest_aes128()?;
                }
                Ok(true)
            }
            EMU_FINIDV_TRIGGER => {
                if value != 0 {
                    let result = self.run_finidv()?;
                    self.finidv_result.set(result);
                }
                Ok(true)
            }
            EMU_HEAP_BASE => {
                self.heap_base.set(value);
                Ok(true)
            }
            EMU_HEAP_SIZE => {
                self.heap_size.set(value);
                eprintln!(
                    "OSAL heap base={:#010x} size={:#x}",
                    self.heap_base.get(),
                    value
                );
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn unknown(&self, op: &str, addr: u32) -> Result<(), Fault> {
        let aligned = addr & !3;
        let first = self.seen_unknown.borrow_mut().insert(aligned);
        if first {
            if self.strict {
                eprintln!(
                    "MMIO unknown {op} addr={addr:#010x} aligned={aligned:#010x} -- strict fault"
                );
            } else {
                eprintln!(
                    "MMIO unknown {op} addr={addr:#010x} aligned={aligned:#010x} -- sparse stub"
                );
            }
        }
        if self.strict {
            Err(Fault::DAccViol)
        } else {
            Ok(())
        }
    }

    fn rom_unknown<T>(&self, op: &str, addr: u32) -> Result<T, Fault> {
        let first = self.seen_rom.borrow_mut().insert(addr & !1);
        if first {
            eprintln!("ROM unknown {op} addr={addr:#010x} -- vendor ROM image/ABI not modeled; strict fault");
        }
        Err(Fault::DAccViol)
    }

    fn read_fallback(&self, op: &str, addr: u32) -> Result<u32, Fault> {
        if Self::storage_reset(addr).is_some() {
            return Ok(self.sparse_read(addr));
        }
        self.unknown(op, addr)?;
        Ok(self.sparse_read(addr))
    }

    fn write_fallback(&self, op: &str, addr: u32, value: u32, width: u32) -> Result<(), Fault> {
        if Self::storage_reset(addr).is_none() {
            self.unknown(op, addr)?;
        }
        self.sparse_write(addr, value, width);
        Ok(())
    }
}

impl Bus for DiscoveryBus {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if let Some(value) = self.rom_shim_read(addr, 4) {
            return Ok(value);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok(value);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read32", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read32(addr);
        }
        self.read_fallback("read32", addr)
    }

    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        if let Some(value) = self.rom_shim_read(addr, 2) {
            return Ok(value as u16);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u16);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read16", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read16(addr);
        }
        let word = self.read_fallback("read16", addr)?;
        Ok((word >> ((addr & 3) * 8)) as u16)
    }

    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        if let Some(value) = self.rom_shim_byte(addr) {
            return Ok(value);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u8);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read8", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read8(addr);
        }
        let word = self.read_fallback("read8", addr)?;
        Ok((word >> ((addr & 3) * 8)) as u8)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        if self.emu_control_write(addr, value)? {
            return Ok(());
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write32", addr);
        }
        if let Some(name) = Self::spif_bootstrap_write_name(addr) {
            eprintln!("SPIF config {name}={value:#010x}");
            self.sparse_write(addr, value, 4);
            return Ok(());
        }
        if let Some(name) = Self::wakeup_bootstrap_write_name(addr) {
            eprintln!("GPIO bootstrap {name}={value:#010x}");
            self.sparse_write(addr, value, 4);
            return Ok(());
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write32(addr, value);
        }
        self.write_fallback("write32", addr, value, 4)
    }

    fn write16(&mut self, addr: u32, value: u16) -> Result<(), Fault> {
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write16", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write16(addr, value);
        }
        self.write_fallback("write16", addr, u32::from(value), 2)
    }

    fn write8(&mut self, addr: u32, value: u8) -> Result<(), Fault> {
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write8", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write8(addr, value);
        }
        self.write_fallback("write8", addr, u32::from(value), 1)
    }

    fn in_range(&self, addr: u32) -> bool {
        self.inner.in_range(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{SRAM_BASE, SRAM_SIZE, XIP_SIZE};

    fn bus(strict: bool) -> DiscoveryBus {
        DiscoveryBus::new(
            Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0; XIP_SIZE]),
            strict,
        )
    }

    #[test]
    fn sparse_mmio_is_full_address_and_preserves_partial_writes() {
        let mut bus = bus(false);
        let a = 0x4001_0000;
        let b = 0x4001_1000;
        bus.write32(a, 0x1122_3344).unwrap();
        bus.write32(b, 0xAABB_CCDD).unwrap();
        bus.write8(a + 1, 0xAA).unwrap();
        assert_eq!(bus.read32(a).unwrap(), 0x1122_AA44);
        assert_eq!(bus.read32(b).unwrap(), 0xAABB_CCDD);
    }

    #[test]
    fn strict_mode_faults_on_unmodeled_register() {
        let mut bus = bus(true);
        assert!(matches!(bus.write32(0x4001_0000, 1), Err(Fault::DAccViol)));
    }

    #[test]
    fn exact_pcr_register_block_supports_clock_reset_cache_rmw() {
        let mut bus = bus(true);
        let regs = [
            PCR_SW_RESET0,
            PCR_SW_RESET1,
            PCR_SW_CLK,
            PCR_SW_RESET2,
            PCR_SW_RESET3,
            PCR_SW_CLK1,
            PCR_APB_CLK,
            PCR_APB_CLK_UPDATE,
            PCR_CACHE_CLOCK_GATE,
            PCR_CACHE_RST,
            PCR_CACHE_BYPASS,
        ];
        for addr in regs {
            let initial = bus.read32(addr).unwrap();
            bus.write32(addr, initial ^ 0x10).unwrap();
            assert_eq!(bus.read32(addr).unwrap(), initial ^ 0x10);
        }
        assert_eq!(DiscoveryBus::storage_reset(PCR_CACHE_BYPASS), Some(0));
    }

    #[test]
    fn gpio_wakeup_bootstrap_is_write_only_and_exact() {
        let mut bus = bus(true);
        bus.write32(WAKEUP_MASK_31_0, 0).unwrap();
        bus.write32(WAKEUP_MASK_34_32, 0).unwrap();
        assert_eq!(bus.sparse_mmio.borrow().get(&WAKEUP_MASK_31_0), Some(&0));
        assert_eq!(bus.sparse_mmio.borrow().get(&WAKEUP_MASK_34_32), Some(&0));
        assert!(matches!(bus.read32(WAKEUP_MASK_31_0), Err(Fault::DAccViol)));
        assert!(matches!(bus.write32(0x4000_F0A8, 0), Err(Fault::DAccViol)));
    }

    #[test]
    fn spif_bootstrap_accepts_only_observed_word_writes() {
        let mut bus = bus(true);
        for (offset, value) in [(0x38, 0xFF01_0005), (0x50, 0), (0x54, 0x10), (0x58, 2)] {
            let addr = SPIF_BASE + offset;
            bus.write32(addr, value).unwrap();
            assert_eq!(bus.sparse_mmio.borrow().get(&addr), Some(&value));
            assert!(matches!(bus.read32(addr), Err(Fault::DAccViol)));
        }
        assert!(matches!(
            bus.write32(SPIF_BASE + 0x3C, 1),
            Err(Fault::DAccViol)
        ));
        assert!(matches!(
            bus.write16(SPIF_BASE + 0x38, 1),
            Err(Fault::DAccViol)
        ));
    }

    #[test]
    fn exact_cache_controller_storage_is_visible_to_strict_mode() {
        let mut bus = bus(true);
        assert_eq!(bus.read32(silicon_regs::AP_CACHE_CTRL0).unwrap(), 0);
        bus.write32(silicon_regs::AP_CACHE_CTRL0, 2).unwrap();
        assert_eq!(bus.read32(silicon_regs::AP_CACHE_CTRL0).unwrap(), 2);
        assert!(matches!(bus.write32(0x4000_C008, 1), Err(Fault::DAccViol)));
    }

    #[test]
    fn aon_bootstrap_and_ram_retention_registers_are_exact_rmw_storage() {
        let mut bus = bus(true);
        for addr in [
            AON_IOCTL0,
            AON_IOCTL1,
            AON_IOCTL2,
            AON_PMCTL0,
            AON_PMCTL1,
            AON_PMCTL2_0,
            AON_PMCTL2_1,
        ] {
            assert_eq!(bus.read32(addr).unwrap(), 0);
            bus.write32(addr, 0x003E_0084).unwrap();
            assert_eq!(bus.read32(addr).unwrap(), 0x003E_0084);
        }
    }

    #[test]
    fn development_secure_profile_and_aes_bridge_work() {
        let mut bus = bus(true);
        assert_eq!(bus.run_finidv().unwrap(), 1);
        assert_eq!(bus.inner.read8(FINIDV_STATUS).unwrap(), 1);
        let key_addr = SRAM_BASE + 0x100;
        let plaintext_addr = SRAM_BASE + 0x120;
        let ciphertext_addr = SRAM_BASE + 0x140;
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let plaintext = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        bus.inner.sram[0x100..0x110].copy_from_slice(&key);
        bus.inner.sram[0x120..0x130].copy_from_slice(&plaintext);
        bus.write32(EMU_AES_KEY_PTR, key_addr).unwrap();
        bus.write32(EMU_AES_PLAINTEXT_PTR, plaintext_addr).unwrap();
        bus.write32(EMU_AES_CIPHERTEXT_PTR, ciphertext_addr)
            .unwrap();
        bus.write32(EMU_AES_TRIGGER, 1).unwrap();
        assert_eq!(
            &bus.inner.sram[0x140..0x150],
            &[
                0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
                0xc5, 0x5a
            ]
        );
    }

    #[test]
    fn explicit_rom_thunks_remain_narrow() {
        let bus = bus(true);
        assert_eq!(bus.read16(0x0000_8AA8).unwrap(), THUMB_BX_LR);
        assert_eq!(bus.read16(0x0000_8C00).unwrap(), THUMB_BX_LR);
        assert_eq!(bus.read16(0x0001_6DC4).unwrap(), THUMB_BX_LR);
        assert!(matches!(bus.read16(0x0000_8C04), Err(Fault::DAccViol)));
        assert_eq!(bus.read16(0x0000_A9C8).unwrap(), THUMB_BX_LR);
        assert_eq!(bus.read16(0x0000_0EB2).unwrap(), 0x2200);
        assert_eq!(bus.read16(0x0001_4CCC).unwrap(), 0xB410);
        assert!(matches!(bus.read16(0x0000_A9CC), Err(Fault::DAccViol)));
    }

    #[test]
    fn power_and_heap_control_cells_track_sdk_contract() {
        let mut bus = bus(true);
        bus.write32(EMU_SLEEP_ALLOWED, 1).unwrap();
        bus.write32(EMU_SLEEP_MODE, 1).unwrap();
        bus.write32(EMU_HEAP_BASE, 0x1FFF_6244).unwrap();
        bus.write32(EMU_HEAP_SIZE, 0xC00).unwrap();
        assert!(bus.sleep_allowed());
        assert_eq!(bus.sleep_mode(), 1);
        assert_eq!(bus.heap_base.get(), 0x1FFF_6244);
        assert_eq!(bus.heap_size.get(), 0xC00);
    }

    #[test]
    fn mirrored_vector_region_is_not_vendor_rom() {
        let mut bus = bus(true);
        assert!(bus.read32(0).is_ok());
        assert!(bus.read32(0x80).is_ok());
    }
}
