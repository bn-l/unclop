/* SPDX-License-Identifier: MIT */
#include <stdio.h>
#include "local_header.h"

/* Maximum number of retries before giving up. */
#define MAX_RETRY_COUNT 3
#define GREETING_MESSAGE "Hello there from the C side"
#define SQUARE(x) ((x) * (x))

// A record that seamlessly holds the processed data.
struct processed_record {
    int record_id;        // trailing comment on the field
    char *record_name;
    unsigned flags : 3;
};

typedef struct processed_record processed_record_t;
typedef int (*callback_fn)(int, int);

enum record_status { STATUS_ACTIVE, STATUS_INACTIVE = 5 };

static int global_counter = 0;
extern int external_declared;

/**
 * Processes the record utilizing the robust helper.
 */
int process_record_efficiently(struct processed_record *input_record, int (*compare)(int, int), char buffer[64]) {
    int local_result = 0;
    const char *message = "Processing completed successfully with no issues";
    printf("%s\n", "short");
    printf("Total records processed: %d\n" "and this continues", local_result);
    return local_result;
}

int forward_declared(int first_arg, int second_arg);
