#include "bcomdef.h"
#include "OSAL.h"
#include "gap.h"
#include "gapgattserver.h"
#include "gatt.h"
#include "gattservapp.h"
#include "hci.h"
#include "ll.h"
#include "central.h"
#include "gpio.h"
#include "clock.h"
#include "bus_dev.h"
#include "log.h"
#include "global_config.h"
#include "simpleBLEPeripheral.h"

extern void silicon_uart_put(unsigned char c);
extern void silicon_uart_reinit(void);

#define TRACK_COUNT 12u
#define STALE_MS 4000u
#define RANK_MS 20u
#define UART_EVERY 25u
#define COLOR_NONE ((int8)(-1))
#define RANK_LED_COUNT 5u
#define HCI_LE_META_EVENT 0x3E
#define HCI_ADV_REPORT 0x02

static uint8 app_task_id;
static uint32 tick_ms;
static uint8 scan_ok;
static uint8 scan_err;
static uint8 gap_ready;
static uint8 live_n;
static int8 peak_rssi;
static uint8 uart_div;

static const gpio_pin_e led_pin[RANK_LED_COUNT] = {
    GPIO_P07, GPIO_P11, GPIO_P18, GPIO_P00, GPIO_P34
};
static const char color_name[RANK_LED_COUNT] = { 'R', 'G', 'B', 'Y', 'W' };

struct Device {
    uint8 addr[B_ADDR_LEN];
    int8 rssi;
    int8 color;
    uint8 used;
    uint32 last_ms;
};

static struct Device devices[TRACK_COUNT];

static uint8 device_name[GAP_DEVICE_NAME_LEN] = "rssi-rank";
extern uint8 central_task_id;

static void apply_scan(const uint8 *addr, int8 rssi);
static void rank_and_drive(void);
static void leds_init(void);
static void start_scan(void);
static void uart_diag(void);
static void process_hci_msg(osal_event_hdr_t *hdr);
static void role_event_cb(gapCentralRoleEvent_t *evt);
static void role_rssi_cb(uint16 conn, int8 rssi);

static const gapCentralRoleCB_t role_cb = {
    role_rssi_cb,
    role_event_cb
};

static void uart_putc(unsigned char c)
{
    silicon_uart_put(c);
}

static void uart_hex(uint8 v)
{
    uint8 hi = (uint8)(v >> 4);
    uint8 lo = (uint8)(v & 0x0Fu);
    uart_putc((unsigned char)(hi < 10u ? '0' + hi : 'A' + (hi - 10u)));
    uart_putc((unsigned char)(lo < 10u ? '0' + lo : 'A' + (lo - 10u)));
}

static void uart_diag(void)
{
    uart_putc('E');
    uart_hex(scan_err);
    uart_putc(' ');
    uart_putc('G');
    uart_hex(gap_ready);
    uart_putc(' ');
    uart_putc('S');
    uart_hex(scan_ok);
    uart_putc(' ');
    uart_putc('N');
    uart_hex(live_n);
    uart_putc(' ');
    uart_putc('D');
    uart_hex((uint8)peak_rssi);
    uart_putc('\r');
    uart_putc('\n');
}

static uint8 addr_eq(const uint8 *a, const uint8 *b)
{
    uint8 i;
    for (i = 0; i < B_ADDR_LEN; i++) {
        if (a[i] != b[i]) {
            return FALSE;
        }
    }
    return TRUE;
}

static void log_mac(const uint8 *addr)
{
    LOG("%02x%02x%02x%02x%02x%02x",
        addr[0], addr[1], addr[2], addr[3], addr[4], addr[5]);
}

void SimpleBLEPeripheral_Init(uint8 task_id)
{
    uint8 scan_res = 16;
    uint16 scan_window = 0x30;
    uint16 scan_interval = 0x30;

    app_task_id = task_id;
    silicon_uart_reinit();
    uart_diag();
    leds_init();

    if (pGlobal_config) {
        pGlobal_config[LL_SWITCH] |= GAP_DUP_RPT_FILTER_DISALLOW;
    }

    GAPCentralRole_SetParameter(GAPCENTRALROLE_MAX_SCAN_RES, sizeof(uint8), &scan_res);
    GAP_SetParamValue(TGAP_GEN_DISC_SCAN_WIND, scan_window);
    GAP_SetParamValue(TGAP_GEN_DISC_SCAN_INT, scan_interval);
    GAP_SetParamValue(TGAP_CONN_SCAN_WIND, scan_window);
    GAP_SetParamValue(TGAP_CONN_SCAN_INT, scan_interval);
    GAP_SetParamValue(TGAP_FILTER_ADV_REPORTS, FALSE);
    GAP_SetParamValue(TGAP_GEN_DISC_SCAN, 0xFFFF);
    GAP_SetParamValue(TGAP_LIM_DISC_SCAN, 0xFFFF);
    GAP_SetParamValue(TGAP_SCAN_RSP_RSSI_MIN, (uint16)(-100));

    GGS_SetParameter(GGS_DEVICE_NAME_ATT, GAP_DEVICE_NAME_LEN, device_name);
    GATT_InitClient();
    GGS_AddService(GATT_ALL_SERVICES);
    GATTServApp_AddService(GATT_ALL_SERVICES);

    osal_set_event(app_task_id, SBP_START_DEVICE_EVT);
}

uint16 SimpleBLEPeripheral_ProcessEvent(uint8 task_id, uint16 events)
{
    (void)task_id;
    if (events & SYS_EVENT_MSG) {
        uint8 *msg;
        while ((msg = osal_msg_receive(app_task_id)) != NULL) {
            osal_event_hdr_t *hdr = (osal_event_hdr_t *)msg;
            if (hdr->event == HCI_GAP_EVENT_EVENT) {
                process_hci_msg(hdr);
            }
            osal_msg_deallocate(msg);
        }
        return (uint16)(events ^ SYS_EVENT_MSG);
    }
    if (events & SBP_START_DEVICE_EVT) {
        scan_err = GAPCentralRole_StartDevice((gapCentralRoleCB_t *)&role_cb);
        uart_diag();
        osal_start_timerEx(app_task_id, SBP_RANK_EVT, RANK_MS);
        return (uint16)(events ^ SBP_START_DEVICE_EVT);
    }
    if (events & SBP_ENABLE_SCAN_EVT) {
        start_scan();
        uart_diag();
        return (uint16)(events ^ SBP_ENABLE_SCAN_EVT);
    }
    if (events & SBP_RANK_EVT) {
        tick_ms += RANK_MS;
        rank_and_drive();
        uart_div++;
        if (uart_div >= UART_EVERY) {
            uart_div = 0;
            uart_diag();
        }
        osal_start_timerEx(app_task_id, SBP_RANK_EVT, RANK_MS);
        return (uint16)(events ^ SBP_RANK_EVT);
    }
    return 0;
}

static void start_scan(void)
{
    uint8 ret;

    ret = GAPCentralRole_StartDiscovery(DEVDISC_MODE_ALL, TRUE, FALSE);
    scan_err = ret;
    scan_ok = (uint8)((ret == SUCCESS || ret == bleAlreadyInRequestedMode) ? 1u : 0u);
}

static void process_hci_msg(osal_event_hdr_t *hdr)
{
    hciEvt_BLEAdvPktReport_t *rpt;
    uint8 i;

    if (hdr->status != HCI_LE_META_EVENT) {
        return;
    }
    rpt = (hciEvt_BLEAdvPktReport_t *)hdr;
    if (rpt->BLEEventCode != HCI_ADV_REPORT || rpt->devInfo == NULL) {
        return;
    }
    for (i = 0; i < rpt->numDevices; i++) {
        apply_scan(rpt->devInfo[i].addr, rpt->devInfo[i].rssi);
    }
    rank_and_drive();
}

static void role_rssi_cb(uint16 conn, int8 rssi)
{
    (void)conn;
    (void)rssi;
}

static void role_event_cb(gapCentralRoleEvent_t *evt)
{
    switch (evt->gap.opcode) {
    case GAP_DEVICE_INIT_DONE_EVENT:
        scan_err = evt->initDone.hdr.status;
        if (evt->initDone.hdr.status == SUCCESS) {
            gap_ready = 1u;
            osal_start_timerEx(app_task_id, SBP_ENABLE_SCAN_EVT, 1000);
        }
        uart_diag();
        break;
    case GAP_DEVICE_INFO_EVENT:
        apply_scan(evt->deviceInfo.addr, evt->deviceInfo.rssi);
        rank_and_drive();
        break;
    case GAP_DEVICE_DISCOVERY_EVENT:
        scan_ok = 0;
        osal_set_event(app_task_id, SBP_ENABLE_SCAN_EVT);
        break;
    default:
        break;
    }
}

static void leds_init(void)
{
    uint8 i;
    for (i = 0; i < RANK_LED_COUNT; i++) {
        hal_gpio_cfg_analog_io(led_pin[i], Bit_DISABLE);
        hal_gpio_pin_init(led_pin[i], GPIO_OUTPUT);
        hal_gpio_fmux(led_pin[i], Bit_DISABLE);
        hal_gpio_write(led_pin[i], 0);
    }
}

static int find_addr(const uint8 *addr)
{
    uint8 i;
    for (i = 0; i < TRACK_COUNT; i++) {
        if (devices[i].used && addr_eq(devices[i].addr, addr)) {
            return (int)i;
        }
    }
    return -1;
}

static void free_color(struct Device *dev)
{
    if (dev->color >= 0) {
        LOG("vacant %c ", color_name[(uint8)dev->color]);
        log_mac(dev->addr);
        LOG("\n");
        dev->color = COLOR_NONE;
    }
}

static int8 first_vacant_color(void)
{
    uint8 taken = 0;
    uint8 i;
    uint8 color;
    for (i = 0; i < TRACK_COUNT; i++) {
        if (devices[i].used && devices[i].color >= 0) {
            taken = (uint8)(taken | (uint8)(1u << (uint8)devices[i].color));
        }
    }
    for (color = 0; color < RANK_LED_COUNT; color++) {
        if ((taken & (uint8)(1u << color)) == 0u) {
            return (int8)color;
        }
    }
    return COLOR_NONE;
}

static int alloc_slot(void)
{
    int weakest = -1;
    int8 weakest_rssi = 127;
    uint8 i;
    for (i = 0; i < TRACK_COUNT; i++) {
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

static uint8 stronger(const struct Device *a, const struct Device *b)
{
    uint8 i;
    if (a->rssi != b->rssi) {
        return a->rssi > b->rssi ? TRUE : FALSE;
    }
    for (i = 0; i < B_ADDR_LEN; i++) {
        if (a->addr[i] != b->addr[i]) {
            return a->addr[i] < b->addr[i] ? TRUE : FALSE;
        }
    }
    return FALSE;
}

static uint16 blink_half_ms(int8 rssi)
{
    int16 depth;

    if (rssi >= -35) {
        return 40u;
    }
    if (rssi <= -90) {
        return 800u;
    }
    depth = (int16)((-35) - (int16)rssi);
    return (uint16)(40u + (uint16)depth * 14u);
}

static uint8 blink_on(int8 rssi)
{
    uint16 half = blink_half_ms(rssi);
    return (uint8)(((tick_ms / (uint32)half) & 1u) != 0u ? 1u : 0u);
}

static void apply_scan(const uint8 *addr, int8 rssi)
{
    int slot = find_addr(addr);

    if (slot < 0) {
        slot = alloc_slot();
        if (slot < 0) {
            return;
        }
        osal_memcpy(devices[slot].addr, addr, B_ADDR_LEN);
        devices[slot].color = COLOR_NONE;
        devices[slot].used = TRUE;
        LOG("scan ");
        log_mac(addr);
        LOG(" %d\n", rssi);
    }
    devices[slot].rssi = rssi;
    devices[slot].last_ms = tick_ms;
}

static void rank_and_drive(void)
{
    uint8 live[TRACK_COUNT];
    uint8 n = 0;
    uint8 i;
    uint8 j;
    uint8 gpio;

    for (i = 0; i < TRACK_COUNT; i++) {
        if (!devices[i].used) {
            continue;
        }
        if ((tick_ms - devices[i].last_ms) >= STALE_MS) {
            LOG("stale ");
            log_mac(devices[i].addr);
            LOG("\n");
            free_color(&devices[i]);
            devices[i].used = FALSE;
            continue;
        }
        live[n++] = i;
    }

    for (i = 1; i < n; i++) {
        uint8 key = live[i];
        j = i;
        while (j > 0u && stronger(&devices[key], &devices[live[j - 1u]])) {
            live[j] = live[j - 1u];
            j--;
        }
        live[j] = key;
    }

    while (n > RANK_LED_COUNT) {
        n--;
        free_color(&devices[live[n]]);
    }
    live_n = n;
    peak_rssi = (n > 0u) ? devices[live[0]].rssi : (int8)(-128);

    if (n == 0u) {
        uint8 on = 0;
        if (gap_ready == 0u) {
            on = ((tick_ms / 400u) & 1u) != 0u ? 1u : 0u;
            for (i = 0; i < RANK_LED_COUNT; i++) {
                hal_gpio_write(led_pin[i], (i == 3u && on) ? 1u : 0u);
            }
        } else if (scan_ok != 0u) {
            on = ((tick_ms / 400u) & 1u) != 0u ? 1u : 0u;
            for (i = 0; i < RANK_LED_COUNT; i++) {
                hal_gpio_write(led_pin[i], (i == 0u && on) ? 1u : 0u);
            }
        } else {
            for (i = 0; i < RANK_LED_COUNT; i++) {
                uint8 bit = 0;
                if (((tick_ms / 200u) & 1u) != 0u) {
                    if (i == 4u) {
                        bit = 1u;
                    } else {
                        bit = (scan_err & (uint8)(1u << i)) != 0u ? 1u : 0u;
                    }
                }
                hal_gpio_write(led_pin[i], bit);
            }
        }
        return;
    }

    gpio = 0;
    for (i = 0; i < n; i++) {
        struct Device *dev = &devices[live[i]];
        if (dev->color < 0) {
            int8 color = first_vacant_color();
            if (color >= 0) {
                dev->color = color;
                LOG("color %c <- ", color_name[(uint8)color]);
                log_mac(dev->addr);
                LOG("\n");
            }
        }
        if (dev->color >= 0 && blink_on(dev->rssi) != 0u) {
            gpio = (uint8)(gpio | (uint8)(1u << (uint8)dev->color));
        }
    }

    for (i = 0; i < RANK_LED_COUNT; i++) {
        hal_gpio_write(led_pin[i], (gpio & (uint8)(1u << i)) ? 1 : 0);
    }
}
