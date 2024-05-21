#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
typedef struct _Dart_Handle* Dart_Handle;

/**
 * The maximum allowed payment note size in bytes.
 *
 * See [`common::constants::MAX_PAYMENT_NOTE_BYTES`].
 */
#define MAX_PAYMENT_NOTE_BYTES 512

typedef struct DartCObject DartCObject;

typedef int64_t DartPort;

typedef bool (*DartPostCObjectFnType)(DartPort port_id, void *message);

typedef struct DartCObject *WireSyncReturn;

typedef struct wire_uint_8_list {
  uint8_t *ptr;
  int32_t len;
} wire_uint_8_list;

typedef struct wire_Config {
  int32_t deploy_env;
  int32_t network;
  struct wire_uint_8_list *gateway_url;
  bool use_sgx;
  struct wire_uint_8_list *base_app_data_dir;
  bool use_mock_secret_store;
} wire_Config;

typedef struct wire_App {
  const void *ptr;
} wire_App;

typedef struct wire_AppHandle {
  struct wire_App inner;
} wire_AppHandle;

typedef struct wire_ClientPaymentId {
  struct wire_uint_8_list *id;
} wire_ClientPaymentId;

typedef struct wire_SendOnchainRequest {
  struct wire_ClientPaymentId cid;
  struct wire_uint_8_list *address;
  uint64_t amount_sats;
  int32_t priority;
  struct wire_uint_8_list *note;
} wire_SendOnchainRequest;

typedef struct wire_EstimateFeeSendOnchainRequest {
  struct wire_uint_8_list *address;
  uint64_t amount_sats;
} wire_EstimateFeeSendOnchainRequest;

typedef struct wire_CreateInvoiceRequest {
  uint32_t expiry_secs;
  uint64_t *amount_sats;
  struct wire_uint_8_list *description;
} wire_CreateInvoiceRequest;

typedef struct wire_PayInvoiceRequest {
  struct wire_uint_8_list *invoice;
  uint64_t *fallback_amount_sats;
  struct wire_uint_8_list *note;
} wire_PayInvoiceRequest;

typedef struct wire_UpdatePaymentNote {
  struct wire_uint_8_list *index;
  struct wire_uint_8_list *note;
} wire_UpdatePaymentNote;

void store_dart_post_cobject(DartPostCObjectFnType ptr);

Dart_Handle get_dart_object(uintptr_t ptr);

void drop_dart_object(uintptr_t ptr);

uintptr_t new_dart_opaque(Dart_Handle handle);

intptr_t init_frb_dart_api_dl(void *obj);

WireSyncReturn wire_deploy_env_from_str(struct wire_uint_8_list *s);

WireSyncReturn wire_network_from_str(struct wire_uint_8_list *s);

WireSyncReturn wire_gen_client_payment_id(void);

WireSyncReturn wire_form_validate_bitcoin_address(struct wire_uint_8_list *address_str,
                                                  int32_t current_network);

WireSyncReturn wire_form_validate_password(struct wire_uint_8_list *password);

void wire_payment_uri_resolve_best(int64_t port_,
                                   int32_t network,
                                   struct wire_uint_8_list *uri_str);

void wire_init_rust_log_stream(int64_t port_, struct wire_uint_8_list *rust_log);

WireSyncReturn wire_debug_delete_secret_store(struct wire_Config *config);

WireSyncReturn wire_debug_delete_latest_provisioned(struct wire_Config *config);

void wire_debug_unconditional_panic(int64_t port_);

void wire_debug_unconditional_error(int64_t port_);

void wire_load__static_method__AppHandle(int64_t port_, struct wire_Config *config);

void wire_restore__static_method__AppHandle(int64_t port_,
                                            struct wire_Config *config,
                                            struct wire_uint_8_list *seed_phrase);

void wire_signup__static_method__AppHandle(int64_t port_,
                                           struct wire_Config *config,
                                           struct wire_uint_8_list *google_auth_code,
                                           struct wire_uint_8_list *password);

void wire_node_info__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

void wire_fiat_rates__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

void wire_send_onchain__method__AppHandle(int64_t port_,
                                          struct wire_AppHandle *that,
                                          struct wire_SendOnchainRequest *req);

void wire_estimate_fee_send_onchain__method__AppHandle(int64_t port_,
                                                       struct wire_AppHandle *that,
                                                       struct wire_EstimateFeeSendOnchainRequest *req);

void wire_get_address__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

void wire_create_invoice__method__AppHandle(int64_t port_,
                                            struct wire_AppHandle *that,
                                            struct wire_CreateInvoiceRequest *req);

void wire_pay_invoice__method__AppHandle(int64_t port_,
                                         struct wire_AppHandle *that,
                                         struct wire_PayInvoiceRequest *req);

void wire_delete_payment_db__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

void wire_sync_payments__method__AppHandle(int64_t port_, struct wire_AppHandle *that);

WireSyncReturn wire_get_payment_by_vec_idx__method__AppHandle(struct wire_AppHandle *that,
                                                              uintptr_t vec_idx);

WireSyncReturn wire_get_short_payment_by_scroll_idx__method__AppHandle(struct wire_AppHandle *that,
                                                                       uintptr_t scroll_idx);

WireSyncReturn wire_get_pending_short_payment_by_scroll_idx__method__AppHandle(struct wire_AppHandle *that,
                                                                               uintptr_t scroll_idx);

WireSyncReturn wire_get_finalized_short_payment_by_scroll_idx__method__AppHandle(struct wire_AppHandle *that,
                                                                                 uintptr_t scroll_idx);

WireSyncReturn wire_get_pending_not_junk_short_payment_by_scroll_idx__method__AppHandle(struct wire_AppHandle *that,
                                                                                        uintptr_t scroll_idx);

WireSyncReturn wire_get_finalized_not_junk_short_payment_by_scroll_idx__method__AppHandle(struct wire_AppHandle *that,
                                                                                          uintptr_t scroll_idx);

WireSyncReturn wire_get_num_payments__method__AppHandle(struct wire_AppHandle *that);

WireSyncReturn wire_get_num_pending_payments__method__AppHandle(struct wire_AppHandle *that);

WireSyncReturn wire_get_num_finalized_payments__method__AppHandle(struct wire_AppHandle *that);

WireSyncReturn wire_get_num_pending_not_junk_payments__method__AppHandle(struct wire_AppHandle *that);

WireSyncReturn wire_get_num_finalized_not_junk_payments__method__AppHandle(struct wire_AppHandle *that);

void wire_update_payment_note__method__AppHandle(int64_t port_,
                                                 struct wire_AppHandle *that,
                                                 struct wire_UpdatePaymentNote *req);

struct wire_App new_App(void);

struct wire_AppHandle *new_box_autoadd_app_handle_0(void);

struct wire_Config *new_box_autoadd_config_0(void);

struct wire_CreateInvoiceRequest *new_box_autoadd_create_invoice_request_0(void);

struct wire_EstimateFeeSendOnchainRequest *new_box_autoadd_estimate_fee_send_onchain_request_0(void);

struct wire_PayInvoiceRequest *new_box_autoadd_pay_invoice_request_0(void);

struct wire_SendOnchainRequest *new_box_autoadd_send_onchain_request_0(void);

uint64_t *new_box_autoadd_u64_0(uint64_t value);

struct wire_UpdatePaymentNote *new_box_autoadd_update_payment_note_0(void);

struct wire_uint_8_list *new_uint_8_list_0(int32_t len);

void drop_opaque_App(const void *ptr);

const void *share_opaque_App(const void *ptr);

void free_WireSyncReturn(WireSyncReturn ptr);

static int64_t dummy_method_to_enforce_bundling(void) {
    int64_t dummy_var = 0;
    dummy_var ^= ((int64_t) (void*) wire_deploy_env_from_str);
    dummy_var ^= ((int64_t) (void*) wire_network_from_str);
    dummy_var ^= ((int64_t) (void*) wire_gen_client_payment_id);
    dummy_var ^= ((int64_t) (void*) wire_form_validate_bitcoin_address);
    dummy_var ^= ((int64_t) (void*) wire_form_validate_password);
    dummy_var ^= ((int64_t) (void*) wire_payment_uri_resolve_best);
    dummy_var ^= ((int64_t) (void*) wire_init_rust_log_stream);
    dummy_var ^= ((int64_t) (void*) wire_debug_delete_secret_store);
    dummy_var ^= ((int64_t) (void*) wire_debug_delete_latest_provisioned);
    dummy_var ^= ((int64_t) (void*) wire_debug_unconditional_panic);
    dummy_var ^= ((int64_t) (void*) wire_debug_unconditional_error);
    dummy_var ^= ((int64_t) (void*) wire_load__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_restore__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_signup__static_method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_node_info__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_fiat_rates__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_send_onchain__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_estimate_fee_send_onchain__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_address__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_create_invoice__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_pay_invoice__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_delete_payment_db__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_sync_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_payment_by_vec_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_short_payment_by_scroll_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_pending_short_payment_by_scroll_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_finalized_short_payment_by_scroll_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_pending_not_junk_short_payment_by_scroll_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_finalized_not_junk_short_payment_by_scroll_idx__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_num_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_num_pending_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_num_finalized_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_num_pending_not_junk_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_get_num_finalized_not_junk_payments__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) wire_update_payment_note__method__AppHandle);
    dummy_var ^= ((int64_t) (void*) new_App);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_app_handle_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_config_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_create_invoice_request_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_estimate_fee_send_onchain_request_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_pay_invoice_request_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_send_onchain_request_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_u64_0);
    dummy_var ^= ((int64_t) (void*) new_box_autoadd_update_payment_note_0);
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
