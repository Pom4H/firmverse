#include "board.h"

typedef uint16_t (*process_event_fn)(uint8_t, uint16_t);
typedef void (*reverse_bytes_fn)(uint8_t *, uint8_t);
typedef uint8_t (*ll_reset_fn)(void);
typedef uint8_t (*ll_set_adv_param_fn)(uint16_t, uint16_t, uint8_t, uint8_t, uint8_t, uint8_t *, uint8_t, uint8_t);
typedef uint8_t (*ll_set_data_length_fn)(uint16_t, uint16_t, uint16_t);
typedef int (*spif_erase_sector_fn)(uint32_t);

#define ROM_HCI_PROCESS_EVENT ((process_event_fn)0x000024FDu)
#define ROM_HCI_REVERSE_BYTES ((reverse_bytes_fn)0x000026A9u)
#define ROM_LL_PROCESS_EVENT ((process_event_fn)0x000059F1u)
#define ROM_LL_RESET0 ((ll_reset_fn)0x00006609u)
#define ROM_LL_SET_ADV_PARAM0 ((ll_set_adv_param_fn)0x00006A9Du)
#define ROM_LL_SET_DATA_LENGTH0 ((ll_set_data_length_fn)0x00006E11u)
#define ROM_SPIF_ERASE_SECTOR ((spif_erase_sector_fn)0x00016FA9u)

#define LL_STATUS_SUCCESS 0x00u
#define LL_STATUS_INACTIVE_CONNECTION 0x02u

#define DMAC_BASE 0x40010000u
#define DMAC_CH_STRIDE 0x58u
#define DMAC_RAW_TFR (*(volatile uint32_t *)(DMAC_BASE + 0x2C0u))
#define DMAC_CLEAR_TFR (*(volatile uint32_t *)(DMAC_BASE + 0x338u))
#define DMAC_CFG (*(volatile uint32_t *)(DMAC_BASE + 0x398u))
#define DMAC_CH_EN (*(volatile uint32_t *)(DMAC_BASE + 0x3A0u))
#define DMAC_CH_REG(ch, off) (*(volatile uint32_t *)(DMAC_BASE + (uint32_t)(ch) * DMAC_CH_STRIDE + (off)))
#define DMA_SAR 0x00u
#define DMA_DAR 0x08u
#define DMA_LLP 0x10u
#define DMA_CTL 0x18u
#define DMA_CTL_H 0x1Cu
#define DMA_CFG_LO 0x40u
#define DMA_CFG_HI 0x44u

#define DMA_TYPE_M2M (0u << 20)
#define DMA_TYPE_M2P (1u << 20)
#define DMA_WIDTH_BYTE_SRC (0u << 4)
#define DMA_WIDTH_BYTE_DST (0u << 1)
#define DMA_WIDTH_WORD_SRC (2u << 4)
#define DMA_WIDTH_WORD_DST (2u << 1)
#define DMA_SRC_INC (0u << 9)
#define DMA_DST_INC (0u << 7)
#define DMA_DST_NO_CHANGE (2u << 7)

#define DMA_FLASH_OFF 0x0003E000u
#define DMA_FLASH_ADDR (0x11000000u + DMA_FLASH_OFF)

static void uart0_puts(const char *s)
{
    volatile uint32_t *uart = (volatile uint32_t *)UART0_BASE;
    while (*s) {
        *uart = (uint32_t)(uint8_t)*s++;
    }
}

static int bytes_equal_local(const uint8_t *a, const uint8_t *b, uint32_t len)
{
    for (uint32_t i = 0; i < len; i++) {
        if (a[i] != b[i]) {
            return 0;
        }
    }
    return 1;
}

static int dma_run(uint32_t ch, uint32_t src, uint32_t dst, uint32_t ctl, uint32_t transfers)
{
    uint32_t bit = 1u << ch;
    DMAC_CLEAR_TFR = bit;
    DMAC_CH_REG(ch, DMA_SAR) = src;
    DMAC_CH_REG(ch, DMA_DAR) = dst;
    DMAC_CH_REG(ch, DMA_LLP) = 0u;
    DMAC_CH_REG(ch, DMA_CTL) = ctl;
    DMAC_CH_REG(ch, DMA_CTL_H) = transfers;
    DMAC_CH_REG(ch, DMA_CFG_LO) = 0u;
    DMAC_CH_REG(ch, DMA_CFG_HI) = 0u;
    DMAC_CH_EN = (1u << (8u + ch)) | bit;

    for (uint32_t guard = 0; guard < 10000u; guard++) {
        if ((DMAC_RAW_TFR & bit) != 0u) {
            DMAC_CLEAR_TFR = bit;
            return 1;
        }
    }
    return 0;
}

static int test_dma(void)
{
    static uint32_t src[8] = {
        0x11223344u, 0x55667788u, 0x99AABBCCu, 0xDDEEFF00u,
        0x01234567u, 0x89ABCDEFu, 0x13579BDFu, 0x2468ACE0u
    };
    static uint32_t mem_copy[8];
    static uint32_t flash_copy[8];
    static const char dma_uart[] = "dma-uart\n";
    volatile const uint8_t *flash = (volatile const uint8_t *)DMA_FLASH_ADDR;

    DMAC_CFG = 1u;

    if (!dma_run(0u, (uint32_t)src, (uint32_t)mem_copy,
                 DMA_TYPE_M2M | DMA_WIDTH_WORD_SRC | DMA_WIDTH_WORD_DST |
                 DMA_SRC_INC | DMA_DST_INC, 8u)) {
        return 0;
    }
    if (!bytes_equal_local((const uint8_t *)src, (const uint8_t *)mem_copy, sizeof(src))) {
        return 0;
    }

    if (ROM_SPIF_ERASE_SECTOR(DMA_FLASH_OFF) != 0) {
        return 0;
    }
    if (!dma_run(1u, (uint32_t)src, DMA_FLASH_ADDR,
                 DMA_TYPE_M2M | DMA_WIDTH_WORD_SRC | DMA_WIDTH_WORD_DST |
                 DMA_SRC_INC | DMA_DST_INC, 8u)) {
        return 0;
    }
    if (!bytes_equal_local((const uint8_t *)src, (const uint8_t *)flash, sizeof(src))) {
        return 0;
    }

    if (!dma_run(2u, DMA_FLASH_ADDR, (uint32_t)flash_copy,
                 DMA_TYPE_M2M | DMA_WIDTH_WORD_SRC | DMA_WIDTH_WORD_DST |
                 DMA_SRC_INC | DMA_DST_INC, 8u)) {
        return 0;
    }
    if (!bytes_equal_local((const uint8_t *)src, (const uint8_t *)flash_copy, sizeof(src))) {
        return 0;
    }

    if (!dma_run(3u, (uint32_t)dma_uart, UART0_BASE,
                 DMA_TYPE_M2P | DMA_WIDTH_BYTE_SRC | DMA_WIDTH_BYTE_DST |
                 DMA_SRC_INC | DMA_DST_NO_CHANGE, sizeof(dma_uart) - 1u)) {
        return 0;
    }

    uart0_puts("dma-probe pass\n");
    return 1;
}

int test_controller_abi(void)
{
    uint8_t bytes[6] = {0, 1, 2, 3, 4, 5};
    uint8_t direct_addr[6] = {0};

    ROM_HCI_REVERSE_BYTES(bytes, sizeof(bytes));
    if (bytes[0] != 5u || bytes[1] != 4u || bytes[2] != 3u ||
        bytes[3] != 2u || bytes[4] != 1u || bytes[5] != 0u) {
        return 0;
    }

    if (ROM_HCI_PROCESS_EVENT(1u, 0x1234u) != 0u) {
        return 0;
    }
    if (ROM_LL_PROCESS_EVENT(2u, 0x4321u) != 0u) {
        return 0;
    }

    if (ROM_LL_SET_ADV_PARAM0(0x20u, 0x40u, 0u, 0u, 0u,
                              direct_addr, 0x07u, 0u) != LL_STATUS_SUCCESS) {
        return 0;
    }

    /* CONNECT can arrive from the live smoke command pipe before this probe.
       Both results are valid and together verify the host-controller boundary. */
    uint8_t dle = ROM_LL_SET_DATA_LENGTH0(0u, 251u, 2120u);
    if (dle != LL_STATUS_SUCCESS && dle != LL_STATUS_INACTIVE_CONNECTION) {
        return 0;
    }

    if (ROM_LL_RESET0() != LL_STATUS_SUCCESS) {
        return 0;
    }
    return test_dma();
}
