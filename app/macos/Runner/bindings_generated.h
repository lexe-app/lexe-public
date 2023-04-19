#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
typedef struct _Dart_Handle* Dart_Handle;

typedef struct DartCObject DartCObject;

typedef int64_t DartPort;

typedef bool (*DartPostCObjectFnType)(DartPort port_id, void *message);

typedef struct wire_uint_8_list {
  uint8_t *ptr;
  int32_t len;
} wire_uint_8_list;

typedef struct DartCObject *WireSyncReturn;

typedef struct wire_Config {
  int32_t deploy_env;
  int32_t network;
} wire_Config;

typedef struct wire_App {
  const void *ptr;
} wire_App;

typedef struct wire_AppHandle {
  struct wire_App inner;
} wire_AppHandle;

void store_dart_post_cobject(DartPostCObjectFnType ptr);

Dart_Handle get_dart_object(uintptr_t ptr);

void drop_dart_object(uintptr_t ptr);

uintptr_t new_dart_opaque(Dart_Handle handle);

intptr_t init_frb_dart_api_dl(void *obj);

void wire_init_rust_log_stream(int64_t port_, struct wire_uint_8_list *rust_log);

WireSyncReturn wire_regtest__static_method__Config(void);

void wire_load__static_method__AppHandle(int64_t port_, struct wire_Config *config);

void wire_restore__static_method__AppHandle(int64_t port_,
                                            struct wire_Config *config,
                                            struct wire_uint_8_list *seed_phrase);

void wire_signup__static_method__AppHandle(int64_t port_, struct wire_Config *config);

void wire_node_info__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

void wire_fiat_rates__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

struct wire_App new_App(void);

struct wire_AppHandle *new_box_autoadd_app_handle_0(void);

struct wire_Config *new_box_autoadd_config_0(void);

struct wire_uint_8_list *new_uint_8_list_0(int32_t len);

void drop_opaque_App(const void *ptr);

const void *share_opaque_App(const void *ptr);

void free_WireSyncReturn(WireSyncReturn ptr);

static int64_t dummy_method_to_enforce_bundling(void) {
    int64_t dummy_var = 0;
    dummy_var ^= ((int64_t) (void*) wire_init_rust_log_stream);
    dummy_var ^= ((int64_t) (void*) wire_regtest__static_method__Config);
    dummy_var ^= ((int64_t) (void*) wire_load__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_restore__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_signup__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_node_info__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_fiat_rates__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) new_App);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_app_handle_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_config_0);
    dummy_var ^= ((int64_t) (void*) new_uint_8_list_0);
    dummy_var ^= ((int64_t) (void*) drop_opaque_App);
    dummy_var ^= ((int64_t) (void*) share_opaque_App);
    dummy_var ^= ((int64_t) (void*) free_WireSyncReturn);
    dummy_var ^= ((int64_t) (void*) store_dart_post_cobject);
    dummy_var ^= ((int64_t) (void*) get_dart_object);
    dummy_var ^= ((int64_t) (void*) drop_dart_object);
    dummy_var ^= ((int64_t) (void*) new_dart_opaque);
    return dummy_var;
}
