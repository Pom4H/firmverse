#include "bus_dev.h"
#include "gpio.h"
#include "clock.h"
#include "timer.h"
#include "jump_function.h"
#include "pwrmgr.h"
#include "mcu.h"
#include "rf_phy_driver.h"
#include "flash.h"
#include "host_cfg.h"
#include "ll.h"
#include "log.h"

extern void init_config(void);
extern int app_main(void);
extern void hal_rom_boot_init(void);
extern void ll_patch_master(void);
extern void silicon_uart_put(unsigned char c);
extern void silicon_uart_reinit(void);
extern void silicon_stage(unsigned int mask);

#define BLE_MAX_ALLOW_CONNECTION 1
#define BLE_MAX_ALLOW_PKT_PER_EVENT_TX 3
#define BLE_MAX_ALLOW_PKT_PER_EVENT_RX 3
#define BLE_PKT_VERSION BLE_PKT_VERSION_5_1
#define BLE_PKT_BUF_SIZE \
    (((BLE_PKT_VERSION == BLE_PKT_VERSION_5_1) ? 1 : 0) * BLE_PKT51_LEN \
     + ((BLE_PKT_VERSION == BLE_PKT_VERSION_4_0) ? 1 : 0) * BLE_PKT40_LEN \
     + (sizeof(struct ll_pkt_desc) - 2))
#define BLE_MAX_ALLOW_PER_CONNECTION BLE_PKT_BUF_SIZE
#define BLE_CONN_BUF_SIZE (BLE_MAX_ALLOW_CONNECTION * BLE_MAX_ALLOW_PER_CONNECTION)
#define LARGE_HEAP_SIZE (8 * 1024)

ALIGN4_U8 g_pConnectionBuffer[BLE_CONN_BUF_SIZE];
llConnState_t pConnContext[BLE_MAX_ALLOW_CONNECTION];
ALIGN4_U8 g_largeHeap[LARGE_HEAP_SIZE];

volatile uint8 g_clk32K_config;
volatile sysclk_t g_spif_clk_config;

static void mark(unsigned char c, unsigned int led)
{
    silicon_stage(led);
    silicon_uart_put(c);
}

static void hal_low_power_io_init(void)
{
    ioinit_cfg_t ioInit[] = {
        {GPIO_P02, GPIO_FLOATING},
        {GPIO_P03, GPIO_FLOATING},
        {GPIO_P09, GPIO_PULL_UP},
        {GPIO_P10, GPIO_PULL_UP},
        {GPIO_P11, GPIO_PULL_DOWN},
        {GPIO_P14, GPIO_PULL_DOWN},
        {GPIO_P15, GPIO_PULL_DOWN},
        {GPIO_P16, GPIO_FLOATING},
        {GPIO_P18, GPIO_PULL_DOWN},
        {GPIO_P20, GPIO_PULL_DOWN},
        {GPIO_P00, GPIO_PULL_DOWN},
        {GPIO_P01, GPIO_PULL_DOWN},
        {GPIO_P07, GPIO_PULL_DOWN},
        {GPIO_P17, GPIO_FLOATING},
        {GPIO_P23, GPIO_PULL_DOWN},
        {GPIO_P24, GPIO_PULL_DOWN},
        {GPIO_P25, GPIO_PULL_DOWN},
        {GPIO_P26, GPIO_PULL_DOWN},
        {GPIO_P27, GPIO_PULL_DOWN},
        {GPIO_P31, GPIO_PULL_DOWN},
        {GPIO_P32, GPIO_PULL_DOWN},
        {GPIO_P33, GPIO_PULL_DOWN},
        {GPIO_P34, GPIO_PULL_DOWN},
    };
    uint8 i;

    for (i = 0; i < (uint8)(sizeof(ioInit) / sizeof(ioInit[0])); i++) {
        hal_gpio_pull_set(ioInit[i].pin, ioInit[i].type);
    }
    DCDC_CONFIG_SETTING(0x0a);
    DCDC_REF_CLK_SETTING(1);
    DIG_LDO_CURRENT_SETTING(0x01);
    hal_pwrmgr_RAM_retention(RET_SRAM0 | RET_SRAM1 | RET_SRAM2);
    hal_pwrmgr_RAM_retention_set();
    hal_pwrmgr_LowCurrentLdo_enable();
}

static void ble_mem_init_config(void)
{
    osal_mem_set_heap((osalMemHdr_t *)g_largeHeap, LARGE_HEAP_SIZE);
    LL_InitConnectContext(pConnContext,
                          g_pConnectionBuffer,
                          BLE_MAX_ALLOW_CONNECTION,
                          BLE_MAX_ALLOW_PKT_PER_EVENT_TX,
                          BLE_MAX_ALLOW_PKT_PER_EVENT_RX,
                          BLE_PKT_VERSION);
    Host_InitContext(MAX_NUM_LL_CONN,
                     glinkDB, glinkCBs,
                     smPairingParam,
                     gMTU_Size,
                     gAuthenLink,
                     l2capReassembleBuf, l2capSegmentBuf,
                     gattClientInfo,
                     gattServerInfo);
}

static void hal_rfphy_init(void)
{
    g_rfPhyTxPower = RF_PHY_TX_POWER_0DBM;
    g_rfPhyPktFmt = PKT_FMT_BLE1M;
    g_rfPhyFreqOffSet = RF_PHY_FREQ_FOFF_00KHZ;
    XTAL16M_CAP_SETTING(0x09);
    XTAL16M_CURRENT_SETTING(0x03);
    hal_rc32k_clk_tracking_init();
    hal_rom_boot_init();
    NVIC_SetPriority((IRQn_Type)BB_IRQn, IRQ_PRIO_REALTIME);
    NVIC_SetPriority((IRQn_Type)TIM1_IRQn, IRQ_PRIO_HIGH);
    NVIC_SetPriority((IRQn_Type)TIM2_IRQn, IRQ_PRIO_HIGH);
    NVIC_SetPriority((IRQn_Type)TIM4_IRQn, IRQ_PRIO_HIGH);
    ble_mem_init_config();
    hal_rfPhyFreqOff_Set();
}

static void hal_init(void)
{
    hal_low_power_io_init();
    clk_init(g_system_clk);
    silicon_uart_reinit();
    hal_rtc_clock_config((CLK32K_e)g_clk32K_config);
    hal_pwrmgr_init();
    {
        xflash_Ctx_t cfg;
        cfg.rd_instr = XFRD_FCMD_READ_DUAL;
        hal_spif_cache_init(cfg);
    }
    hal_gpio_init();
}

int main(void)
{
    g_system_clk = SYS_CLK_XTAL_16M;
    g_clk32K_config = CLK_32K_RCOSC;

    mark('I', 0x10u);
    drv_irq_init();
    init_config();

    /* SPIF XIP window is 64K until cache init. ll_patch_master and
     * LL_InitConnectContext sit past 0x11030000 and hang if called earlier. */
    mark('C', 0x1u);
    hal_init();

    mark('P', 0x80u);
    ll_patch_master();

    mark('R', 0x1000u);
    hal_rfphy_init();

    mark('A', 0x100u);
    app_main();
    return 0;
}
