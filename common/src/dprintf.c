#include "dprintf.h"

#include <stdarg.h>
#include <stdio.h>
#include <windows.h>

void dprintf(const char *fmt, ...)
{
    char buffer[1024];
    va_list ap;
    int len;

    va_start(ap, fmt);
    len = vsnprintf(buffer, sizeof(buffer), fmt, ap);
    va_end(ap);

    if (len <= 0) {
        return;
    }

    OutputDebugStringA(buffer);
    fputs(buffer, stderr);
}
