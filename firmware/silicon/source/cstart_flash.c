#include "bus_dev.h"

extern unsigned char _sbss;
extern unsigned char _ebss;
extern unsigned char _eronly;
extern unsigned char _sdata;
extern unsigned char _edata;
extern int main(void);

/* Lives in flash (.textentry). SDK c_start sits in SRAM .data, so a cold
 * boot that does not preload SRAM never reaches main. */
void c_start(void)
{
    unsigned char *dest;
    unsigned char *end;
    const unsigned char *src;

    AP_PCR->CACHE_BYPASS = 1;

    dest = &_sbss;
    end = &_ebss;
    while (dest < end) {
        *dest++ = 0;
    }

    src = &_eronly;
    dest = &_sdata;
    end = &_edata;
    while (dest < end) {
        *dest++ = *src++;
    }

    (void)main();
    for (;;) {
    }
}
