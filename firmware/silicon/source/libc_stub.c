#include "string.h"
#include "stdio.h"
#include "stdlib.h"

int printf(const char *fmt, ...)
{
    (void)fmt;
    return 0;
}

int sprintf(char *buf, const char *fmt, ...)
{
    (void)fmt;
    if (buf) {
        buf[0] = '\0';
    }
    return 0;
}

int snprintf(char *buf, unsigned long n, const char *fmt, ...)
{
    (void)fmt;
    (void)n;
    if (buf && n > 0u) {
        buf[0] = '\0';
    }
    return 0;
}

int atoi(const char *s)
{
    int v = 0;
    int sign = 1;
    if (!s) {
        return 0;
    }
    if (*s == '-') {
        sign = -1;
        s++;
    }
    while (*s >= '0' && *s <= '9') {
        v = v * 10 + (*s - '0');
        s++;
    }
    return sign * v;
}

void abort(void)
{
    for (;;) {
    }
}

void *memcpy(void *dst, const void *src, size_t n)
{
    unsigned char *d = dst;
    const unsigned char *s = src;
    while (n > 0u) {
        *d++ = *s++;
        n--;
    }
    return dst;
}

void *memmove(void *dst, const void *src, size_t n)
{
    unsigned char *d = dst;
    const unsigned char *s = src;
    if (d == s || n == 0u) {
        return dst;
    }
    if (d < s) {
        return memcpy(dst, src, n);
    }
    d += n;
    s += n;
    while (n > 0u) {
        *--d = *--s;
        n--;
    }
    return dst;
}

void *memset(void *dst, int c, size_t n)
{
    unsigned char *d = dst;
    unsigned char v = (unsigned char)c;
    while (n > 0u) {
        *d++ = v;
        n--;
    }
    return dst;
}

int memcmp(const void *a, const void *b, size_t n)
{
    const unsigned char *p = a;
    const unsigned char *q = b;
    while (n > 0u) {
        if (*p != *q) {
            return (int)*p - (int)*q;
        }
        p++;
        q++;
        n--;
    }
    return 0;
}

size_t strlen(const char *s)
{
    size_t n = 0u;
    while (s[n] != '\0') {
        n++;
    }
    return n;
}
