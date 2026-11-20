/* LAME 3.100's config.h for an Apple-silicon Mac (aarch64-apple-darwin, Apple's clang), as LAME's own configure writes
   it there from config.h.in, but written by hand, so that building needs no autoconf. The Windows build uses LAME's
   own configMS.h instead; build.rs picks this one for the Mac.

   What configure finds on macOS: the C99 headers and fixed-width types (so none of config.h.in's fallback typedefs for
   int8_t and the rest is needed; they would clash with the SDK's own headers, as configMS.h's do), a
   64-bit long, and a long double no wider than a double. No SSE (HAVE_XMMINTRIN_H) on ARM, so the vector/ file is not
   built. TAKEHIRO_IEEE754_HACK (sound on any IEEE 754 machine with a 32-bit int) and the float switches are this
   crate's build switches, given in build.rs exactly as for Windows, so the two encode alike; NDEBUG is left undefined, as there.
   */
#ifndef LAME_CONFIG_H
#define LAME_CONFIG_H

#define HAVE_DLFCN_H 1
#define HAVE_ERRNO_H 1
#define HAVE_FCNTL_H 1
#define HAVE_FLOAT_H 1
#define HAVE_GETTIMEOFDAY 1
#define HAVE_INTTYPES_H 1
#define HAVE_LIMITS_H 1
#define HAVE_MEMORY_H 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRCHR 1
#define HAVE_STRINGS_H 1
#define HAVE_STRING_H 1
#define HAVE_STRTOL 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TIME_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_UNISTD_H 1
#define STDC_HEADERS 1
#define PROTOTYPES 1
#define __PROTOTYPES 1

/* the fixed-width types are the system's (<inttypes.h>, which machine.h includes) */
#define HAVE_INT8_T 1
#define HAVE_INT16_T 1
#define HAVE_INT32_T 1
#define HAVE_INT64_T 1
#define HAVE_UINT8_T 1
#define HAVE_UINT16_T 1
#define HAVE_UINT32_T 1
#define HAVE_UINT64_T 1

/* the IEEE types have no system name: config.h.in's typedefs */
typedef float ieee754_float32_t;
typedef double ieee754_float64_t;
typedef long double ieee854_float80_t;

#define SIZEOF_DOUBLE 8
#define SIZEOF_FLOAT 4
#define SIZEOF_INT 4
#define SIZEOF_LONG 8
#define SIZEOF_LONG_DOUBLE 8
#define SIZEOF_SHORT 2
#define SIZEOF_UNSIGNED_INT 4
#define SIZEOF_UNSIGNED_LONG 8
#define SIZEOF_UNSIGNED_SHORT 2

/* faster log implementation with less but enough precision (as configMS.h, and configure's default) */
#define USE_FAST_LOG 1

/* configure defines it always: the library's own sources rely on it */
#define LAME_LIBRARY_BUILD 1

#define PACKAGE "lame"
#define PACKAGE_NAME "lame"
#define PACKAGE_VERSION "3.100"
#define VERSION "3.100"

#endif /* LAME_CONFIG_H */
