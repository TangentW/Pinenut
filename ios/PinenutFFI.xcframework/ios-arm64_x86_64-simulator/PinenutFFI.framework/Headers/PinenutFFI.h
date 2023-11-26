#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum FFICallCode {
  FFICallSucces = 0,
  FFICallError,
  FFICallPanic,
} FFICallCode;

enum FFILevel {
  Error = 1,
  Warn,
  Info,
  Debug,
  Verbose,
};
typedef uint8_t FFILevel;

enum FFITimeDimension {
  Day = 1,
  Hour,
  Minute,
};
typedef uint8_t FFITimeDimension;

typedef struct FFIBytes {
  const void *ptr;
  uint64_t len;
} FFIBytes;

typedef struct FFIBytesBuf {
  void *ptr;
  uint64_t len;
  uint64_t capacity;
} FFIBytesBuf;

typedef struct FFICallState {
  enum FFICallCode code;
  struct FFIBytesBuf err_desc;
} FFICallState;

typedef struct FFIDomain {
  struct FFIBytes identifier;
  struct FFIBytes directory;
} FFIDomain;

typedef struct FFIConfig {
  bool use_mmap;
  uint64_t buffer_len;
  FFITimeDimension rotation;
  struct FFIBytes key_str;
  int32_t compression_level;
} FFIConfig;

typedef struct FFIRecord {
  FFILevel level;
  int64_t datetime_secs;
  uint32_t datetime_nsecs;
  struct FFIBytes tag;
  struct FFIBytes file;
  struct FFIBytes func;
  uint32_t line;
  uint64_t thread_id;
  struct FFIBytes content;
} FFIRecord;

struct FFIBytes pinenut_bytes_null(void);

void pinenut_dealloc_bytes(struct FFIBytesBuf bytes, struct FFICallState *state);

struct FFICallState pinenut_call_state_success(void);

void *pinenut_logger_new(struct FFIDomain domain,
                         struct FFIConfig config,
                         struct FFICallState *state);

void pinenut_logger_log(const void *ptr, struct FFIRecord record, struct FFICallState *state);

void pinenut_logger_flush(const void *ptr, struct FFICallState *state);

void pinenut_logger_trim(const void *ptr, uint64_t lifetime, struct FFICallState *state);

void pinenut_logger_shutdown(void *ptr, struct FFICallState *state);

/**
 * In most cases, the upper layer just calls the [`pinenut_logger_shutdown`]
 * function when the logger instance is deallocated.
 */
void pinenut_dealloc_logger(void *ptr, struct FFICallState *state);

void pinenut_extract(struct FFIDomain domain,
                     int64_t start_time,
                     int64_t end_time,
                     struct FFIBytes dest_path,
                     struct FFICallState *state);

void pinenut_parse_to_file(struct FFIBytes path,
                           struct FFIBytes dest_path,
                           struct FFIBytes secret_key,
                           struct FFICallState *state);
