#include "board.h"

extern uint32_t _estack;
extern uint32_t _sbss;
extern uint32_t _ebss;

void Reset_Handler(void);
int main(void);
int test_controller_abi(void);

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

/* Public PHY6252 ROM ABI entrypoints, Thumb bit included. */
typedef void (*osal_mem_set_heap_fn)(void *, uint32_t);
typedef void *(*osal_mem_alloc_fn)(uint16_t);
typedef void (*osal_mem_free_fn)(void *);
typedef void *(*osal_memset_fn)(void *, uint8_t, int);
typedef void *(*osal_memcpy_fn)(void *, const void *, unsigned int);
typedef uint16_t (*osal_rand_fn)(void);
typedef uint8_t *(*osal_msg_allocate_fn)(uint16_t);
typedef uint8_t (*osal_msg_deallocate_fn)(uint8_t *);
typedef void (*osal_msg_enqueue_fn)(void **, void *);
typedef uint8_t (*osal_msg_enqueue_max_fn)(void **, void *, uint8_t);
typedef void *(*osal_msg_dequeue_fn)(void **);
typedef void (*osal_msg_push_fn)(void **, void *);
typedef void (*osal_msg_extract_fn)(void **, void *, void *);
typedef int (*spif_write_fn)(uint32_t, uint8_t *, uint32_t);
typedef int (*spif_erase_sector_fn)(uint32_t);
typedef void (*ll_aes_fn)(uint8_t *, uint8_t *, uint8_t *);

#define ROM_OSAL_MEM_SET_HEAP   ((osal_mem_set_heap_fn)0x00014CB5u)
#define ROM_OSAL_MEM_ALLOC      ((osal_mem_alloc_fn)0x00014B3Du)
#define ROM_OSAL_MEM_FREE       ((osal_mem_free_fn)0x00014C01u)
#define ROM_OSAL_MEMSET         ((osal_memset_fn)0x00014D15u)
#define ROM_OSAL_MEMCPY         ((osal_memcpy_fn)0x00014CE9u)
#define ROM_OSAL_RAND           ((osal_rand_fn)0x00015129u)
#define ROM_OSAL_MSG_ALLOC      ((osal_msg_allocate_fn)0x00014D1Du)
#define ROM_OSAL_MSG_DEALLOC    ((osal_msg_deallocate_fn)0x00014D43u)
#define ROM_OSAL_MSG_DEQUEUE    ((osal_msg_dequeue_fn)0x00014D65u)
#define ROM_OSAL_MSG_ENQUEUE    ((osal_msg_enqueue_fn)0x00014D91u)
#define ROM_OSAL_MSG_ENQ_MAX    ((osal_msg_enqueue_max_fn)0x00014DC3u)
#define ROM_OSAL_MSG_EXTRACT    ((osal_msg_extract_fn)0x00014E6Du)
#define ROM_OSAL_MSG_PUSH       ((osal_msg_push_fn)0x00014ED1u)
#define ROM_SPIF_ERASE_SECTOR   ((spif_erase_sector_fn)0x00016FA9u)
#define ROM_SPIF_WRITE          ((spif_write_fn)0x00017395u)
#define ROM_SPIF_WRITE_DMA      ((spif_write_fn)0x0001744Du)
#define ROM_LL_AES128           ((ll_aes_fn)0x00003FC5u)

#define XIP_BASE 0x11000000u
#define TEST_FLASH_OFF 0x0003F000u
#define TEST_FLASH_ADDR (XIP_BASE + TEST_FLASH_OFF)

static volatile uint32_t *const gpio_dr = (volatile uint32_t *)(GPIO_BASE + 0x00);
static volatile uint32_t *const gpio_ddr = (volatile uint32_t *)(GPIO_BASE + 0x04);
static volatile uint32_t *const gpio_ext = (volatile uint32_t *)(GPIO_BASE + 0x50);
static volatile uint32_t *const uart0 = (volatile uint32_t *)UART0_BASE;
static volatile uint32_t *const uart1 = (volatile uint32_t *)UART1_BASE;
static volatile uint32_t *const wdt = (volatile uint32_t *)WDT_BASE;
static volatile uint32_t *const i2c0 = (volatile uint32_t *)I2C0_BASE;
static volatile uint32_t *const spi0 = (volatile uint32_t *)SPI0_BASE;
static volatile uint32_t *const pcr = (volatile uint32_t *)PCR_BASE;
static volatile uint32_t *const aon = (volatile uint32_t *)AON_BASE;
static struct mailbox *const mb = (struct mailbox *)MAILBOX_BASE;

static volatile uint32_t *const timers[6] = {
    (volatile uint32_t *)0x40001004u,
    (volatile uint32_t *)0x40001018u,
    (volatile uint32_t *)0x4000102Cu,
    (volatile uint32_t *)0x40001040u,
    (volatile uint32_t *)0x40001054u,
    (volatile uint32_t *)0x40001068u,
};

static uint32_t osal_heap[512];

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

static void uart_hex8(volatile uint32_t *uart, uint32_t value)
{
    static const char hex[] = "0123456789abcdef";
    for (int i = 7; i >= 0; i--) {
        uart_putc(uart, hex[(value >> ((uint32_t)i * 4u)) & 0xFu]);
    }
}

static void uart_bool(volatile uint32_t *uart, int value)
{
    uart_putc(uart, value ? '1' : '0');
}

static uint32_t adc_ch(uint32_t ch)
{
    return *(volatile uint32_t *)(ADC_CH_BASE + ch * 4u);
}

static void pwm_duty(uint32_t ch, uint32_t duty)
{
    *(volatile uint32_t *)(PWM_BASE + ch * 16u + 8u) = duty;
}

static int bytes_equal(const uint8_t *a, const uint8_t *b, uint32_t len)
{
    for (uint32_t i = 0; i < len; i++) {
        if (a[i] != b[i]) {
            return 0;
        }
    }
    return 1;
}

static void radio_send(const uint8_t *data, uint32_t n)
{
    if (n > 256u) {
        n = 256u;
    }
    mb->tx_len = n;
    for (uint32_t i = 0; i < n; i++) {
        mb->tx[i] = data[i];
    }
    mb->tx_seq = mb->tx_seq + 1u;
}

static int test_osal_memory(void)
{
    ROM_OSAL_MEM_SET_HEAP(osal_heap, sizeof(osal_heap));
    uint8_t *a = (uint8_t *)ROM_OSAL_MEM_ALLOC(32u);
    uint8_t *b = (uint8_t *)ROM_OSAL_MEM_ALLOC(32u);
    if (a == 0 || b == 0) {
        return 0;
    }
    ROM_OSAL_MEMSET(a, 0xA5u, 32);
    ROM_OSAL_MEMSET(b, 0x00u, 32);
    ROM_OSAL_MEMCPY(b, a, 32u);
    int ok = bytes_equal(a, b, 32u) && ROM_OSAL_RAND() != 0u;
    ROM_OSAL_MEM_FREE(b);
    ROM_OSAL_MEM_FREE(a);
    return ok;
}

static int test_osal_queue(void)
{
    void *q = 0;
    uint8_t *a = ROM_OSAL_MSG_ALLOC(8u);
    uint8_t *b = ROM_OSAL_MSG_ALLOC(8u);
    uint8_t *c = ROM_OSAL_MSG_ALLOC(8u);
    if (a == 0 || b == 0 || c == 0) {
        return 0;
    }

    ROM_OSAL_MSG_ENQUEUE(&q, a);
    ROM_OSAL_MSG_ENQUEUE(&q, b);
    ROM_OSAL_MSG_PUSH(&q, c);
    int ok = ROM_OSAL_MSG_DEQUEUE(&q) == c;
    ok = ok && ROM_OSAL_MSG_DEQUEUE(&q) == a;
    ok = ok && ROM_OSAL_MSG_ENQ_MAX(&q, a, 1u) == 0u;
    ROM_OSAL_MSG_EXTRACT(&q, b, 0);
    ok = ok && q == 0;
    ok = ok && ROM_OSAL_MSG_ENQ_MAX(&q, a, 1u) != 0u;
    ok = ok && ROM_OSAL_MSG_DEQUEUE(&q) == a;

    ok = ok && ROM_OSAL_MSG_DEALLOC(c) == 0u;
    ok = ok && ROM_OSAL_MSG_DEALLOC(b) == 0u;
    ok = ok && ROM_OSAL_MSG_DEALLOC(a) == 0u;
    return ok;
}

static int test_flash(void)
{
    uint8_t first[8] = {0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10};
    uint8_t second[8] = {0xF0, 0x0F, 0xAA, 0x55, 0xC3, 0x3C, 0x5A, 0xA5};
    volatile uint8_t *xip = (volatile uint8_t *)TEST_FLASH_ADDR;
    if (ROM_SPIF_ERASE_SECTOR(TEST_FLASH_OFF) != 0) {
        uart_puts(uart0, "flash erase failed\n");
        return 0;
    }
    if (ROM_SPIF_WRITE(TEST_FLASH_OFF, first, sizeof(first)) != 0) {
        uart_puts(uart0, "flash pio failed\n");
        return 0;
    }
    if (ROM_SPIF_WRITE_DMA(TEST_FLASH_OFF + 8u, second, sizeof(second)) != 0) {
        uart_puts(uart0, "flash dma failed\n");
        return 0;
    }
    for (uint32_t i = 0; i < 8u; i++) {
        if (xip[i] != first[i] || xip[8u + i] != second[i]) {
            uart_puts(uart0, "flash mismatch i=");
            uart_hex8(uart0, i);
            uart_puts(uart0, " first=");
            uart_hex8(uart0, first[i]);
            uart_putc(uart0, '/');
            uart_hex8(uart0, xip[i]);
            uart_puts(uart0, " second=");
            uart_hex8(uart0, second[i]);
            uart_putc(uart0, '/');
            uart_hex8(uart0, xip[8u + i]);
            uart_putc(uart0, '\n');
            return 0;
        }
    }
    return 1;
}

static int test_aes(void)
{
    uint8_t key[16] = {
        0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,
        0x08,0x09,0x0A,0x0B,0x0C,0x0D,0x0E,0x0F
    };
    uint8_t plain[16] = {
        0x00,0x11,0x22,0x33,0x44,0x55,0x66,0x77,
        0x88,0x99,0xAA,0xBB,0xCC,0xDD,0xEE,0xFF
    };
    static const uint8_t expected[16] = {
        0x69,0xC4,0xE0,0xD8,0x6A,0x7B,0x04,0x30,
        0xD8,0xCD,0xB7,0x80,0x70,0xB4,0xC5,0x5A
    };
    uint8_t out[16];
    ROM_LL_AES128(key, plain, out);
    return bytes_equal(out, expected, 16u);
}

int main(void)
{
    uint32_t seen_rx = 0;
    uint32_t last_log = 0;
    uint32_t phase = 0;
    uint32_t timer_mix = 0;

    *gpio_ddr = OUT_MASK;
    *gpio_dr = 0;
    *pcr = 0x1u;
    *aon = 0x1u;
    *i2c0 = 0xA5u;
    *spi0 = 0x5Au;
    *wdt = 0xAAAAu;

    uart_puts(uart0, "capability-demo boot\n");
    uart_puts(uart1, "capability-demo uart1\n");

    int osal_ok = test_osal_memory();
    int queue_ok = test_osal_queue();
    int flash_ok = test_flash();
    int aes_ok = test_aes();
    int ctrl_ok = test_controller_abi();

    uart_puts(uart0, "selftest osal=");
    uart_bool(uart0, osal_ok);
    uart_puts(uart0, " queue=");
    uart_bool(uart0, queue_ok);
    uart_puts(uart0, " flash=");
    uart_bool(uart0, flash_ok);
    uart_puts(uart0, " aes=");
    uart_bool(uart0, aes_ok);
    uart_puts(uart0, " ctrl=");
    uart_bool(uart0, ctrl_ok);
    uart_putc(uart0, '\n');

    if (mb->magic != MAGIC_PHY2) {
        mb->magic = MAGIC_PHY2;
    }

    for (;;) {
        uint32_t tick = mb->tick_ms;
        *wdt = 0x5555u;

        for (uint32_t i = 0; i < 6u; i++) {
            timer_mix ^= *timers[i];
        }

        uint32_t p20 = adc_ch(ADC_CH_P20);
        uint32_t p15 = adc_ch(ADC_CH_P15);
        uint32_t p24 = adc_ch(ADC_CH_P24);
        uint32_t p23 = adc_ch(ADC_CH_P23);
        int btn = (*gpio_ext & BIT(PIN_BTN)) != 0;
        int linked = (mb->status & STATUS_CONNECTED) != 0;

        phase = (tick >> 5) & 7u;
        if (phase >= 6u) {
            phase -= 6u;
        }
        static const uint32_t walk[6] = {
            BIT(PIN_HDR_P14), BIT(PIN_HDR_P16), BIT(PIN_HDR_P17),
            BIT(PIN_HDR_P31), BIT(PIN_HDR_P32), BIT(PIN_HDR_P33),
        };
        uint32_t rgb = linked ? BIT(PIN_LED_G) : BIT(PIN_LED_B);
        if (btn) {
            rgb = RGB_MASK;
        }
        *gpio_dr = rgb | walk[phase];

        pwm_duty(0, tick & 0xFFu);
        pwm_duty(1, p20 & 0xFFu);
        pwm_duty(2, p15 & 0xFFu);
        pwm_duty(3, p24 & 0xFFu);
        pwm_duty(4, p23 & 0xFFu);
        pwm_duty(5, timer_mix & 0xFFu);

        if ((tick - last_log) >= 250u) {
            last_log = tick;
            uart_puts(uart0, "cap t=");
            uart_hex8(uart0, tick);
            uart_puts(uart0, " gpio=");
            uart_hex8(uart0, *gpio_dr);
            uart_puts(uart0, " adc=");
            uart_hex8(uart0, p20);
            uart_putc(uart0, ',');
            uart_hex8(uart0, p15);
            uart_putc(uart0, ',');
            uart_hex8(uart0, p24);
            uart_putc(uart0, ',');
            uart_hex8(uart0, p23);
            uart_putc(uart0, '\n');
        }

        if (mb->rx_seq != seen_rx) {
            seen_rx = mb->rx_seq;
            if (mb->rx_len > 0u && mb->rx_len <= 256u) {
                radio_send((const uint8_t *)mb->rx, mb->rx_len);
            }
        } else if ((mb->status & STATUS_NOTIFY) != 0u && (tick & 0x7Fu) == 0u) {
            uint8_t snap[16];
            snap[0] = 0xC2u;
            snap[1] = (uint8_t)phase;
            snap[2] = (uint8_t)(*gpio_dr);
            snap[3] = (uint8_t)(*gpio_dr >> 8);
            snap[4] = (uint8_t)p20;
            snap[5] = (uint8_t)p15;
            snap[6] = (uint8_t)p24;
            snap[7] = (uint8_t)p23;
            snap[8] = (uint8_t)osal_ok;
            snap[9] = (uint8_t)queue_ok;
            snap[10] = (uint8_t)flash_ok;
            snap[11] = (uint8_t)aes_ok;
            snap[12] = (uint8_t)ctrl_ok;
            snap[13] = (uint8_t)(timer_mix >> 8);
            snap[14] = btn ? 1u : 0u;
            snap[15] = (uint8_t)mb->status;
            radio_send(snap, sizeof(snap));
        }

        __asm volatile("cpsid i");
        __asm volatile("cpsie i");
        __asm volatile("wfi");
    }
}
