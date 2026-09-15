use std::collections::BTreeMap;

pub(crate) fn hosted_provider_source(providers: &BTreeMap<String, String>) -> String {
    if providers.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        r#"
#include <string.h>

typedef struct {
    const uint8_t *data;
    uintptr_t len;
} ForgeStr;

typedef struct {
    uintptr_t slots;
    size_t element_size;
    void *data;
} ForgeRawStore;

static ForgeRawStore *forge_raw_stores = NULL;
static uintptr_t forge_raw_store_count = 0;
static uintptr_t forge_raw_store_capacity = 0;
static const uint8_t forge_empty_bytes[1] = {0};

static void forge_host_abort(void) { abort(); }

static ForgeStr forge_empty_str(void) {
    ForgeStr value = { forge_empty_bytes, 0 };
    return value;
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

static ForgeStr forge_str_copy(const uint8_t *data, uintptr_t len) {
    if (len == 0) return forge_empty_str();
    uint8_t *copy = malloc((size_t)len);
    if (copy == NULL) forge_host_abort();
    memcpy(copy, data, (size_t)len);
    ForgeStr value = { copy, len };
    return value;
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
"#,
    );

    for (name, symbol) in providers {
        let definition = match name.as_str() {
  "__forge_console_write" => format!(
      "void {symbol}(const uint8_t *data, uintptr_t len) {{ if (len != 0 && fwrite(data, 1, (size_t)len, stdout) != (size_t)len) forge_host_abort(); }}\n"
  ),
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
    out
}
