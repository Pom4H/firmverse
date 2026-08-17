#ifndef KIT_BOARD_H
#define KIT_BOARD_H

#include <stdint.h>

/* PHY6252 gpio_pin_e bit in AP_GPIO->swporta_dr — not the silkscreen number. */
#define GPIO_BIT_P0  0u
#define GPIO_BIT_P2  2u
#define GPIO_BIT_P3  3u
#define GPIO_BIT_P7  4u
#define GPIO_BIT_P11 7u
#define GPIO_BIT_P14 8u
#define GPIO_BIT_P15 9u
#define GPIO_BIT_P16 10u
#define GPIO_BIT_P17 11u
#define GPIO_BIT_P18 12u
#define GPIO_BIT_P20 13u
#define GPIO_BIT_P23 14u
#define GPIO_BIT_P24 15u
#define GPIO_BIT_P31 19u
#define GPIO_BIT_P32 20u
#define GPIO_BIT_P33 21u
#define GPIO_BIT_P34 22u

#define BIT(n) (1u << (n))

/* AI-Thinker PB-03F-Kit A148 — DIP-30 silkscreen. */
#define PIN_LED_R GPIO_BIT_P7
#define PIN_LED_G GPIO_BIT_P11
#define PIN_LED_B GPIO_BIT_P18
#define PIN_LED_WARM GPIO_BIT_P0
#define PIN_BTN GPIO_BIT_P34

#define PIN_HDR_P14 GPIO_BIT_P14
#define PIN_HDR_P16 GPIO_BIT_P16
#define PIN_HDR_P17 GPIO_BIT_P17
#define PIN_HDR_P31 GPIO_BIT_P31
#define PIN_HDR_P32 GPIO_BIT_P32
#define PIN_HDR_P33 GPIO_BIT_P33

#define RGB_MASK (BIT(PIN_LED_R) | BIT(PIN_LED_G) | BIT(PIN_LED_B) | BIT(PIN_LED_WARM))
#define HDR_MASK (BIT(PIN_HDR_P14) | BIT(PIN_HDR_P16) | BIT(PIN_HDR_P17) | \
                  BIT(PIN_HDR_P31) | BIT(PIN_HDR_P32) | BIT(PIN_HDR_P33))
#define OUT_MASK (RGB_MASK | HDR_MASK)

#define GPIO_BASE  0x40008000u
#define UART0_BASE 0x40004000u
#define UART1_BASE 0x40009000u
#define TIM_BASE   0x40001004u
#define WDT_BASE   0x40002000u
#define I2C0_BASE  0x40005000u
#define SPI0_BASE  0x40006000u
#define PWM_BASE   0x4000E000u
#define ADC_CH_BASE 0x40050400u
#define PCR_BASE   0x40000000u
#define AON_BASE   0x4000F000u

/* ADC channel index at ADC_CH_BASE + ch*4 (silicon enum). */
#define ADC_CH_P23 3u
#define ADC_CH_P24 4u
#define ADC_CH_P15 6u
#define ADC_CH_P20 7u

#define MAILBOX_BASE 0x20000000u

struct mailbox {
    volatile uint32_t magic;
    volatile uint32_t status;
    volatile uint32_t rx_seq;
    volatile uint32_t rx_len;
    volatile uint8_t rx[256];
    volatile uint32_t tx_seq;
    volatile uint32_t tx_len;
    volatile uint8_t tx[256];
    volatile uint32_t tick_ms;
} __attribute__((packed));

#define STATUS_CONNECTED 1u
#define STATUS_NOTIFY 2u
#define MAGIC_PHY2 0x50485932u

#endif
