/* DotAny Archive's configuration for libarchive 3.8.11 on Windows with MSVC, written by hand (no CMake). Only the RAR,
   LHA, cpio and ar readers are built, with no compression libraries: RAR's and LHA's own decoders are inside libarchive,
   RAR's CRC32 comes from archive_crc32.h when zlib is absent, and whatever compresses a cpio is undone in Rust before
   libarchive sees it. See build.rs. */
#ifndef DOTANY_LIBARCHIVE_CONFIG_H
#define DOTANY_LIBARCHIVE_CONFIG_H

#define LIBARCHIVE_STATIC 1
#define _CRT_SECURE_NO_WARNINGS 1
#define _CRT_NONSTDC_NO_DEPRECATE 1
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#ifndef WINVER
#define WINVER 0x0A00
#endif
#ifndef NTDDI_VERSION
#define NTDDI_VERSION 0x0A000000
#endif

#define HAVE_ERRNO_H 1
#define HAVE_FCNTL_H 1
#define HAVE_INTTYPES_H 1
#define HAVE_IO_H 1
#define HAVE_LIMITS_H 1
#define HAVE_LOCALE_H 1
#define HAVE_SIGNAL_H 1
#define HAVE_STDARG_H 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_SYS_UTIME_H 1
#define HAVE_TIME_H 1
#define HAVE_WCHAR_H 1
#define HAVE_WINDOWS_H 1
#define HAVE_DIRECT_H 1
#define HAVE_PROCESS_H 1

#define HAVE_EILSEQ 1
#define HAVE_LOCALTIME_S 1
#define HAVE__LOCALTIME64_S 1
#define HAVE__MKGMTIME 1
#define HAVE__MKGMTIME64 1
#define HAVE_MBRTOWC 1
#define HAVE_WCRTOMB 1
#define HAVE_WCTOMB 1
#define HAVE_WCSCPY 1
#define HAVE_WCSLEN 1
#define HAVE_WMEMCMP 1
#define HAVE_WMEMCPY 1
#define HAVE_WMEMMOVE 1
#define HAVE_STRERROR_S 1
#define HAVE_STRNLEN 1
#define HAVE_VPRINTF 1
#define HAVE__CTIME64_S 1
#define HAVE__FSEEKI64 1
#define HAVE__GET_TIMEZONE 1

#define HAVE_DECL_SIZE_MAX 1
#define HAVE_DECL_INT32_MAX 1
#define HAVE_DECL_INT32_MIN 1
#define HAVE_DECL_UINT32_MAX 1
#define HAVE_DECL_INT64_MAX 1
#define HAVE_DECL_INT64_MIN 1
#define HAVE_DECL_UINT64_MAX 1
#define HAVE_DECL_INTMAX_MAX 1
#define HAVE_DECL_INTMAX_MIN 1
#define HAVE_DECL_UINTMAX_MAX 1

#define HAVE_INTMAX_T 1
#define HAVE_UINTMAX_T 1
#define HAVE_INT8_T 1
#define HAVE_INT16_T 1
#define HAVE_INT32_T 1
#define HAVE_INT64_T 1
#define HAVE_UINT8_T 1
#define HAVE_UINT16_T 1
#define HAVE_UINT32_T 1
#define HAVE_UINT64_T 1
#define HAVE_LONG_LONG_INT 1
#define HAVE_UNSIGNED_LONG_LONG 1
#define HAVE_UNSIGNED_LONG_LONG_INT 1
#define HAVE_WCHAR_T 1

#define SIZEOF_SHORT 2
#define SIZEOF_INT 4
#define SIZEOF_LONG 4
#define SIZEOF_LONG_LONG 8
#define SIZEOF_UNSIGNED_SHORT 2
#define SIZEOF_UNSIGNED 4
#define SIZEOF_UNSIGNED_LONG 4
#define SIZEOF_UNSIGNED_LONG_LONG 8
#define SIZEOF_WCHAR_T 2

/* what POSIX has and the Windows C library lacks or names differently */
#define id_t short
#define uid_t short
#define gid_t short
#define pid_t int
#define mode_t unsigned short
#define ssize_t __int64

#endif
