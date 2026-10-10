// ---- standard library: `use input`, `use os`, `use fs`, `use time`, `use random`, `use math`, `use text`
// The host's services are declared by hand (no <windows.h>: its macros would clash with user names).
#include <errno.h>
#include <math.h>
#include <time.h>
#ifdef _WIN32
#include <io.h>
#include <wchar.h>
typedef unsigned short nyrt_wchar;
__declspec(dllimport) int __stdcall MultiByteToWideChar(unsigned, unsigned long, const char *, int, nyrt_wchar *, int);
__declspec(dllimport) int __stdcall WideCharToMultiByte(unsigned, unsigned long, const nyrt_wchar *, int, char *, int, const char *, int *);
__declspec(dllimport) unsigned long __stdcall GetFileAttributesW(const nyrt_wchar *);
__declspec(dllimport) int __stdcall CreateDirectoryW(const nyrt_wchar *, void *);
__declspec(dllimport) int __stdcall RemoveDirectoryW(const nyrt_wchar *);
__declspec(dllimport) int __stdcall DeleteFileW(const nyrt_wchar *);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllimport) void *__stdcall FindFirstFileW(const nyrt_wchar *, void *);
__declspec(dllimport) int __stdcall FindNextFileW(void *, void *);
__declspec(dllimport) int __stdcall FindClose(void *);
__declspec(dllimport) nyrt_wchar *__stdcall GetCommandLineW(void);
__declspec(dllimport) nyrt_wchar **__stdcall CommandLineToArgvW(const nyrt_wchar *, int *);
__declspec(dllimport) unsigned long __stdcall GetEnvironmentVariableW(const nyrt_wchar *, nyrt_wchar *, unsigned long);
__declspec(dllimport) void __stdcall GetSystemTimeAsFileTime(uint64_t *);
__declspec(dllimport) int __stdcall QueryPerformanceCounter(int64_t *);
__declspec(dllimport) int __stdcall QueryPerformanceFrequency(int64_t *);
__declspec(dllimport) void __stdcall Sleep(unsigned long);
__declspec(dllimport) unsigned char __stdcall SystemFunction036(void *, unsigned long);
#else
#include <dirent.h>
#include <sys/types.h>
#include <unistd.h>
int mkdir(const char *, mode_t);
#endif

static int nyrt_argc;
static char **nyrt_argv;

// UTF-8 as Nyra strings hold it: no overlong forms, no surrogates, nothing above U+10FFFF.
static bool nyrt_utf8_valid(const char *s, int64_t n) {
    const unsigned char *p = (const unsigned char *)s;
    for (int64_t i = 0; i < n;) {
        unsigned c = p[i];
        if (c < 0x80) { i++; continue; }
        int k;
        unsigned min;
        if (c >= 0xC2 && c <= 0xDF) { k = 1; min = 0x80; }
        else if (c >= 0xE0 && c <= 0xEF) { k = 2; min = 0x800; }
        else if (c >= 0xF0 && c <= 0xF4) { k = 3; min = 0x10000; }
        else return false;
        if (n - i <= k) return false;
        unsigned cp = c & (k == 1 ? 0x1F : k == 2 ? 0x0F : 0x07);
        for (int j = 1; j <= k; j++) {
            if ((p[i + j] & 0xC0) != 0x80) return false;
            cp = (cp << 6) | (p[i + j] & 0x3F);
        }
        if (cp < min || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) return false;
        i += k + 1;
    }
    return true;
}

// A message `what "path" (reason)`, with the path shown like in other runtime errors.
static void nyrt_fs_fail(const char *what, const nyrt_str *path, const char *reason, int line, int col) {
    nyrt_buf b = nyrt_buf_new();
    nyrt_buf_cstr(&b, what);
    nyrt_buf_cstr(&b, " \"");
    nyrt_buf_shown(&b, path);
    nyrt_buf_cstr(&b, "\" (");
    nyrt_buf_cstr(&b, reason);
    nyrt_buf_cstr(&b, ")");
    nyrt_panic("E0340", nyrt_buf_done(&b)->data, "check the path: it is relative to the folder the program runs in (`fs.exists(path)` tests first)", line, col);
}
static const char *nyrt_errno_reason(int e) {
    if (e == ENOENT || e == ENOTDIR) return "not found";
    if (e == EACCES || e == EPERM) return "permission denied";
    if (e == EEXIST) return "already exists";
    if (e == ENOTEMPTY) return "not empty";
    return "io error";
}

#ifdef _WIN32
// A UTF-8 string as a NUL-terminated UTF-16 one (freed by the caller).
static nyrt_wchar *nyrt_wide(const char *s, int64_t n) {
    int len = MultiByteToWideChar(65001, 0, s, (int)n, NULL, 0);
    nyrt_wchar *w = malloc(((size_t)len + 1) * sizeof(nyrt_wchar));
    if (!w) nyrt_oom(0, 0);
    MultiByteToWideChar(65001, 0, s, (int)n, w, len);
    w[len] = 0;
    return w;
}
static nyrt_str *nyrt_from_wide(const nyrt_wchar *w) {
    int len = WideCharToMultiByte(65001, 0, w, -1, NULL, 0, NULL, NULL);
    nyrt_str *s = nyrt_str_alloc(len);
    WideCharToMultiByte(65001, 0, w, -1, s->data, len, NULL, NULL);
    s->len = len > 0 ? len - 1 : 0;
    s->data[s->len] = '\0';
    s->nchars = nyrt_utf8_count(s->data, s->len);
    return s;
}
static const char *nyrt_win_reason(unsigned long e) {
    if (e == 2 || e == 3) return "not found";
    if (e == 5 || e == 32) return "permission denied";
    if (e == 80 || e == 183) return "already exists";
    if (e == 145) return "not empty";
    return "io error";
}
#endif

// "" and a path with a NUL name no file on any system.
static bool nyrt_bad_path(const nyrt_str *path) { return path->len == 0 || memchr(path->data, 0, (size_t)path->len) != NULL; }
// 0: nothing there, 1: a file, 2: a directory.
static int nyrt_path_kind(const nyrt_str *path) {
    if (nyrt_bad_path(path)) return 0;
#ifdef _WIN32
    nyrt_wchar *w = nyrt_wide(path->data, path->len);
    unsigned long a = GetFileAttributesW(w);
    free(w);
    if (a == 0xFFFFFFFFul) return 0;
    return (a & 0x10) ? 2 : 1;
#else
    DIR *d = opendir(path->data);
    if (d) { closedir(d); return 2; }
    return access(path->data, F_OK) == 0 ? 1 : 0;
#endif
}
static FILE *nyrt_open(const nyrt_str *path, const char *mode) {
#ifdef _WIN32
    nyrt_wchar *w = nyrt_wide(path->data, path->len);
    wchar_t m[4] = { (wchar_t)mode[0], (wchar_t)mode[1], 0, 0 };
    FILE *f = _wfopen((const wchar_t *)w, m);
    free(w);
    return f;
#else
    return fopen(path->data, mode);
#endif
}

// ---- fs ------------------------------------------------------------------------------------
static nyrt_str *nyrt_std_fs_read(const nyrt_str *path, int line, int col) {
    const char *what = "fs.read: cannot read";
    int kind = nyrt_path_kind(path);
    if (kind == 0) nyrt_fs_fail(what, path, "not found", line, col);
    if (kind == 2) nyrt_fs_fail(what, path, "is a directory", line, col);
    FILE *f = nyrt_open(path, "rb");
    if (!f) nyrt_fs_fail(what, path, nyrt_errno_reason(errno), line, col);
    nyrt_buf b = nyrt_buf_new();
    char chunk[65536];
    size_t n;
    while ((n = fread(chunk, 1, sizeof chunk, f)) > 0) nyrt_buf_add(&b, chunk, (int64_t)n);
    bool bad = ferror(f);
    fclose(f);
    nyrt_str *s = nyrt_buf_done(&b);
    if (bad) { nyrt_str_release(s); nyrt_fs_fail(what, path, "io error", line, col); }
    if (!nyrt_utf8_valid(s->data, s->len)) { nyrt_str_release(s); nyrt_fs_fail(what, path, "not valid UTF-8", line, col); }
    return s;
}
static void nyrt_fs_put(const nyrt_str *path, const nyrt_str *text, const char *mode, const char *what, int line, int col) {
    if (nyrt_bad_path(path)) nyrt_fs_fail(what, path, "not found", line, col);
    if (nyrt_path_kind(path) == 2) nyrt_fs_fail(what, path, "is a directory", line, col);
    FILE *f = nyrt_open(path, mode);
    if (!f) nyrt_fs_fail(what, path, nyrt_errno_reason(errno), line, col);
    bool ok = fwrite(text->data, 1, (size_t)text->len, f) == (size_t)text->len;
    if (fclose(f) != 0) ok = false;
    if (!ok) nyrt_fs_fail(what, path, "io error", line, col);
}
static void nyrt_std_fs_write(const nyrt_str *path, const nyrt_str *text, int line, int col) {
    nyrt_fs_put(path, text, "wb", "fs.write: cannot write", line, col);
}
static void nyrt_std_fs_append(const nyrt_str *path, const nyrt_str *text, int line, int col) {
    nyrt_fs_put(path, text, "ab", "fs.append: cannot append to", line, col);
}
static bool nyrt_std_fs_exists(const nyrt_str *path, int line, int col) {
    (void)line; (void)col;
    return nyrt_path_kind(path) != 0;
}
static int nyrt_cmp_strp(const void *a, const void *b) { return nyrt_str_cmp(*(nyrt_str *const *)a, *(nyrt_str *const *)b); }
static nyrt_arr *nyrt_std_fs_list(const nyrt_str *dir, int line, int col) {
    const char *what = "fs.list: cannot list";
    int kind = nyrt_path_kind(dir);
    if (kind == 0) nyrt_fs_fail(what, dir, "not found", line, col);
    if (kind == 1) nyrt_fs_fail(what, dir, "not a directory", line, col);
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_str, 8);
#ifdef _WIN32
    nyrt_buf pat = nyrt_buf_new();
    nyrt_buf_str(&pat, dir);
    nyrt_buf_cstr(&pat, "/*");
    nyrt_wchar *w = nyrt_wide(pat.s->data, pat.s->len);
    nyrt_str_release(pat.s);
    // WIN32_FIND_DATAW: 44 bytes of attributes, times and sizes, then the name (260 wide chars)
    union { char raw[600]; uint32_t align; } data;
    void *h = FindFirstFileW(w, &data);
    free(w);
    if (h == (void *)(intptr_t)-1) {
        unsigned long e = GetLastError();
        if (e != 2 && e != 18) { nyrt_arr_release(r); nyrt_fs_fail(what, dir, nyrt_win_reason(e), line, col); }
    } else {
        do {
            const nyrt_wchar *name = (const nyrt_wchar *)(data.raw + 44);
            bool dot = name[0] == '.' && (name[1] == 0 || (name[1] == '.' && name[2] == 0));
            if (!dot) {
                nyrt_str *s = nyrt_from_wide(name);
                nyrt_arr_grow(&r, 1);
                ((nyrt_str **)r->data)[r->len++] = s;
            }
        } while (FindNextFileW(h, &data));
        FindClose(h);
    }
#else
    DIR *d = opendir(dir->data);
    if (!d) { nyrt_arr_release(r); nyrt_fs_fail(what, dir, nyrt_errno_reason(errno), line, col); }
    struct dirent *ent;
    while ((ent = readdir(d)) != NULL) {
        const char *name = ent->d_name;
        if (name[0] == '.' && (name[1] == 0 || (name[1] == '.' && name[2] == 0))) continue;
        int64_t n = (int64_t)strlen(name);
        if (!nyrt_utf8_valid(name, n)) { closedir(d); nyrt_arr_release(r); nyrt_fs_fail(what, dir, "not valid UTF-8", line, col); }
        nyrt_arr_grow(&r, 1);
        ((nyrt_str **)r->data)[r->len++] = nyrt_str_from(name, n);
    }
    closedir(d);
#endif
    qsort(r->data, (size_t)r->len, sizeof(nyrt_str *), nyrt_cmp_strp);
    return r;
}
static void nyrt_std_fs_remove(const nyrt_str *path, int line, int col) {
    const char *what = "fs.remove: cannot remove";
    int kind = nyrt_path_kind(path);
    if (kind == 0) nyrt_fs_fail(what, path, "not found", line, col);
    if (kind == 2) {
        nyrt_arr *inside = nyrt_std_fs_list(path, line, col);
        bool empty = inside->len == 0;
        nyrt_arr_release(inside);
        if (!empty) nyrt_fs_fail(what, path, "not empty", line, col);
    }
#ifdef _WIN32
    nyrt_wchar *w = nyrt_wide(path->data, path->len);
    int ok = kind == 2 ? RemoveDirectoryW(w) : DeleteFileW(w);
    free(w);
    if (!ok) nyrt_fs_fail(what, path, nyrt_win_reason(GetLastError()), line, col);
#else
    if ((kind == 2 ? rmdir(path->data) : unlink(path->data)) != 0) nyrt_fs_fail(what, path, nyrt_errno_reason(errno), line, col);
#endif
}
static void nyrt_std_fs_mkdir(const nyrt_str *path, int line, int col) {
    const char *what = "fs.mkdir: cannot create";
    if (nyrt_bad_path(path)) nyrt_fs_fail(what, path, "not found", line, col);
    if (nyrt_path_kind(path) != 0) nyrt_fs_fail(what, path, "already exists", line, col);
#ifdef _WIN32
    nyrt_wchar *w = nyrt_wide(path->data, path->len);
    int ok = CreateDirectoryW(w, NULL);
    free(w);
    if (!ok) nyrt_fs_fail(what, path, nyrt_win_reason(GetLastError()), line, col);
#else
    if (mkdir(path->data, 0777) != 0) nyrt_fs_fail(what, path, nyrt_errno_reason(errno), line, col);
#endif
}

// ---- input: standard input, read in binary (the same bytes on every system) -------------------
static char *nyrt_in = NULL;
static int64_t nyrt_in_pos = 0, nyrt_in_len = 0, nyrt_in_cap = 0;
static bool nyrt_in_end = false;
// Reads more input; false at the end. Reads what is there (a line typed at a terminal), not a full buffer.
static bool nyrt_in_fill(void) {
    if (nyrt_in_end) return false;
    if (!nyrt_in) {
#ifdef _WIN32
        _setmode(0, 0x8000);   // _O_BINARY: no CRLF translation, no Ctrl-Z end
#endif
        nyrt_in_cap = 65536;
        nyrt_in = malloc((size_t)nyrt_in_cap);
        if (!nyrt_in) nyrt_oom(0, 0);
    }
    if (nyrt_in_pos > 0) {
        memmove(nyrt_in, nyrt_in + nyrt_in_pos, (size_t)(nyrt_in_len - nyrt_in_pos));
        nyrt_in_len -= nyrt_in_pos;
        nyrt_in_pos = 0;
    }
    if (nyrt_in_len == nyrt_in_cap) {
        nyrt_in_cap *= 2;
        nyrt_in = nyrt_realloc(nyrt_in, (size_t)nyrt_in_cap);
    }
    for (;;) {
#ifdef _WIN32
        int n = _read(0, nyrt_in + nyrt_in_len, (unsigned)(nyrt_in_cap - nyrt_in_len));
#else
        long n = (long)read(0, nyrt_in + nyrt_in_len, (size_t)(nyrt_in_cap - nyrt_in_len));
        if (n < 0 && errno == EINTR) continue;
#endif
        if (n <= 0) { nyrt_in_end = true; return false; }
        nyrt_in_len += n;
        return true;
    }
}
static nyrt_str *nyrt_in_text(const char *p, int64_t n, const char *what, int line, int col) {
    if (!nyrt_utf8_valid(p, n)) {
        char msg[96];
        snprintf(msg, sizeof msg, "%s: the input is not valid UTF-8", what);
        nyrt_panic("E0341", msg, "standard input must be UTF-8 text", line, col);
    }
    return nyrt_str_from(p, n);
}
static nyrt_str *nyrt_std_input_line(int line, int col) {
    fflush(stdout);
    int64_t scan = nyrt_in_pos;
    for (;;) {
        char *nl = nyrt_in ? memchr(nyrt_in + scan, '\n', (size_t)(nyrt_in_len - scan)) : NULL;
        if (nl) {
            int64_t start = nyrt_in_pos, end = nl - nyrt_in;
            nyrt_in_pos = end + 1;
            if (end > start && nyrt_in[end - 1] == '\r') end--;
            return nyrt_in_text(nyrt_in + start, end - start, "input.line", line, col);
        }
        scan = nyrt_in_len - nyrt_in_pos;
        if (!nyrt_in_fill()) break;
    }
    int64_t start = nyrt_in_pos;
    nyrt_in_pos = nyrt_in_len;
    return nyrt_in_text(nyrt_in + start, nyrt_in_len - start, "input.line", line, col);
}
static bool nyrt_std_input_eof(int line, int col) {
    (void)line; (void)col;
    fflush(stdout);
    while (nyrt_in_pos == nyrt_in_len)
        if (!nyrt_in_fill()) return true;
    return false;
}
static nyrt_str *nyrt_std_input_all(int line, int col) {
    fflush(stdout);
    while (nyrt_in_fill()) {}
    int64_t start = nyrt_in_pos;
    nyrt_in_pos = nyrt_in_len;
    return nyrt_in_text(nyrt_in + start, nyrt_in_len - start, "input.all", line, col);
}
// The lines of a text: each without its `\n` (and a `\r` before it); no last empty line.
static nyrt_arr *nyrt_lines(const char *p, int64_t n) {
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_str, 8);
    int64_t i = 0;
    while (i < n) {
        const char *nl = memchr(p + i, '\n', (size_t)(n - i));
        int64_t end = nl ? nl - p : n, next = nl ? end + 1 : n;
        if (nl && end > i && p[end - 1] == '\r') end--;
        nyrt_arr_grow(&r, 1);
        ((nyrt_str **)r->data)[r->len++] = nyrt_str_from(p + i, end - i);
        i = next;
    }
    return r;
}
static nyrt_arr *nyrt_std_input_lines(int line, int col) {
    fflush(stdout);
    while (nyrt_in_fill()) {}
    int64_t start = nyrt_in_pos;
    nyrt_in_pos = nyrt_in_len;
    if (!nyrt_utf8_valid(nyrt_in + start, nyrt_in_len - start))
        nyrt_panic("E0341", "input.lines: the input is not valid UTF-8", "standard input must be UTF-8 text", line, col);
    return nyrt_lines(nyrt_in + start, nyrt_in_len - start);
}

// ---- os --------------------------------------------------------------------------------------
static nyrt_arr *nyrt_std_os_args(int line, int col) {
    nyrt_arr *r = nyrt_arr_new(&nyrt_T_str, nyrt_argc);
#ifdef _WIN32
    (void)line; (void)col;
    int n = 0;
    nyrt_wchar **w = CommandLineToArgvW(GetCommandLineW(), &n);
    for (int i = 1; w && i < n; i++) {
        nyrt_arr_grow(&r, 1);
        ((nyrt_str **)r->data)[r->len++] = nyrt_from_wide(w[i]);
    }
#else
    for (int i = 1; i < nyrt_argc; i++) {
        int64_t n = (int64_t)strlen(nyrt_argv[i]);
        if (!nyrt_utf8_valid(nyrt_argv[i], n))
            nyrt_panic("E0341", "os.args: an argument is not valid UTF-8", "program arguments must be UTF-8 text", line, col);
        nyrt_arr_grow(&r, 1);
        ((nyrt_str **)r->data)[r->len++] = nyrt_str_from(nyrt_argv[i], n);
    }
#endif
    return r;
}
// The value of an environment variable, or NULL when it is not set.
static nyrt_str *nyrt_getenv(const nyrt_str *name, int line, int col) {
    if (name->len == 0 || memchr(name->data, '=', (size_t)name->len) || memchr(name->data, 0, (size_t)name->len)) return NULL;
#ifdef _WIN32
    (void)line; (void)col;
    nyrt_wchar *w = nyrt_wide(name->data, name->len);
    unsigned long n = GetEnvironmentVariableW(w, NULL, 0);
    nyrt_str *s = NULL;
    if (n > 0) {
        nyrt_wchar *v = malloc((size_t)n * sizeof(nyrt_wchar));
        if (!v) nyrt_oom(0, 0);
        v[0] = 0;
        GetEnvironmentVariableW(w, v, n);
        s = nyrt_from_wide(v);
        free(v);
    }
    free(w);
    return s;
#else
    const char *v = getenv(name->data);
    if (!v) return NULL;
    int64_t n = (int64_t)strlen(v);
    if (!nyrt_utf8_valid(v, n))
        nyrt_panic("E0341", "os.env: the value is not valid UTF-8", "environment variables must be UTF-8 text", line, col);
    return nyrt_str_from(v, n);
#endif
}
static nyrt_str *nyrt_std_os_env(const nyrt_str *name, int line, int col) {
    nyrt_str *s = nyrt_getenv(name, line, col);
    return s ? s : nyrt_str_alloc(0);
}
static bool nyrt_std_os_has_env(const nyrt_str *name, int line, int col) {
    nyrt_str *s = nyrt_getenv(name, line, col);
    if (!s) return false;
    nyrt_str_release(s);
    return true;
}
static void nyrt_std_os_exit(int64_t code, int line, int col) {
    (void)line; (void)col;
    fflush(stdout);
    exit((int)(code & 255));
}

// ---- time ------------------------------------------------------------------------------------
static int64_t nyrt_std_time_now_ms(int line, int col) {
    (void)line; (void)col;
#ifdef _WIN32
    uint64_t ft;
    GetSystemTimeAsFileTime(&ft);   // 100 ns steps since 1601
    return (int64_t)((ft - 116444736000000000ull) / 10000);
#else
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
#endif
}
static double nyrt_std_time_mono_ms(int line, int col) {
    (void)line; (void)col;
#ifdef _WIN32
    int64_t now, freq;
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&freq);
    return (double)(now / freq) * 1000.0 + (double)(now % freq) * 1000.0 / (double)freq;
#else
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec * 1000.0 + (double)t.tv_nsec / 1e6;
#endif
}
static void nyrt_std_time_sleep_ms(int64_t ms, int line, int col) {
    (void)line; (void)col;
    fflush(stdout);
    if (ms <= 0) return;
#ifdef _WIN32
    while (ms > 0) {
        unsigned long part = ms > 86400000 ? 86400000ul : (unsigned long)ms;
        Sleep(part);
        ms -= (int64_t)part;
    }
#else
    struct timespec t = { (time_t)(ms / 1000), (long)(ms % 1000) * 1000000L };
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {}
#endif
}

// ---- random: the operating system's generator, or after `random.seed(n)` xoshiro128** -----------
static bool nyrt_seeded = false;
static uint32_t nyrt_rs[4];
static uint32_t nyrt_pool[64];
static int nyrt_pool_left = 0;
static uint32_t nyrt_rotl(uint32_t x, int k) { return (x << k) | (x >> (32 - k)); }
static uint32_t nyrt_mix32(uint32_t z) {
    z = (z ^ (z >> 16)) * 0x85EBCA6Bu;
    z = (z ^ (z >> 13)) * 0xC2B2AE35u;
    return z ^ (z >> 16);
}
static uint32_t nyrt_bits32(void) {
    if (nyrt_seeded) {
        uint32_t *s = nyrt_rs;
        uint32_t r = nyrt_rotl(s[1] * 5u, 7) * 9u, t = s[1] << 9;
        s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3]; s[2] ^= t; s[3] = nyrt_rotl(s[3], 11);
        return r;
    }
    if (nyrt_pool_left == 0) {
        bool ok;
#ifdef _WIN32
        ok = SystemFunction036(nyrt_pool, sizeof nyrt_pool) != 0;
#else
        static FILE *dev = NULL;
        if (!dev) dev = fopen("/dev/urandom", "rb");
        ok = dev && fread(nyrt_pool, 1, sizeof nyrt_pool, dev) == sizeof nyrt_pool;
#endif
        if (!ok) {
            fflush(stdout);
            fputs("random: no randomness source available\n", stderr);
            exit(101);
        }
        nyrt_pool_left = 64;
    }
    return nyrt_pool[--nyrt_pool_left];
}
static void nyrt_std_random_seed(int64_t n, int line, int col) {
    (void)line; (void)col;
    uint64_t u = (uint64_t)n;
    uint32_t half[2] = { (uint32_t)u, (uint32_t)(u >> 32) };
    for (int i = 0; i < 4; i++) nyrt_rs[i] = nyrt_mix32(half[i & 1] + (uint32_t)(i + 1) * 0x9E3779B9u);
    if (!(nyrt_rs[0] | nyrt_rs[1] | nyrt_rs[2] | nyrt_rs[3])) nyrt_rs[0] = 1;
    nyrt_seeded = true;
}
// 53 random bits: the first draw gives the high 27, the second the low 26.
static int64_t nyrt_bits53(void) {
    int64_t a = nyrt_bits32() >> 5;
    int64_t b = nyrt_bits32() >> 6;
    return a * 67108864 + b;
}
static double nyrt_std_random_random(int line, int col) {
    (void)line; (void)col;
    return (double)nyrt_bits53() / 9007199254740992.0;
}
static int64_t nyrt_std_random_range(int64_t lo, int64_t hi, int line, int col) {
    uint64_t n = (uint64_t)hi - (uint64_t)lo;
    if (hi <= lo || n > 9007199254740992ull) {
        char msg[128];
        snprintf(msg, sizeof msg, "random.range(%lld, %lld): need lo < hi and hi - lo <= 2^53", (long long)lo, (long long)hi);
        nyrt_panic("E0342", msg, "the upper bound is excluded: `random.range(1, 7)` rolls a die", line, col);
    }
    int64_t limit = 9007199254740992 - (int64_t)(9007199254740992ull % n), r;
    do r = nyrt_bits53(); while (r >= limit);
    return (int64_t)((uint64_t)lo + (uint64_t)r % n);
}

// ---- math: operations that are exact on every host -----------------------------------------------
static double nyrt_std_math_sqrt(double x, int line, int col) { (void)line; (void)col; return sqrt(x); }
static double nyrt_std_math_floor(double x, int line, int col) { (void)line; (void)col; return floor(x); }
static double nyrt_std_math_ceil(double x, int line, int col) { (void)line; (void)col; return ceil(x); }
static double nyrt_std_math_trunc(double x, int line, int col) { (void)line; (void)col; return trunc(x); }
// half away from zero (x - trunc(x) is exact)
static double nyrt_std_math_round(double x, int line, int col) {
    (void)line; (void)col;
    double t = trunc(x);
    if (fabs(x - t) >= 0.5) t += x < 0 ? -1.0 : 1.0;
    return t;
}

// ---- text ------------------------------------------------------------------------------------
static bool nyrt_text_int_ok(const nyrt_str *s) {
    int64_t i = 0, n = s->len;
    bool neg = n > 0 && s->data[0] == '-';
    if (neg) i = 1;
    uint64_t v = 0, limit = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    if (i >= n) return false;
    for (; i < n; i++) {
        char c = s->data[i];
        if (c < '0' || c > '9' || v > (limit - (uint64_t)(c - '0')) / 10) return false;
        v = v * 10 + (uint64_t)(c - '0');
    }
    return true;
}
static bool nyrt_std_text_is_int(const nyrt_str *s, int line, int col) { (void)line; (void)col; return nyrt_text_int_ok(s); }
static bool nyrt_std_text_is_float(const nyrt_str *s, int line, int col) {
    (void)line; (void)col;
    const char *p = s->data, *e = s->data + s->len, *d;
    if (p < e && *p == '-') p++;
    d = p;
    while (p < e && *p >= '0' && *p <= '9') p++;
    if (p == d) return false;
    if (p < e && *p == '.') {
        d = ++p;
        while (p < e && *p >= '0' && *p <= '9') p++;
        if (p == d) return false;
    }
    if (p < e && (*p == 'e' || *p == 'E')) {
        p++;
        if (p < e && (*p == '+' || *p == '-')) p++;
        d = p;
        while (p < e && *p >= '0' && *p <= '9') p++;
        if (p == d) return false;
    }
    return p == e;
}
// `text.fixed(x, d)`: the exact value of x rounded to d decimals, ties away from zero. The double
// is m * 2^e; x * 10^d = m * 5^d * 2^(e+d) is computed exactly with 32-bit limbs.
#define NYRT_LIMBS 80
static void nyrt_big_mul_small(uint32_t *a, int *n, uint32_t k) {
    uint64_t carry = 0;
    for (int i = 0; i < *n; i++) { uint64_t v = (uint64_t)a[i] * k + carry; a[i] = (uint32_t)v; carry = v >> 32; }
    if (carry) a[(*n)++] = (uint32_t)carry;
}
static nyrt_str *nyrt_std_text_fixed(double x, int64_t d, int line, int col) {
    if (d < 0 || d > 100) {
        char msg[96];
        snprintf(msg, sizeof msg, "text.fixed: digits must be 0 to 100, got %lld", (long long)d);
        nyrt_panic("E0342", msg, "`text.fixed(x, 2)` shows two decimals", line, col);
    }
    if (x != x || x > DBL_MAX || x < -DBL_MAX) {
        char t[32];
        nyrt_float_fmt(t, x);
        return nyrt_str_from(t, (int64_t)strlen(t));
    }
    uint64_t bits;
    memcpy(&bits, &x, 8);
    bool neg = bits >> 63;
    int ex = (int)((bits >> 52) & 0x7FF);
    uint64_t m = bits & 0xFFFFFFFFFFFFFull;
    if (ex == 0) ex = 1; else m |= 1ull << 52;
    int e = ex - 1075;   // x = m * 2^e
    uint32_t a[NYRT_LIMBS] = { (uint32_t)m, (uint32_t)(m >> 32) };
    int n = a[1] ? 2 : 1;
    for (int64_t i = 0; i < d; i++) nyrt_big_mul_small(a, &n, 5);
    int s = e + (int)d;
    if (s > 0) {
        // shift left by s bits
        int words = s / 32, bitsh = s % 32;
        for (int i = n - 1; i >= 0; i--) a[i + words] = a[i];
        for (int i = 0; i < words; i++) a[i] = 0;
        n += words;
        if (bitsh) {
            uint32_t carry = 0;
            for (int i = 0; i < n; i++) { uint32_t v = a[i]; a[i] = (v << bitsh) | carry; carry = v >> (32 - bitsh); }
            if (carry) a[n++] = carry;
        }
    } else if (s < 0) {
        int k = -s;
        // round half away from zero: bit k-1 decides
        bool up = (k - 1) / 32 < n && ((a[(k - 1) / 32] >> ((k - 1) % 32)) & 1);
        int words = k / 32, bitsh = k % 32;
        if (words >= n) { n = 1; a[0] = 0; }
        else {
            for (int i = 0; i + words < n; i++) a[i] = a[i + words];
            n -= words;
            if (bitsh) {
                for (int i = 0; i < n; i++) a[i] = (a[i] >> bitsh) | (i + 1 < n ? a[i + 1] << (32 - bitsh) : 0);
            }
        }
        if (up) {
            int i = 0;
            while (i < n && ++a[i] == 0) i++;
            if (i == n) a[n++] = 1;
        }
    }
    while (n > 1 && a[n - 1] == 0) n--;
    // the digits, least significant first
    char digits[512];
    int nd = 0;
    while (n > 1 || a[0] != 0) {
        uint64_t rem = 0;
        for (int i = n - 1; i >= 0; i--) { uint64_t v = (rem << 32) | a[i]; a[i] = (uint32_t)(v / 1000000000u); rem = v % 1000000000u; }
        while (n > 1 && a[n - 1] == 0) n--;
        for (int j = 0; j < 9; j++) { digits[nd++] = (char)('0' + rem % 10); rem /= 10; }
    }
    while (nd > 0 && digits[nd - 1] == '0') nd--;
    bool zero = nd == 0;
    while (nd <= d) digits[nd++] = '0';
    nyrt_buf b = nyrt_buf_new();
    if (neg && !zero) nyrt_buf_add(&b, "-", 1);
    for (int i = nd - 1; i >= 0; i--) {
        if (i == d - 1) nyrt_buf_add(&b, ".", 1);
        nyrt_buf_add(&b, &digits[i], 1);
    }
    return nyrt_buf_done(&b);
}
