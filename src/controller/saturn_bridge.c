/*
 * Firmverse ↔ upstream fbd-runtime bridge for Saturn-PLC.
 *
 * The runtime itself is pinned as third_party/fbd-runtime. This file only owns
 * the environment boundary: PLC pins, NVRAM, hardware properties and HMI hooks.
 * Keeping those responsibilities here lets Firmverse execute the exact runtime
 * without copying its semantics into Rust.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include "fbdrt.h"

#define FV_MAX_PINS 128
#define FV_MAX_HARDWARE 32
#define FV_NVRAM_SIGNALS (NVRAMSIZE / SIGNAL_SIZE)

static tSignal fv_inputs[FV_MAX_PINS];
static tSignal fv_outputs[FV_MAX_PINS];
static tSignal fv_hardware[FV_MAX_HARDWARE];
static tSignal fv_nvram[FV_NVRAM_SIGNALS];

static unsigned char *fv_schema = NULL;
static char *fv_memory = NULL;
static int fv_memory_size = 0;

/* fbd-runtime callbacks --------------------------------------------------- */

tSignal FBDgetProc(char type, tSignal index)
{
    switch (type) {
    case FBD_PIN:
        if (index >= 0 && index < FV_MAX_PINS) return fv_inputs[index];
        return 0;
    case FBD_NVRAM:
        if (index >= 0 && index < FV_NVRAM_SIGNALS) return fv_nvram[index];
        return 0;
    case FBD_HRDW:
        if (index >= 0 && index < FV_MAX_HARDWARE) return fv_hardware[index];
        return 0;
    default:
        return 0;
    }
}

void FBDsetProc(char type, tSignal index, tSignal *value)
{
    if (value == NULL) return;
    switch (type) {
    case FBD_PIN:
        if (index >= 0 && index < FV_MAX_PINS) fv_outputs[index] = *value;
        break;
    case FBD_NVRAM:
        if (index >= 0 && index < FV_NVRAM_SIGNALS) fv_nvram[index] = *value;
        break;
    default:
        break;
    }
}

/* Upstream enables HMI in fbdrt.h. Native Firmverse does not paint the
 * controller display yet; these hooks intentionally form a no-op display
 * backend while watchpoints/setpoints/metadata remain fully available. */
void FBDdrawRectangle(tScreenDim x1, tScreenDim y1, tScreenDim x2, tScreenDim y2, tColor color)
{ (void)x1; (void)y1; (void)x2; (void)y2; (void)color; }
void FBDdrawText(tScreenDim x1, tScreenDim y1, unsigned char font, tColor color, tColor bkcolor, bool transparent, char *text)
{ (void)x1; (void)y1; (void)font; (void)color; (void)bkcolor; (void)transparent; (void)text; }
void FBDdrawLine(tScreenDim x1, tScreenDim y1, tScreenDim x2, tScreenDim y2, tColor color)
{ (void)x1; (void)y1; (void)x2; (void)y2; (void)color; }
void FBDdrawEllipse(tScreenDim x1, tScreenDim y1, tScreenDim x2, tScreenDim y2, tColor color)
{ (void)x1; (void)y1; (void)x2; (void)y2; (void)color; }
void FBDdrawImage(tScreenDim x1, tScreenDim y1, tScreenDim image)
{ (void)x1; (void)y1; (void)image; }
void FBDdrawEnd(void) {}

/* Firmverse ABI ---------------------------------------------------------- */

void fv_fbd_unload(void)
{
    if (fv_memory != NULL) free(fv_memory);
    if (fv_schema != NULL) free(fv_schema);
    fv_memory = NULL;
    fv_schema = NULL;
    fv_memory_size = 0;
    memset(fv_inputs, 0, sizeof(fv_inputs));
    memset(fv_outputs, 0, sizeof(fv_outputs));
    memset(fv_hardware, 0, sizeof(fv_hardware));
}

int fv_fbd_load(const unsigned char *data, int length, int reset_nvram)
{
    int size;
    if (data == NULL || length <= 0) return -100;

    fv_fbd_unload();
    fv_schema = (unsigned char *)malloc((size_t)length);
    if (fv_schema == NULL) return -101;
    memcpy(fv_schema, data, (size_t)length);

    size = fbdInit((DESCR_MEM unsigned char *)fv_schema);
    if (size <= 0) {
        fv_fbd_unload();
        return size;
    }

    fv_memory = (char *)calloc(1, (size_t)size);
    if (fv_memory == NULL) {
        fv_fbd_unload();
        return -102;
    }
    fv_memory_size = size;
    memset(fv_outputs, 0, sizeof(fv_outputs));
    memset(fv_inputs, 0, sizeof(fv_inputs));
    memset(fv_hardware, 0, sizeof(fv_hardware));
    if (reset_nvram) memset(fv_nvram, 0, sizeof(fv_nvram));
    fbdSetMemory(fv_memory, reset_nvram != 0);
    return size;
}

int fv_fbd_memory_size(void) { return fv_memory_size; }
void fv_fbd_step(int period) { if (fv_memory != NULL) fbdDoStep((tSignal)period); }

void fv_fbd_set_input(int pin, int value)
{
    if (pin >= 0 && pin < FV_MAX_PINS) fv_inputs[pin] = (tSignal)value;
}

int fv_fbd_get_input(int pin)
{
    if (pin >= 0 && pin < FV_MAX_PINS) return fv_inputs[pin];
    return 0;
}

int fv_fbd_get_output(int pin)
{
    if (pin >= 0 && pin < FV_MAX_PINS) return fv_outputs[pin];
    return 0;
}

void fv_fbd_set_hardware(int index, int value)
{
    if (index >= 0 && index < FV_MAX_HARDWARE) fv_hardware[index] = (tSignal)value;
}

int fv_fbd_sp_count(void)
{
    tHMIdata data;
    int count = 0;
    while (fbdHMIgetSP(count, &data)) count++;
    return count;
}

int fv_fbd_sp_value(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.value;
}

int fv_fbd_sp_low(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.lowlimit;
}

int fv_fbd_sp_high(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.upperLimit;
}

int fv_fbd_sp_default(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.defValue;
}

int fv_fbd_sp_divider(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.divider;
}

int fv_fbd_sp_step(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return 0;
    return data.step;
}

const char *fv_fbd_sp_caption(int index)
{
    tHMIdata data;
    if (!fbdHMIgetSP(index, &data)) return NULL;
    return data.caption;
}

void fv_fbd_sp_set(int index, int value) { fbdHMIsetSP(index, (tSignal)value); }

int fv_fbd_wp_count(void)
{
    tHMIdata data;
    int count = 0;
    while (fbdHMIgetWP(count, &data)) count++;
    return count;
}

int fv_fbd_wp_value(int index)
{
    tHMIdata data;
    if (!fbdHMIgetWP(index, &data)) return 0;
    return data.value;
}

int fv_fbd_wp_divider(int index)
{
    tHMIdata data;
    if (!fbdHMIgetWP(index, &data)) return 0;
    return data.divider;
}

const char *fv_fbd_wp_caption(int index)
{
    tHMIdata data;
    if (!fbdHMIgetWP(index, &data)) return NULL;
    return data.caption;
}

const char *fv_fbd_project_field(int field)
{
    tHMIdescription info;
    fbdHMIgetDescription(&info);
    switch (field) {
    case 0: return info.name;
    case 1: return info.version;
    case 2: return info.btime;
    default: return NULL;
    }
}

const char *fv_fbd_io_hint(int type, int index)
{
    return fbdHMIgetIOhint((char)type, (char)index);
}
