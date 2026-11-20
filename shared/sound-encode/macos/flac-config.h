/* libFLAC 1.5.0's config.h for an Apple-silicon Mac (aarch64-apple-darwin, Apple's clang), as libFLAC's own CMake build
   writes it there from config.cmake.h.in, but written by hand, so that building needs no CMake. The Windows build's
   configuration is the one written inside build.rs; build.rs picks this one for the Mac.

   On ARM64 CMake finds arm_neon.h and the 64-bit NEON intrinsics, so libFLAC's NEON LPC code (lpc_intrin_neon.c) is built,
   and no x86 intrinsics. It finds pthreads too: the encoder can use threads, but only when asked, and this crate never asks
   (src/flac.rs), so it encodes on one thread as on Windows. No Ogg, as on Windows. */
#ifndef DOTANY_FLAC_CONFIG_H
#define DOTANY_FLAC_CONFIG_H

#define CPU_IS_BIG_ENDIAN 0
#define CPU_IS_LITTLE_ENDIAN 1
#define ENABLE_64_BIT_WORDS 1
#define FLAC__CPU_ARM64 1
#define FLAC__HAS_OGG 0
#define FLAC__HAS_X86INTRIN 0
#define FLAC__HAS_NEONINTRIN 1
#define FLAC__HAS_A64NEONINTRIN 1
#define FLAC__SYS_DARWIN 1

#define HAVE_BSWAP16 1
#define HAVE_BSWAP32 1
#define HAVE_CLOCK_GETTIME 1
#define HAVE_FSEEKO 1
#define HAVE_INTTYPES_H 1
#define HAVE_LANGINFO_CODESET 1
#define HAVE_LROUND 1
#define HAVE_PTHREAD 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define HAVE_SYS_IOCTL_H 1
#define HAVE_SYS_PARAM_H 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_TERMIOS_H 1
#define HAVE_TYPEOF 1
#define HAVE_UNISTD_H 1

#define PACKAGE_VERSION "1.5.0"
#define VERSION "1.5.0"

#endif
