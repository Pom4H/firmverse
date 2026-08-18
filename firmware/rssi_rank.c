#include "board.h"

extern uint32_t _estack;
extern uint32_t _sbss;
extern uint32_t _ebss;

void Reset_Handler(void);
int main(void);

__attribute__((section(".vectors"), used))
void * const vectors[2] = {
    &_estack,
    Reset_Handler,
};

void Reset_Handler(void)
{
    uint32_t *p = &_sbss;
    while (p < &_ebss) {
        *p++ = 0;
    }
    (void)main();
    for (;;) {
    }
}

static volatile uint32_t *const gpio_dr = (volatile uint32_t *)(GPIO_BASE + 0x00);
static volatile uint32_t *const gpio_ddr = (volatile uint32_t *)(GPIO_BASE + 0x04);
static volatile uint32_t *const uart0 = (volatile uint32_t *)UART0_BASE;
static volatile uint32_t *const wdt = (volatile uint32_t *)WDT_BASE;
static volatile uint32_t *const pcr = (volatile uint32_t *)PCR_BASE;
static volatile uint32_t *const aon = (volatile uint32_t *)AON_BASE;
/* Inside PHY6252 SRAM, so the same image can run on silicon. The emulator
 * mirrors mailbox writes here from 0x20000000. */
static struct mailbox *const mb = (struct mailbox *)0x1FFF8000u;

#define TRACK_COUNT 12u
#define STALE_MS 4000u
#define COLOR_NONE (-1)

static const uint32_t led_bit[RANK_LED_COUNT] = {
    BIT(PIN_LED_R),
    BIT(PIN_LED_G),
    BIT(PIN_LED_B),
    BIT(PIN_LED_WARM),
    BIT(PIN_LED_X),
};

static const char color_name[RANK_LED_COUNT] = { 'R', 'G', 'B', 'Y', 'W' };

struct Device {
    uint8_t addr[6];
    int8_t rssi;
    int8_t color;
    uint8_t used;
    uint32_t last_ms;
};

static struct Device devices[TRACK_COUNT];
static uint32_t seen_rx;

static void uart_putc(volatile uint32_t *uart, char c)
{
    *uart = (uint32_t)(uint8_t)c;
}

static void uart_puts(volatile uint32_t *uart, const char *s)
{
    while (*s) {
        uart_putc(uart, *s++);
    }
}

static void uart_u8dec(volatile uint32_t *uart, uint32_t value)
{
    uint32_t hundreds = 0;
    uint32_t tens = 0;
    while (value >= 100u) {
        value -= 100u;
        hundreds++;
    }
    while (value >= 10u) {
        value -= 10u;
        tens++;
    }
    if (hundreds != 0u) {
        uart_putc(uart, (char)('0' + hundreds));
    }
    if (hundreds != 0u || tens != 0u) {
        uart_putc(uart, (char)('0' + tens));
    }
    uart_putc(uart, (char)('0' + value));
}

static void uart_rssi(volatile uint32_t *uart, int8_t rssi)
{
    if (rssi < 0) {
        uart_putc(uart, '-');
        uart_u8dec(uart, (uint32_t)(-(int)rssi));
    } else {
        uart_u8dec(uart, (uint32_t)rssi);
    }
}

static void uart_mac(volatile uint32_t *uart, const uint8_t *addr)
{
    static const char hex[] = "0123456789ABCDEF";
    for (uint32_t i = 0; i < 6u; i++) {
        uart_putc(uart, hex[addr[i] >> 4]);
        uart_putc(uart, hex[addr[i] & 0xFu]);
    }
}

static int addr_eq(const uint8_t *a, const uint8_t *b)
{
    for (uint32_t i = 0; i < 6u; i++) {
        if (a[i] != b[i]) {
            return 0;
        }
    }
    return 1;
}

static void copy_addr(uint8_t *dst, const uint8_t *src)
{
    for (uint32_t i = 0; i < 6u; i++) {
        dst[i] = src[i];
    }
}

static void log_color(const uint8_t *addr, int8_t color, int assigned)
{
    if (assigned) {
        uart_puts(uart0, "color ");
        uart_putc(uart0, color_name[(uint8_t)color]);
        uart_puts(uart0, " <- ");
    } else {
        uart_puts(uart0, "vacant ");
        uart_putc(uart0, color_name[(uint8_t)color]);
        uart_putc(uart0, ' ');
    }
    uart_mac(uart0, addr);
    uart_putc(uart0, '\n');
}

static void free_color(struct Device *dev)
{
    if (dev->color >= 0) {
        log_color(dev->addr, dev->color, 0);
        dev->color = COLOR_NONE;
    }
}

static int8_t first_vacant_color(void)
{
    uint8_t taken = 0;
    for (uint32_t i = 0; i < TRACK_COUNT; i++) {
        if (devices[i].used && devices[i].color >= 0) {
            taken = (uint8_t)(taken | (uint8_t)(1u << (uint8_t)devices[i].color));
        }
    }
    for (uint32_t color = 0; color < RANK_LED_COUNT; color++) {
        if ((taken & (uint8_t)(1u << color)) == 0u) {
            return (int8_t)color;
        }
    }
    return COLOR_NONE;
}

static int find_addr(const uint8_t *addr)
{
    for (uint32_t i = 0; i < TRACK_COUNT; i++) {
        if (devices[i].used && addr_eq(devices[i].addr, addr)) {
            return (int)i;
        }
    }
    return -1;
}

static int alloc_slot(void)
{
    int weakest = -1;
    int8_t weakest_rssi = 127;
    for (uint32_t i = 0; i < TRACK_COUNT; i++) {
        if (!devices[i].used) {
            return (int)i;
        }
        if (devices[i].color < 0 && devices[i].rssi <= weakest_rssi) {
            weakest_rssi = devices[i].rssi;
            weakest = (int)i;
        }
    }
    if (weakest >= 0) {
        devices[weakest].used = 0;
        return weakest;
    }
    return -1;
}

static void drop_device(struct Device *dev)
{
    free_color(dev);
    dev->used = 0;
}

static void apply_report(const uint8_t *pkt, uint32_t tick)
{
    uint8_t flags = pkt[1];
    const uint8_t *addr = &pkt[2];
    int8_t rssi = (int8_t)pkt[8];
    int slot = find_addr(addr);

    if (flags == SCAN_PKT_GONE) {
        if (slot >= 0) {
            uart_puts(uart0, "gone ");
            uart_mac(uart0, addr);
            uart_putc(uart0, '\n');
            drop_device(&devices[slot]);
        }
        return;
    }

    if (slot < 0) {
        slot = alloc_slot();
        if (slot < 0) {
            return;
        }
        copy_addr(devices[slot].addr, addr);
        devices[slot].color = COLOR_NONE;
        devices[slot].used = 1;
    }
    devices[slot].rssi = rssi;
    devices[slot].last_ms = tick;
    uart_puts(uart0, "scan ");
    uart_mac(uart0, addr);
    uart_putc(uart0, ' ');
    uart_rssi(uart0, rssi);
    uart_putc(uart0, '\n');
}

static uint32_t blink_half_ms(int8_t rssi)
{
    int32_t depth;
    if (rssi >= -35) {
        return 40u;
    }
    if (rssi <= -90) {
        return 800u;
    }
    depth = (-35) - (int32_t)rssi;
    return 40u + (uint32_t)depth * 14u;
}

static uint32_t udiv32(uint32_t n, uint32_t d)
{
    uint32_t q = 0;
    uint32_t r = 0;
    uint32_t i = 32u;
    if (d == 0u) {
        return 0u;
    }
    while (i > 0u) {
        i--;
        r = (r << 1) | ((n >> i) & 1u);
        if (r >= d) {
            r -= d;
            q |= (1u << i);
        }
    }
    return q;
}

static int blink_lit(uint32_t tick, int8_t rssi)
{
    uint32_t half = blink_half_ms(rssi);
    return (udiv32(tick, half) & 1u) == 0u;
}

static int stronger(const struct Device *a, const struct Device *b)
{
    if (a->rssi != b->rssi) {
        return a->rssi > b->rssi;
    }
    for (uint32_t i = 0; i < 6u; i++) {
        if (a->addr[i] != b->addr[i]) {
            return a->addr[i] < b->addr[i];
        }
    }
    return 0;
}

static void rank_and_light(uint32_t tick)
{
    uint32_t live[TRACK_COUNT];
    uint32_t n = 0;
    uint32_t gpio = 0;

    for (uint32_t i = 0; i < TRACK_COUNT; i++) {
        if (!devices[i].used) {
            continue;
        }
        if ((tick - devices[i].last_ms) >= STALE_MS) {
            uart_puts(uart0, "stale ");
            uart_mac(uart0, devices[i].addr);
            uart_putc(uart0, '\n');
            drop_device(&devices[i]);
            continue;
        }
        live[n++] = i;
    }

    for (uint32_t i = 1; i < n; i++) {
        uint32_t key = live[i];
        uint32_t j = i;
        while (j > 0u && stronger(&devices[key], &devices[live[j - 1u]])) {
            live[j] = live[j - 1u];
            j--;
        }
        live[j] = key;
    }

    for (uint32_t i = RANK_LED_COUNT; i < n; i++) {
        free_color(&devices[live[i]]);
    }

    uint32_t top = n;
    if (top > RANK_LED_COUNT) {
        top = RANK_LED_COUNT;
    }
    for (uint32_t i = 0; i < top; i++) {
        struct Device *dev = &devices[live[i]];
        if (dev->color < 0) {
            int8_t color = first_vacant_color();
            if (color >= 0) {
                dev->color = color;
                log_color(dev->addr, color, 1);
            }
        }
        if (dev->color >= 0 && blink_lit(tick, dev->rssi)) {
            gpio |= led_bit[(uint8_t)dev->color];
        }
    }

    *gpio_dr = gpio;
}

static int take_rx(uint32_t tick)
{
    int got = 0;
    if (mb->magic != MAGIC_PHY2 || mb->rx_seq == seen_rx) {
        return 0;
    }
    seen_rx = mb->rx_seq;
    uint32_t len = mb->rx_len;
    uint32_t off = 0;
    while (off + SCAN_PKT_BYTES <= len && off + SCAN_PKT_BYTES <= 256u) {
        if (mb->rx[off] == SCAN_PKT_MAGIC) {
            uint8_t pkt[SCAN_PKT_BYTES];
            for (uint32_t i = 0; i < SCAN_PKT_BYTES; i++) {
                pkt[i] = mb->rx[off + i];
            }
            apply_report(pkt, tick);
            got = 1;
        }
        off += SCAN_PKT_BYTES;
    }
    return got;
}

int main(void)
{
    uint32_t last_rank = 0;

    *gpio_ddr = RANK_LED_MASK;
    *gpio_dr = 0;
    *pcr = 0x1;
    *aon = 0x1;
    *wdt = 0xAAAA;

    uart_puts(uart0, "rssi-rank boot\n");
    uart_puts(uart0, "leds R G B Y W\n");

    if (mb->magic != MAGIC_PHY2) {
        mb->status = 0;
        mb->rx_seq = 0;
        mb->rx_len = 0;
        mb->tx_seq = 0;
        mb->tx_len = 0;
        mb->tick_ms = 0;
        mb->magic = MAGIC_PHY2;
    }

    for (;;) {
        uint32_t tick = mb->tick_ms;
        *wdt = 0x5555;
        int got = take_rx(tick);
        if (got || (tick - last_rank) >= 20u) {
            last_rank = tick;
            rank_and_light(tick);
        }
    }
}
