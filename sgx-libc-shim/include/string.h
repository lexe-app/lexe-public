#ifndef	_STRING_H
#define	_STRING_H	1

#define	__need_size_t
#define	__need_NULL
#include <stddef.h>

extern void *memcpy (void *__restrict dest, const void *__restrict src, size_t n);
extern void *memmove (void *dest, const void *src, size_t n);
extern void *memset (void *s, int c, size_t n);
extern int memcmp (const void *s1, const void *s2, size_t n);

#endif /* _STRING_H */
