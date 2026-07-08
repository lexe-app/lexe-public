#ifndef	_STDIO_H
#define	_STDIO_H	1

#include <stdarg.h>

typedef struct _IO_FILE FILE;

#ifdef __cplusplus
extern "C" {
#endif

extern FILE *stderr;

extern int fprintf(FILE *stream, const char *format, ...);
extern int vsnprintf(char *s, unsigned long n, const char *format, va_list arg);

#ifdef __cplusplus
}
#endif

#endif /* _STDIO_H */
