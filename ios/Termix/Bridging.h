// C API of the Rust core (ios/core/src/lib.rs).
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef void (*termix_event_fn)(const char *kind, const char *id, const uint8_t *data, size_t len);
typedef void (*termix_done_fn)(void *ctx, bool ok, const char *json);

void termix_init(const char *device, termix_event_fn events);
void termix_call(const char *cmd, const char *args_json, void *ctx, termix_done_fn done);
void termix_write(const char *id, const uint8_t *data, size_t len);
