use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn hosted_provider_source(providers: &BTreeMap<String, String>) -> String {
    if providers.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        r#"
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <string.h>
#include <sys/file.h>
#include <time.h>
#include <unistd.h>

typedef struct {
    const uint8_t *data;
    uintptr_t len;
} ForgeStr;

typedef struct {
    uintptr_t handle;
} ForgeFileLock;

typedef struct {
    uintptr_t slots;
    size_t element_size;
    void *data;
} ForgeRawStore;

static ForgeRawStore *forge_raw_stores = NULL;
static uintptr_t forge_raw_store_count = 0;
static uintptr_t forge_raw_store_capacity = 0;
static const uint8_t forge_empty_bytes[1] = {0};

static ForgeStr *forge_process_args = NULL;
static uintptr_t forge_process_arg_count = 0;
static int forge_process_args_loaded = 0;

static void forge_host_abort(void) { abort(); }

static ForgeStr forge_empty_str(void) {
    ForgeStr value = { forge_empty_bytes, 0 };
    return value;
}

static ForgeStr forge_str_copy(const uint8_t *data, uintptr_t len) {
    if (len == 0) return forge_empty_str();
    uint8_t *copy = malloc((size_t)len);
    if (copy == NULL) forge_host_abort();
    memcpy(copy, data, (size_t)len);
    ForgeStr value = { copy, len };
    return value;
}

static char *forge_path_copy(ForgeStr path) {
    if (path.len == UINTPTR_MAX) forge_host_abort();
    if (path.len != 0 && memchr(path.data, 0, (size_t)path.len) != NULL) forge_host_abort();
    char *copy = malloc((size_t)path.len + 1);
    if (copy == NULL) forge_host_abort();
    if (path.len != 0) memcpy(copy, path.data, (size_t)path.len);
    copy[path.len] = '\0';
    return copy;
}

static void forge_load_process_args(void) {
    if (forge_process_args_loaded) return;
    forge_process_args_loaded = 1;

    FILE *file = fopen("/proc/self/cmdline", "rb");
    if (file == NULL) forge_host_abort();

    size_t capacity = 256;
    size_t len = 0;
    uint8_t *bytes = malloc(capacity);
    if (bytes == NULL) forge_host_abort();
    for (;;) {
        if (len == capacity) {
            if (capacity > SIZE_MAX / 2) forge_host_abort();
            capacity *= 2;
            uint8_t *grown = realloc(bytes, capacity);
            if (grown == NULL) forge_host_abort();
            bytes = grown;
        }
        size_t read = fread(bytes + len, 1, capacity - len, file);
        len += read;
        if (read == 0) {
            if (ferror(file)) forge_host_abort();
            break;
        }
    }
    if (fclose(file) != 0) forge_host_abort();

    size_t total = 0;
    for (size_t i = 0; i < len; i++) {
        if (bytes[i] == 0) total += 1;
    }
    if (total == 0) {
        free(bytes);
        return;
    }

    size_t wanted = total - 1;
    if (wanted > (size_t)UINTPTR_MAX) forge_host_abort();
    if (wanted != 0) {
        forge_process_args = calloc(wanted, sizeof(*forge_process_args));
        if (forge_process_args == NULL) forge_host_abort();
    }

    size_t item = 0;
    size_t start = 0;
    size_t ordinal = 0;
    for (size_t i = 0; i < len; i++) {
        if (bytes[i] != 0) continue;
        if (ordinal != 0) {
            forge_process_args[item].data = bytes + start;
            forge_process_args[item].len = (uintptr_t)(i - start);
            item += 1;
        }
        ordinal += 1;
        start = i + 1;
    }
    forge_process_arg_count = (uintptr_t)item;
    /* `bytes` intentionally lives for the process lifetime because returned str
       values borrow from it. */
}

static uintptr_t forge_raw_create(uintptr_t slots, size_t element_size) {
    if (slots != 0 && element_size > SIZE_MAX / slots) forge_host_abort();
    if (forge_raw_store_count == forge_raw_store_capacity) {
        uintptr_t next = forge_raw_store_capacity == 0 ? 16 : forge_raw_store_capacity * 2;
        if (next < forge_raw_store_capacity) forge_host_abort();
        ForgeRawStore *stores = realloc(forge_raw_stores, (size_t)next * sizeof(*stores));
        if (stores == NULL) forge_host_abort();
        forge_raw_stores = stores;
        forge_raw_store_capacity = next;
    }
    void *data = slots == 0 ? NULL : calloc((size_t)slots, element_size);
    if (slots != 0 && data == NULL) forge_host_abort();
    forge_raw_stores[forge_raw_store_count] = (ForgeRawStore){slots, element_size, data};
    forge_raw_store_count += 1;
    return forge_raw_store_count;
}

static ForgeRawStore *forge_raw_at(uintptr_t handle, size_t element_size) {
    if (handle == 0 || handle > forge_raw_store_count) forge_host_abort();
    ForgeRawStore *store = &forge_raw_stores[handle - 1];
    if (store->element_size != element_size) forge_host_abort();
    return store;
}

static void *forge_raw_slot(uintptr_t handle, uintptr_t index, size_t element_size) {
    ForgeRawStore *store = forge_raw_at(handle, element_size);
    if (index >= store->slots) forge_host_abort();
    return (uint8_t *)store->data + (size_t)index * element_size;
}

static void forge_raw_resize(uintptr_t handle, uintptr_t slots, size_t element_size) {
    ForgeRawStore *store = forge_raw_at(handle, element_size);
    if (slots != 0 && element_size > SIZE_MAX / slots) forge_host_abort();
    if (slots == 0) {
        free(store->data);
        store->data = NULL;
        store->slots = 0;
        return;
    }
    size_t bytes = (size_t)slots * element_size;
    void *data = realloc(store->data, bytes);
    if (data == NULL) forge_host_abort();
    if (slots > store->slots) {
        memset((uint8_t *)data + (size_t)store->slots * element_size, 0,
     (size_t)(slots - store->slots) * element_size);
    }
    store->data = data;
    store->slots = slots;
}

static void forge_raw_swap(uintptr_t left, uintptr_t right, size_t element_size) {
    ForgeRawStore *a = forge_raw_at(left, element_size);
    ForgeRawStore *b = forge_raw_at(right, element_size);
    ForgeRawStore tmp = *a;
    *a = *b;
    *b = tmp;
}

static ForgeStr forge_str_from_u64(uint64_t value) {
    char buffer[32];
    int written = snprintf(buffer, sizeof(buffer), "%llu", (unsigned long long)value);
    if (written < 0 || (size_t)written >= sizeof(buffer)) forge_host_abort();
    return forge_str_copy((const uint8_t *)buffer, (uintptr_t)written);
}

static uint64_t forge_parse_u64(ForgeStr text, uint64_t limit) {
    if (text.len == 0) forge_host_abort();
    uint64_t value = 0;
    for (uintptr_t i = 0; i < text.len; i++) {
        uint8_t byte = text.data[i];
        if (byte < '0' || byte > '9') forge_host_abort();
        uint64_t digit = (uint64_t)(byte - '0');
        if (value > (limit - digit) / 10) forge_host_abort();
        value = value * 10 + digit;
    }
    return value;
}

static uint8_t forge_str_equal(ForgeStr a, ForgeStr b) {
    if (a.len != b.len) return 0;
    if (a.len == 0) return 1;
    return (uint8_t)(memcmp(a.data, b.data, (size_t)a.len) == 0);
}

static ForgeStr forge_str_concat(ForgeStr a, ForgeStr b) {
    if (a.len > UINTPTR_MAX - b.len) forge_host_abort();
    uintptr_t len = a.len + b.len;
    if (len == 0) return forge_empty_str();
    uint8_t *copy = malloc((size_t)len);
    if (copy == NULL) forge_host_abort();
    if (a.len != 0) memcpy(copy, a.data, (size_t)a.len);
    if (b.len != 0) memcpy(copy + a.len, b.data, (size_t)b.len);
    ForgeStr value = { copy, len };
    return value;
}

static uintptr_t forge_str_line_count(ForgeStr text) {
    if (text.len == 0) return 0;
    uintptr_t count = 0;
    for (uintptr_t i = 0; i < text.len; i++) {
        if (text.data[i] == '\n') count += 1;
    }
    if (text.data[text.len - 1] != '\n') count += 1;
    return count;
}

static ForgeStr forge_str_line_at(ForgeStr text, uintptr_t index) {
    if (text.len == 0) forge_host_abort();
    uintptr_t current = 0;
    uintptr_t start = 0;
    for (uintptr_t i = 0; i <= text.len; i++) {
        if (i == text.len || text.data[i] == '\n') {
            if (i == text.len && i == start && i != 0) break;
            if (current == index) return forge_str_copy(text.data + start, i - start);
            current += 1;
            start = i + 1;
        }
    }
    forge_host_abort();
    return forge_empty_str();
}

static uintptr_t forge_str_first_tab(ForgeStr text) {
    for (uintptr_t i = 0; i < text.len; i++) {
        if (text.data[i] == '\t') return i;
    }
    return UINTPTR_MAX;
}

static ForgeStr forge_fs_read_text(ForgeStr path) {
    char *name = forge_path_copy(path);
    FILE *file = fopen(name, "rb");
    free(name);
    if (file == NULL) {
        if (errno == ENOENT) return forge_empty_str();
        forge_host_abort();
    }
    if (fseek(file, 0, SEEK_END) != 0) forge_host_abort();
    long end = ftell(file);
    if (end < 0 || (uintmax_t)end > (uintmax_t)UINTPTR_MAX) forge_host_abort();
    if (fseek(file, 0, SEEK_SET) != 0) forge_host_abort();
    uintptr_t len = (uintptr_t)end;
    if (len == 0) {
        if (fclose(file) != 0) forge_host_abort();
        return forge_empty_str();
    }
    uint8_t *data = malloc((size_t)len);
    if (data == NULL) forge_host_abort();
    if (fread(data, 1, (size_t)len, file) != (size_t)len) forge_host_abort();
    if (fclose(file) != 0) forge_host_abort();
    ForgeStr value = { data, len };
    return value;
}

static void forge_fs_write_text(ForgeStr path, ForgeStr text, const char *mode) {
    char *name = forge_path_copy(path);
    FILE *file = fopen(name, mode);
    free(name);
    if (file == NULL) forge_host_abort();
    if (text.len != 0 && fwrite(text.data, 1, (size_t)text.len, file) != (size_t)text.len) {
        forge_host_abort();
    }
    if (fclose(file) != 0) forge_host_abort();
}

static uint64_t forge_monotonic_us(void) {
    clock_t ticks = clock();
    if (ticks == (clock_t)-1) forge_host_abort();
    uint64_t value = (uint64_t)ticks;
    uint64_t whole = value / (uint64_t)CLOCKS_PER_SEC;
    uint64_t part = value % (uint64_t)CLOCKS_PER_SEC;
    if (whole > UINT64_MAX / UINT64_C(1000000)) forge_host_abort();
    return whole * UINT64_C(1000000)
        + (part * UINT64_C(1000000)) / (uint64_t)CLOCKS_PER_SEC;
}

static ForgeFileLock forge_lock_acquire(ForgeStr path) {
    char *name = forge_path_copy(path);
    int fd = open(name, O_RDWR | O_CREAT, 0666);
    free(name);
    if (fd < 0) forge_host_abort();
    if (flock(fd, LOCK_EX) != 0) forge_host_abort();
    ForgeFileLock lock = { (uintptr_t)fd + 1 };
    return lock;
}

static void forge_lock_release(ForgeFileLock lock) {
    if (lock.handle == 0 || lock.handle - 1 > (uintptr_t)INT_MAX) forge_host_abort();
    int fd = (int)(lock.handle - 1);
    if (flock(fd, LOCK_UN) != 0) forge_host_abort();
    if (close(fd) != 0) forge_host_abort();
}
"#,
    );

    let provider_names = providers.keys().cloned().collect::<BTreeSet<_>>();
    for (name, symbol) in providers {
        let definition = match name.as_str() {
  "__forge_panic" => format!(
      "__attribute__((noreturn)) void {symbol}(const void *info) {{ (void)info; forge_host_abort(); }}\n"
  ),
  "__forge_console_write" => format!(
      "void {symbol}(const uint8_t *data, uintptr_t len) {{ if (len != 0 && fwrite(data, 1, (size_t)len, stdout) != (size_t)len) forge_host_abort(); }}\n"
  ),
  "__forge_args_count" => format!("uintptr_t {symbol}(void) {{ forge_load_process_args(); return forge_process_arg_count; }}\n"),
  "__forge_args_get" => format!("ForgeStr {symbol}(uintptr_t index) {{ forge_load_process_args(); if (index >= forge_process_arg_count) forge_host_abort(); return forge_process_args[index]; }}\n"),
  "__forge_time_monotonic_us" => format!("uint64_t {symbol}(void) {{ return forge_monotonic_us(); }}\n"),
  "__forge_fs_read_text" => format!("ForgeStr {symbol}(ForgeStr path) {{ return forge_fs_read_text(path); }}\n"),
  "__forge_fs_write_text" => format!("void {symbol}(ForgeStr path, ForgeStr text) {{ forge_fs_write_text(path, text, \"wb\"); }}\n"),
  "__forge_fs_append_text" => format!("void {symbol}(ForgeStr path, ForgeStr text) {{ forge_fs_write_text(path, text, \"ab\"); }}\n"),
  "__forge_lock_acquire_exclusive" => format!("ForgeFileLock {symbol}(ForgeStr path) {{ return forge_lock_acquire(path); }}\n"),
  "__forge_lock_release" => format!("void {symbol}(ForgeFileLock lock) {{ forge_lock_release(lock); }}\n"),
  "__forge_raw_u8_create" => format!("uintptr_t {symbol}(uintptr_t slots) {{ return forge_raw_create(slots, sizeof(uint8_t)); }}\n"),
  "__forge_raw_u8_slots" => format!("uintptr_t {symbol}(uintptr_t handle) {{ return forge_raw_at(handle, sizeof(uint8_t))->slots; }}\n"),
  "__forge_raw_u8_get" => format!("uint8_t {symbol}(uintptr_t handle, uintptr_t index) {{ return *(uint8_t *)forge_raw_slot(handle, index, sizeof(uint8_t)); }}\n"),
  "__forge_raw_u8_set" => format!("void {symbol}(uintptr_t handle, uintptr_t index, uint8_t value) {{ *(uint8_t *)forge_raw_slot(handle, index, sizeof(uint8_t)) = value; }}\n"),
  "__forge_raw_u8_resize" => format!("void {symbol}(uintptr_t handle, uintptr_t slots) {{ forge_raw_resize(handle, slots, sizeof(uint8_t)); }}\n"),
  "__forge_raw_u8_swap" => format!("void {symbol}(uintptr_t left, uintptr_t right) {{ forge_raw_swap(left, right, sizeof(uint8_t)); }}\n"),
  "__forge_raw_u64_create" => format!("uintptr_t {symbol}(uintptr_t slots) {{ return forge_raw_create(slots, sizeof(uint64_t)); }}\n"),
  "__forge_raw_u64_slots" => format!("uintptr_t {symbol}(uintptr_t handle) {{ return forge_raw_at(handle, sizeof(uint64_t))->slots; }}\n"),
  "__forge_raw_u64_get" => format!("uint64_t {symbol}(uintptr_t handle, uintptr_t index) {{ return *(uint64_t *)forge_raw_slot(handle, index, sizeof(uint64_t)); }}\n"),
  "__forge_raw_u64_set" => format!("void {symbol}(uintptr_t handle, uintptr_t index, uint64_t value) {{ *(uint64_t *)forge_raw_slot(handle, index, sizeof(uint64_t)) = value; }}\n"),
  "__forge_raw_u64_resize" => format!("void {symbol}(uintptr_t handle, uintptr_t slots) {{ forge_raw_resize(handle, slots, sizeof(uint64_t)); }}\n"),
  "__forge_raw_u64_swap" => format!("void {symbol}(uintptr_t left, uintptr_t right) {{ forge_raw_swap(left, right, sizeof(uint64_t)); }}\n"),
  "__forge_raw_usize_create" => format!("uintptr_t {symbol}(uintptr_t slots) {{ return forge_raw_create(slots, sizeof(uintptr_t)); }}\n"),
  "__forge_raw_usize_slots" => format!("uintptr_t {symbol}(uintptr_t handle) {{ return forge_raw_at(handle, sizeof(uintptr_t))->slots; }}\n"),
  "__forge_raw_usize_get" => format!("uintptr_t {symbol}(uintptr_t handle, uintptr_t index) {{ return *(uintptr_t *)forge_raw_slot(handle, index, sizeof(uintptr_t)); }}\n"),
  "__forge_raw_usize_set" => format!("void {symbol}(uintptr_t handle, uintptr_t index, uintptr_t value) {{ *(uintptr_t *)forge_raw_slot(handle, index, sizeof(uintptr_t)) = value; }}\n"),
  "__forge_raw_usize_resize" => format!("void {symbol}(uintptr_t handle, uintptr_t slots) {{ forge_raw_resize(handle, slots, sizeof(uintptr_t)); }}\n"),
  "__forge_raw_usize_swap" => format!("void {symbol}(uintptr_t left, uintptr_t right) {{ forge_raw_swap(left, right, sizeof(uintptr_t)); }}\n"),
  "__forge_raw_string_create" => format!("uintptr_t {symbol}(uintptr_t slots) {{ return forge_raw_create(slots, sizeof(ForgeStr)); }}\n"),
  "__forge_raw_string_slots" => format!("uintptr_t {symbol}(uintptr_t handle) {{ return forge_raw_at(handle, sizeof(ForgeStr))->slots; }}\n"),
  "__forge_raw_string_get" => format!("ForgeStr {symbol}(uintptr_t handle, uintptr_t index) {{ return *(ForgeStr *)forge_raw_slot(handle, index, sizeof(ForgeStr)); }}\n"),
  "__forge_raw_string_set" => format!("void {symbol}(uintptr_t handle, uintptr_t index, ForgeStr value) {{ *(ForgeStr *)forge_raw_slot(handle, index, sizeof(ForgeStr)) = value; }}\n"),
  "__forge_raw_string_resize" => format!("void {symbol}(uintptr_t handle, uintptr_t slots) {{ forge_raw_resize(handle, slots, sizeof(ForgeStr)); }}\n"),
  "__forge_raw_string_swap" => format!("void {symbol}(uintptr_t left, uintptr_t right) {{ forge_raw_swap(left, right, sizeof(ForgeStr)); }}\n"),
  "__forge_string_concat" => format!("ForgeStr {symbol}(ForgeStr a, ForgeStr b) {{ return forge_str_concat(a, b); }}\n"),
  "__forge_string_line_count" => format!("uintptr_t {symbol}(ForgeStr text) {{ return forge_str_line_count(text); }}\n"),
  "__forge_string_line_at" => format!("ForgeStr {symbol}(ForgeStr text, uintptr_t index) {{ return forge_str_line_at(text, index); }}\n"),
  "__forge_string_has_tab" => format!("uint8_t {symbol}(ForgeStr text) {{ return (uint8_t)(forge_str_first_tab(text) != UINTPTR_MAX); }}\n"),
  "__forge_string_before_tab" => format!("ForgeStr {symbol}(ForgeStr text) {{ uintptr_t at = forge_str_first_tab(text); if (at == UINTPTR_MAX) forge_host_abort(); return forge_str_copy(text.data, at); }}\n"),
  "__forge_string_after_tab" => format!("ForgeStr {symbol}(ForgeStr text) {{ uintptr_t at = forge_str_first_tab(text); if (at == UINTPTR_MAX) forge_host_abort(); return forge_str_copy(text.data + at + 1, text.len - at - 1); }}\n"),
  "__forge_string_from_u64" => format!("ForgeStr {symbol}(uint64_t value) {{ return forge_str_from_u64(value); }}\n"),
  "__forge_string_from_usize" => format!("ForgeStr {symbol}(uintptr_t value) {{ return forge_str_from_u64((uint64_t)value); }}\n"),
  "__forge_string_parse_u64" => format!("uint64_t {symbol}(ForgeStr text) {{ return forge_parse_u64(text, UINT64_MAX); }}\n"),
  "__forge_string_parse_usize" => format!("uintptr_t {symbol}(ForgeStr text) {{ return (uintptr_t)forge_parse_u64(text, (uint64_t)UINTPTR_MAX); }}\n"),
  "__forge_string_byte_len" => format!("uintptr_t {symbol}(ForgeStr text) {{ return text.len; }}\n"),
  "__forge_string_byte_at" => format!("uintptr_t {symbol}(ForgeStr text, uintptr_t index) {{ if (index >= text.len) forge_host_abort(); return (uintptr_t)text.data[index]; }}\n"),
  "__forge_string_equal" => format!("uint8_t {symbol}(ForgeStr a, ForgeStr b) {{ return forge_str_equal(a, b); }}\n"),
  "__forge_string_not_equal" => format!("uint8_t {symbol}(ForgeStr a, ForgeStr b) {{ return (uint8_t)!forge_str_equal(a, b); }}\n"),
  _ => String::new(),
        };
        out.push_str(&definition);
    }

    for name in provider_names {
        if !matches!(
            name.as_str(),
            "__forge_panic"
                | "__forge_console_write"
                | "__forge_args_count"
                | "__forge_args_get"
                | "__forge_time_monotonic_us"
                | "__forge_fs_read_text"
                | "__forge_fs_write_text"
                | "__forge_fs_append_text"
                | "__forge_lock_acquire_exclusive"
                | "__forge_lock_release"
                | "__forge_raw_u8_create"
                | "__forge_raw_u8_slots"
                | "__forge_raw_u8_get"
                | "__forge_raw_u8_set"
                | "__forge_raw_u8_resize"
                | "__forge_raw_u8_swap"
                | "__forge_raw_u64_create"
                | "__forge_raw_u64_slots"
                | "__forge_raw_u64_get"
                | "__forge_raw_u64_set"
                | "__forge_raw_u64_resize"
                | "__forge_raw_u64_swap"
                | "__forge_raw_usize_create"
                | "__forge_raw_usize_slots"
                | "__forge_raw_usize_get"
                | "__forge_raw_usize_set"
                | "__forge_raw_usize_resize"
                | "__forge_raw_usize_swap"
                | "__forge_raw_string_create"
                | "__forge_raw_string_slots"
                | "__forge_raw_string_get"
                | "__forge_raw_string_set"
                | "__forge_raw_string_resize"
                | "__forge_raw_string_swap"
                | "__forge_string_concat"
                | "__forge_string_line_count"
                | "__forge_string_line_at"
                | "__forge_string_has_tab"
                | "__forge_string_before_tab"
                | "__forge_string_after_tab"
                | "__forge_string_from_u64"
                | "__forge_string_from_usize"
                | "__forge_string_parse_u64"
                | "__forge_string_parse_usize"
                | "__forge_string_byte_len"
                | "__forge_string_byte_at"
                | "__forge_string_equal"
                | "__forge_string_not_equal"
        ) {
            panic!("missing hosted provider implementation for {name}");
        }
    }
    out
}
