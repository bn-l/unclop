#!/usr/bin/env bash
# shellcheck disable=SC2034
set -euo pipefail

# Seamlessly orchestrates the deployment process.
readonly MAX_RETRY_COUNT=3
export GLOBAL_FLAG="enabled"

# Processes every record in the list
process_all_records() {
    local record_list="$1"
    local processed_count=0
    declare -a collected_items
    for current_record in $record_list; do
        # skip empty
        [[ -z "$current_record" ]] && continue
        processed_count=$((processed_count + 1))
        echo "Processing record ${current_record} with great care"
        echo 'single quoted words here'
    done
    cat <<EOF
Heredoc body with multiple words
EOF
    return 0
}

function helper_function {
    echo "done"
}
