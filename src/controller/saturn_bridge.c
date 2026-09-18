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
static int fv_schema_size = 0;

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

/* Bounded observation-only HMI buffer shared by native and WASM hosts. */
#define FV_DRAW_LIMIT 256
#define FV_TEXT_LIMIT 256
typedef struct {
    int kind, x1, y1, x2, y2, color, background, font, transparent, image;
    char text[FV_TEXT_LIMIT];
} FvDraw;
static FvDraw fv_draws[FV_DRAW_LIMIT];
static int fv_draw_count = 0;
static int fv_draw_overflow = 0;
static FvDraw *fv_draw(int kind) {
    FvDraw *d;
    if (fv_draw_count >= FV_DRAW_LIMIT) { fv_draw_overflow = 1; return NULL; }
    d = &fv_draws[fv_draw_count++]; memset(d, 0, sizeof(*d)); d->kind = kind; return d;
}
void FBDdrawRectangle(tScreenDim x1,tScreenDim y1,tScreenDim x2,tScreenDim y2,tColor color) {
    FvDraw *d=fv_draw(0); if(d){d->x1=x1;d->y1=y1;d->x2=x2;d->y2=y2;d->color=color;}
}
void FBDdrawText(tScreenDim x,tScreenDim y,unsigned char font,tColor color,tColor bk,bool transparent,char *text) {
    FvDraw *d=fv_draw(1); if(d){d->x1=x;d->y1=y;d->font=font;d->color=color;d->background=bk;d->transparent=transparent;
    if(text){size_t n=strlen(text);if(n>=FV_TEXT_LIMIT){fv_draw_overflow=1;n=FV_TEXT_LIMIT-1;}memcpy(d->text,text,n);}}
}
void FBDdrawLine(tScreenDim x1,tScreenDim y1,tScreenDim x2,tScreenDim y2,tColor color) {
    FvDraw *d=fv_draw(2);if(d){d->x1=x1;d->y1=y1;d->x2=x2;d->y2=y2;d->color=color;}
}
void FBDdrawEllipse(tScreenDim x1,tScreenDim y1,tScreenDim x2,tScreenDim y2,tColor color) {
    FvDraw *d=fv_draw(3);if(d){d->x1=x1;d->y1=y1;d->x2=x2;d->y2=y2;d->color=color;}
}
void FBDdrawImage(tScreenDim x,tScreenDim y,tScreenDim image) {
    FvDraw *d=fv_draw(4);if(d){d->x1=x;d->y1=y;d->image=image;}
}
void FBDdrawEnd(void) {}
extern DESCR_MEM tScreen DESCR_MEM_SUFX *fbdScreensBuf;
extern DESCR_MEM tSignal DESCR_MEM_SUFX *fbdGlobalOptions;
extern DESCR_MEM unsigned char DESCR_MEM_SUFX *fbdGlobalOptionsCount;
extern void drawCurrentScreen(DESCR_MEM tScreen DESCR_MEM_SUFX *screen);
int fv_fbd_render(int index) {
    int i; DESCR_MEM tScreen *screen=fbdScreensBuf;
    fv_draw_count=0;fv_draw_overflow=0;
    if(!fv_memory || index<0 || *fbdGlobalOptionsCount<=FBD_OPT_SCREEN_COUNT || index>=fbdGlobalOptions[FBD_OPT_SCREEN_COUNT]) return 0;
    for(i=0;i<index;i++)screen=(DESCR_MEM tScreen*)((DESCR_MEM char*)screen+screen->len);
    drawCurrentScreen(screen);return fv_draw_overflow?-1:fv_draw_count;
}
int fv_fbd_draw_count(void){return fv_draw_count;}
int fv_fbd_draw_field(int index,int field){
    FvDraw *d;if(index<0||index>=fv_draw_count)return 0;d=&fv_draws[index];
    switch(field){case 0:return d->kind;case 1:return d->x1;case 2:return d->y1;case 3:return d->x2;case 4:return d->y2;
    case 5:return d->color;case 6:return d->background;case 7:return d->font;case 8:return d->transparent;case 9:return d->image;default:return 0;}
}
const char *fv_fbd_draw_text(int index){return index>=0&&index<fv_draw_count?fv_draws[index].text:NULL;}

/* Firmverse ABI ---------------------------------------------------------- */

void fv_fbd_unload(void)
{
    if (fv_memory != NULL) free(fv_memory);
    if (fv_schema != NULL) free(fv_schema);
    fv_memory = NULL;
    fv_schema = NULL;
    fv_memory_size = 0;
    fv_schema_size = 0;
    fv_draw_count = 0;
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
    fv_schema_size = length;

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

#include "saturn_state.inc"
