#include <stddef.h>

__attribute__((optimize("O0")))
void *memcpy(void *dst, const void *src, size_t len)
{
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    while (len-- != 0u) {
        *d++ = *s++;
    }
    return dst;
}

__attribute__((optimize("O0")))
void *memset(void *dst, int value, size_t len)
{
    unsigned char *d = (unsigned char *)dst;
    while (len-- != 0u) {
        *d++ = (unsigned char)value;
    }
    return dst;
}
