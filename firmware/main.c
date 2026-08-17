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
static volatile uint32_t *const gpio_ext = (volatile uint32_t *)(GPIO_BASE + 0x50);
static volatile uint32_t *const uart0 = (volatile uint32_t *)UART0_BASE;
static volatile uint32_t *const uart1 = (volatile uint32_t *)UART1_BASE;
static volatile uint32_t *const tim = (volatile uint32_t *)TIM_BASE;
static volatile uint32_t *const wdt = (volatile uint32_t *)WDT_BASE;
static volatile uint32_t *const i2c0 = (volatile uint32_t *)I2C0_BASE;
static volatile uint32_t *const spi0 = (volatile uint32_t *)SPI0_BASE;
static volatile uint32_t *const pcr = (volatile uint32_t *)PCR_BASE;
static volatile uint32_t *const aon = (volatile uint32_t *)AON_BASE;
static struct mailbox *const mb = (struct mailbox *)MAILBOX_BASE;

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
        uart_putc(uart, hex[(value >> (i * 4)) & 0xF]);
    }
}

static void uart_u16(volatile uint32_t *uart, uint32_t value)
{
    uart_hex8(uart, value);
}

static uint32_t adc_ch(uint32_t ch)
{
    return *(volatile uint32_t *)(ADC_CH_BASE + ch * 4u);
}

static void pwm_duty(uint32_t ch, uint32_t duty)
{
    *(volatile uint32_t *)(PWM_BASE + ch * 16u + 8u) = duty;
}

static void radio_send(const uint8_t *data, uint32_t n)
{
    if (n > 256) {
        n = 256;
    }
    mb->tx_len = n;
    for (uint32_t i = 0; i < n; i++) {
        mb->tx[i] = data[i];
    }
    mb->tx_seq = mb->tx_seq + 1u;
}

int main(void)
{
    uint32_t seen_rx = 0;
    uint32_t last_log = 0;
    uint32_t phase = 0;
    uint32_t last_link = 0;

    *gpio_ddr = OUT_MASK;
    *gpio_dr = 0;
    *pcr = 0x1;
    *aon = 0x1;
    *i2c0 = 0xA5;
    *spi0 = 0x5A;
    *wdt = 0xAAAA;

    uart_puts(uart0, "kit-demo boot\n");
    uart_puts(uart0, "ble adv PB03FKIT\n");
    uart_puts(uart1, "uart1\n");

    if (mb->magic != MAGIC_PHY2) {
        mb->magic = MAGIC_PHY2;
    }

    for (;;) {
        uint32_t tick = mb->tick_ms;
        uint32_t tmr = *tim;
        (void)tmr;
        *wdt = 0x5555;

        int linked = (mb->status & STATUS_CONNECTED) != 0;
        if ((uint32_t)linked != last_link) {
            last_link = (uint32_t)linked;
            uart_puts(uart0, linked ? "ble up\n" : "ble down\n");
        }

        int btn = (*gpio_ext & BIT(PIN_BTN)) != 0;

        uint32_t rgb = 0;
        if (linked) {
            rgb = BIT(PIN_LED_G);
        } else {
            switch ((tick >> 6) & 3u) {
            case 0:
                rgb = BIT(PIN_LED_R);
                break;
            case 1:
                rgb = BIT(PIN_LED_G);
                break;
            case 2:
                rgb = BIT(PIN_LED_B);
                break;
            default:
                rgb = BIT(PIN_LED_WARM);
                break;
            }
        }
        if (btn) {
            rgb = RGB_MASK;
        }

        static const uint32_t walk[] = {
            BIT(PIN_HDR_P14),
            BIT(PIN_HDR_P16),
            BIT(PIN_HDR_P17),
            BIT(PIN_HDR_P31),
            BIT(PIN_HDR_P32),
            BIT(PIN_HDR_P33),
        };
        phase = (tick >> 5) & 7u;
        if (phase >= 6u) {
            phase -= 6u;
        }
        *gpio_dr = rgb | walk[phase];

        uint32_t p20 = adc_ch(ADC_CH_P20);
        uint32_t p15 = adc_ch(ADC_CH_P15);
        uint32_t p24 = adc_ch(ADC_CH_P24);
        uint32_t p23 = adc_ch(ADC_CH_P23);

        pwm_duty(0, (tick * 3u) & 0xFFu);
        pwm_duty(1, p20 & 0xFFu);
        pwm_duty(2, p15 & 0xFFu);

        if ((tick - last_log) >= 200u) {
            last_log = tick;
            uart_puts(uart0, "t=");
            uart_u16(uart0, tick);
            uart_puts(uart0, " gpio=");
            uart_hex8(uart0, *gpio_dr);
            uart_puts(uart0, " btn=");
            uart_putc(uart0, btn ? '1' : '0');
            uart_puts(uart0, " adc=");
            uart_u16(uart0, p20);
            uart_putc(uart0, ',');
            uart_u16(uart0, p15);
            uart_putc(uart0, ',');
            uart_u16(uart0, p24);
            uart_putc(uart0, ',');
            uart_u16(uart0, p23);
            uart_putc(uart0, '\n');
        }

        if (mb->rx_seq != seen_rx) {
            seen_rx = mb->rx_seq;
            uart_puts(uart0, "rx ");
            uart_u16(uart0, mb->rx_len);
            uart_putc(uart0, '\n');
            if (mb->rx_len > 0 && mb->rx_len <= 256) {
                radio_send((const uint8_t *)mb->rx, mb->rx_len);
            }
        } else if ((mb->status & STATUS_NOTIFY) != 0 && (tick & 0x7Fu) == 0) {
            uint8_t snap[12];
            snap[0] = 0xA1;
            snap[1] = (uint8_t)phase;
            snap[2] = (uint8_t)(*gpio_dr);
            snap[3] = (uint8_t)(*gpio_dr >> 8);
            snap[4] = (uint8_t)(*gpio_dr >> 16);
            snap[5] = (uint8_t)(*gpio_dr >> 24);
            snap[6] = (uint8_t)(p20 >> 8);
            snap[7] = (uint8_t)p20;
            snap[8] = (uint8_t)(p23 >> 8);
            snap[9] = (uint8_t)p23;
            snap[10] = btn ? 1 : 0;
            snap[11] = (uint8_t)(mb->status);
            radio_send(snap, 12);
        }
    }
}
