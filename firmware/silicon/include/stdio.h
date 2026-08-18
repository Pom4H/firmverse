#ifndef SILICON_STDIO_H
#define SILICON_STDIO_H

int printf(const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, unsigned long n, const char *fmt, ...);

#endif
