/* Runs after SRAM copy, before SDK main. */
extern int __real_main(void);
extern void ll_patch_master(void);

void __wrap_ll_patch_slave(void)
{
    ll_patch_master();
}

void __wrap_ll_patch_advscan(void)
{
    ll_patch_master();
}

void __wrap_dbg_printf_init(void)
{
}

void __wrap_dbg_printf(const char *fmt, ...)
{
    (void)fmt;
}

int __wrap_watchdog_config(unsigned char cycle)
{
    (void)cycle;
    return 0;
}

static void uart_boot_put(unsigned char c)
{
    volatile unsigned int *thr = (volatile unsigned int *)0x40004000u;
    volatile unsigned int spin;
    *thr = (unsigned int)c;
    spin = 8000u;
    while (spin > 0u) {
        spin--;
    }
}

static void uart_boot_init(void)
{
    volatile unsigned int *clk = (volatile unsigned int *)0x40000008u;
    volatile unsigned int *fmux = (volatile unsigned int *)0x4000380Cu;
    volatile unsigned int *sel1 = (volatile unsigned int *)0x4000381Cu;
    volatile unsigned char *uart = (volatile unsigned char *)0x40004000u;
    unsigned int sel;

    *clk |= 0x100u;
    *fmux |= (1u << 5) | (1u << 6);
    sel = *sel1;
    sel &= ~((0x3Fu << 8) | (0x3Fu << 16));
    sel |= (4u << 8) | (5u << 16);
    *sel1 = sel;
    uart[0x0C] = 0x80;
    uart[0x00] = 9;
    uart[0x04] = 0;
    uart[0x0C] = 0x03;
    uart[0x04] = 0;
}

void silicon_uart_put(unsigned char c)
{
    uart_boot_put(c);
}

void silicon_uart_reinit(void)
{
    uart_boot_init();
}

void silicon_stage(unsigned int mask)
{
    volatile unsigned int *dr = (volatile unsigned int *)0x40008000u;
    *dr = mask;
}

int __wrap_main(void)
{
    uart_boot_init();
    uart_boot_put('B');
    uart_boot_put('\r');
    uart_boot_put('\n');
    silicon_stage(0x10u);
    return __real_main();
}
