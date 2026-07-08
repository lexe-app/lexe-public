#ifndef	_STDLIB_H

#define __need_size_t
#define __need_NULL
#include <stddef.h>

#define	_STDLIB_H	1

#ifdef __cplusplus
extern "C" {
#endif

extern char *getenv(const char *name);

#ifdef __cplusplus
}
#endif

#endif /* _STDLIB_H */
